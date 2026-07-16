use crate::downloader::DownloaderHandle;
use crate::magic::MagicHandle;
use crate::magic::structure::ConfigPackage;
use crate::session::SessionHandle;
use crate::shutdown::ShutdownSignals;
use crate::utils::network::NetworkClient;
use anyhow::Context;
use anyhow::Result;
use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;
use std::path::PathBuf;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time::{self, Duration};
use tracing::{error, info, warn};

const MAX_INSTALL_RETRIES: u32 = 3;

#[derive(Clone, Debug)]
enum InstallFailureKind {
    CorruptPackage,
    SystemError,
    Unknown,
}

impl std::fmt::Display for InstallFailureKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallFailureKind::CorruptPackage => write!(f, "CorruptPackage"),
            InstallFailureKind::SystemError => write!(f, "SystemError"),
            InstallFailureKind::Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Clone, Debug)]
struct PackageFailure {
    consecutive_failures: u32,
    last_failure_kind: InstallFailureKind,
}

/// Failure modes of the single-transaction install. A timeout is kept separate
/// from an ordinary failure because the killed child is only our `sudo`
/// wrapper — the apt transaction itself may still be running and holding the
/// dpkg lock, so the caller must not start competing installs.
#[derive(Debug)]
enum BatchInstallError {
    TimedOut { seconds: u64 },
    Failed { detail: String },
}

fn classify_install_failure(stderr: &str) -> InstallFailureKind {
    let stderr_lower = stderr.to_lowercase();

    let corrupt_patterns = [
        "is not a debian format archive",
        "archive is corrupt",
        "unexpected end of file",
        "could not read meta data",
    ];

    for pattern in &corrupt_patterns {
        if stderr_lower.contains(pattern) {
            return InstallFailureKind::CorruptPackage;
        }
    }

    let system_patterns = [
        "dpkg was interrupted",
        "dependency problems",
        "conflicts with",
        "no space left on device",
        "unable to access dpkg",
        "unable to acquire the dpkg frontend lock",
        "could not get lock",
        "unmet dependencies",
        "broken packages",
    ];

    for pattern in &system_patterns {
        if stderr_lower.contains(pattern) {
            return InstallFailureKind::SystemError;
        }
    }

    InstallFailureKind::Unknown
}

#[derive(Debug)]
pub enum ActorMessage {
    Update,
    Upgrade,
    Check,
    StatusReport { rpc: oneshot::Sender<String> },
}

#[derive(Clone, Debug, PartialEq)]
enum Status {
    Idle,
    Updating,
    Upgrading,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Idle => write!(f, "Idle"),
            Status::Updating => write!(f, "Updating"),
            Status::Upgrading => write!(f, "Upgrading"),
        }
    }
}

/// Updater Actor
pub struct Actor {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<ActorMessage>,
    magic: MagicHandle,
    session: SessionHandle,
    status: Status,
    network: NetworkClient,
    last_update: Option<Result<time::Instant>>,
    last_upgrade: Option<Result<time::Instant>>,
    downloader: DownloaderHandle,
    install_failures: HashMap<String, PackageFailure>,
    packages_dir: PathBuf,
}

impl Actor {
    pub fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<ActorMessage>,
        magic: MagicHandle,
        downloader: DownloaderHandle,
        session: SessionHandle,
    ) -> Self {
        let network = NetworkClient::new();

        //if this unwrap fails, there's no point continuing
        let smith_home = std::env::current_dir().unwrap();
        let packages_dir = smith_home.join("packages");

        Self {
            shutdown,
            receiver,
            magic,
            session,
            network,
            status: Status::Idle,
            last_update: None,
            last_upgrade: None,
            downloader,
            install_failures: HashMap::new(),
            packages_dir,
        }
    }

    async fn run_dpkg_recovery_static() -> Result<()> {
        info!("Running dpkg recovery using systemd-run with 5 minute timeout");
        let recovery_command = "systemd-run --unit=dpkg-fix --description='Finish broken configs' --property=Type=oneshot --no-ask-password dpkg --configure -a";

        let recovery_future = Command::new("sh")
            .arg("-c")
            .arg(recovery_command)
            .kill_on_drop(true)
            .output();

        let output = match time::timeout(Duration::from_secs(300), recovery_future).await {
            Ok(result) => result.with_context(|| "Failed to execute dpkg recovery command")?,
            Err(_) => {
                error!("dpkg recovery timed out after 5 minutes");
                return Err(anyhow::anyhow!("dpkg recovery timed out"));
            }
        };

        if output.status.success() {
            info!("Dpkg recovery completed successfully");
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow::anyhow!("Dpkg recovery failed: {}", stderr))
        }
    }

    /// Record an install failure for a package. Returns `true` if the `.deb` file
    /// should be deleted (only for corrupt-package errors where re-download may help).
    fn handle_install_failure(&mut self, package_name: &str, kind: InstallFailureKind) -> bool {
        let entry = self
            .install_failures
            .entry(package_name.to_string())
            .or_insert(PackageFailure {
                consecutive_failures: 0,
                last_failure_kind: kind.clone(),
            });
        entry.consecutive_failures += 1;
        entry.last_failure_kind = kind.clone();

        matches!(kind, InstallFailureKind::CorruptPackage)
    }

    fn should_skip_install(&self, package_name: &str) -> bool {
        if let Some(failure) = self.install_failures.get(package_name)
            && failure.consecutive_failures >= MAX_INSTALL_RETRIES
        {
            warn!(
                "Skipping install of {} after {} consecutive failures (last: {})",
                package_name, failure.consecutive_failures, failure.last_failure_kind
            );
            return true;
        }
        false
    }

    async fn handle_message(&mut self, msg: ActorMessage) {
        match msg {
            ActorMessage::Update => {
                self.update().await;
            }
            ActorMessage::Upgrade => {
                self.upgrade().await;
            }
            ActorMessage::Check => {
                let release_id = self.magic.get_release_id().await.ok();
                let target_release_id = self.magic.get_target_release_id().await.ok();

                if release_id != target_release_id {
                    self.install_failures.clear();

                    self.update().await;

                    if matches!(self.last_update, Some(Err(_)) | None) {
                        return;
                    }

                    self.upgrade().await;
                }
            }
            ActorMessage::StatusReport { rpc } => {
                let interval = |time: time::Instant| {
                    let duration = time.elapsed();
                    let seconds = duration.as_secs();
                    let minutes = seconds / 60;
                    let hours = minutes / 60;
                    let days = hours / 24;

                    if days > 0 {
                        format!("{} days ago", days)
                    } else if hours > 0 {
                        format!("{} hours ago", hours)
                    } else if minutes > 0 {
                        format!("{} minutes ago", minutes)
                    } else {
                        format!("{} seconds ago", seconds)
                    }
                };

                let last_update_string = match &self.last_update {
                    Some(Ok(time)) => interval(*time),
                    Some(Err(err)) => format!("Error: {}", err),
                    None => "Never".to_string(),
                };

                let last_upgrade_string = match &self.last_upgrade {
                    Some(Ok(time)) => interval(*time),
                    Some(Err(err)) => format!("Error: {}", err),
                    None => "Never".to_string(),
                };

                let status_string = format!(
                    "Status: {} | Last Update: {} | Last Upgrade: {}",
                    self.status, last_update_string, last_upgrade_string
                );

                let _rpc = rpc.send(status_string);
            }
        }
    }

    #[tracing::instrument(skip(self))]
    async fn update(&mut self) {
        info!("Checking for updates");
        self.status = Status::Updating;
        let res = self.check_for_updates().await.map(|_| time::Instant::now());
        info!("Updating result: {:?}", res);
        self.last_update = Some(res);
        self.status = Status::Idle;
    }

    async fn upgrade(&mut self) {
        info!("Upgrading device");
        self.status = Status::Upgrading;
        let res = self.upgrade_device().await.map(|_| time::Instant::now());
        info!("Upgrading result: {:?}", res);
        self.last_upgrade = Some(res);
        self.status = Status::Idle;
    }

    async fn write_manifest(&self, path: &Path, contents: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, contents)
            .await
            .with_context(|| format!("writing manifest to {}", path.display()))?;
        Ok(())
    }

    async fn fetch_blob(&self, package: &ConfigPackage, blob_path: &Path) -> Result<()> {
        if let Some(parent) = blob_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // TODO: remove when legacy /packages layout is fully migrated.
        let legacy_path = self.packages_dir.join(&package.file);
        if legacy_path.exists() {
            warn!(?legacy_path, ?blob_path, "migrating from legacy layout");
            tokio::fs::rename(&legacy_path, blob_path).await?;
            return Ok(());
        }

        let remote = format!("packages/{}", package.file);
        let download_to = blob_path
            .to_str()
            .ok_or(anyhow::anyhow!("Failed to unwrap blob path"))?;

        info!(?remote, "downloading");
        self.downloader
            // 2 MB/s keeps us friendly on constrained networks.
            .download_blocking(&remote, download_to, 2.0)
            .await?;

        Ok(())
    }

    /// Returns Ok(true) if the blob exists and looks usable.
    /// Removes zero-byte files as a side effect so they'll be re-downloaded.
    async fn blob_is_valid(&self, blob_path: &Path) -> Result<bool> {
        if !blob_path.exists() {
            return Ok(false);
        }
        let metadata = tokio::fs::metadata(blob_path)
            .await
            .with_context(|| format!("stat {}", blob_path.display()))?;
        if metadata.len() == 0 {
            warn!(?blob_path, "zero-byte blob, removing for re-download");
            tokio::fs::remove_file(blob_path).await?;
            return Ok(false);
        }
        Ok(true)
    }

    async fn ensure_release_cache(&self, release_id: i32) -> Result<()> {
        info!("ensuring release cache for release_id: {release_id}");

        let release_cache = self
            .packages_dir
            .join("versions")
            .join(release_id.to_string());

        if release_cache.exists() {
            info!("release cache exists, skipping download");
            return Ok(());
        }

        // Prefer the short-lived device JWT; falls back to the opaque token when
        // no valid JWT is cached (see SessionHandle::bearer_token).
        let token = self.session.bearer_token().await.unwrap_or_default();

        let release_packages = self
            .network
            .get_release_packages(release_id, &token)
            .await
            .with_context(|| "failed to fetch release packages manifest")?;

        let blobs = self.packages_dir.join("blobs");
        let mut manifest = String::new();
        let mut all_cached = true;

        for package in &release_packages {
            info!("Processing package: {}", package.file);
            let blob_path = blobs.join(&package.file);

            if self.blob_is_valid(&blob_path).await? {
                info!("blob present in cache");
                writeln!(
                    manifest,
                    "{} {} {}",
                    package.name, package.version, package.file
                )?;
                continue;
            }

            self.fetch_blob(package, &blob_path)
                .await
                .with_context(|| format!("fetching blob for package {}", package.file))?;
            all_cached = false;
        }

        if all_cached {
            self.write_manifest(&release_cache, &manifest).await?;
            info!(release_id, "release cache ready");
        } else {
            info!(
                release_id,
                "blobs being fetched; manifest write deferred to next call"
            );
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    async fn check_for_updates(&self) -> Result<()> {
        // apt update on check for updates with timeout
        info!("Running apt update with 5 minute timeout");
        let apt_update_future = Command::new("sh")
            .arg("-c")
            .arg("apt update -y")
            .kill_on_drop(true)
            .output();

        match time::timeout(Duration::from_secs(300), apt_update_future).await {
            Ok(result) => {
                result.with_context(|| "Failed to run apt update")?;
            }
            Err(_) => {
                error!("apt update timed out after 5 minutes");
                return Err(anyhow::anyhow!("apt update timed out"));
            }
        }

        // TODO: take a look at this once we clean up the smith install flow
        // on new devices
        match self.magic.get_release_id().await {
            Ok(current_release_id) => {
                self.ensure_release_cache(current_release_id)
                    .await
                    .with_context(|| "Failed to ensure current release cache")?;
            }
            Err(err) => {
                warn!(
                    error = ?err,
                    "Skipping current release cache warm-up because current release id is unavailable"
                );
            }
        }

        let target_release_id = self
            .magic
            .get_target_release_id()
            .await
            .with_context(|| "Failed to get Target Release ID")?;

        self.ensure_release_cache(target_release_id)
            .await
            .with_context(|| "Failed to ensure target release cache")?;

        Ok(())
    }

    async fn upgrade_device(&mut self) -> Result<()> {
        // Check if previous update was successful
        match self.last_update {
            Some(Ok(time)) => {
                let time_since_last_update = time.elapsed();
                info!(
                    "Previous update was successful {:?}",
                    time_since_last_update
                );
            }
            Some(_) => {
                warn!("Previous update was not successful");
                return Ok(());
            }
            None => {
                info!("No previous update, continuing anyway");
            }
        }

        let target_release_id = self
            .magic
            .get_target_release_id()
            .await
            .with_context(|| "Failed to get Target Release ID")?;

        let blobs = self.packages_dir.join("blobs");
        let release_cache = self
            .packages_dir
            .join("versions")
            .join(target_release_id.to_string());

        // read the file from release cache
        let content = tokio::fs::read(&release_cache).await?;
        let content = std::str::from_utf8(&content)?;

        let packages: Vec<ConfigPackage> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let mut parts = line.splitn(3, ' ');
                Ok::<_, anyhow::Error>(ConfigPackage {
                    name: parts
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("missing name"))?
                        .to_string(),
                    version: parts
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("missing version"))?
                        .to_string(),
                    file: parts
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("missing file"))?
                        .to_string(),
                })
            })
            .collect::<Result<_, _>>()?;

        // check if all packages are available locally
        for package in &packages {
            info!("Checking package: {}", package.name);
            let package_name = &package.name;
            let package_file = &package.file;

            // check if package is available locally
            let package_file = blobs.join(package_file);

            if package_file.exists() {
                info!("Package {} exists locally", package_name);
                continue;
            } else {
                info!("Package {} does not exist locally", package_name);
                return Err(anyhow::anyhow!(
                    "Package {} does not exist locally",
                    package_name
                ));
            }
        }

        // now install packages
        let mut update_smith = false;
        let mut to_install: Vec<(String, PathBuf)> = Vec::new();
        for package in packages {
            let package_name = package.name;
            let package_file = package.file;
            let package_version = package.version;

            if self.should_skip_install(&package_name) {
                continue;
            }

            let package_file = blobs.join(package_file);

            // check if version on system is the one we should be running
            let output = match Command::new("dpkg")
                .arg("-l")
                .arg(&package_name)
                .output()
                .await
            {
                Ok(output) => output,
                Err(e) => {
                    error!("Failed to execute dpkg command for {}: {}", package_name, e);
                    continue;
                }
            };

            let mut package_installed = false;
            if output.status.success() {
                if let Ok(stdout) = String::from_utf8(output.stdout) {
                    let lines: Vec<&str> = stdout.lines().collect();
                    if let Some(package_info) = lines.get(5) {
                        let fields: Vec<&str> = package_info.split_whitespace().collect();
                        if let Some(version) = fields.get(2) {
                            info!("> {} | {} => {}", package_name, version, &package_version);
                            package_installed = version == &package_version;
                        } else {
                            error!("Failed to get version for package {}", package_name);
                        }
                    } else {
                        error!("Failed to get package info for {}", package_name);
                    }
                } else {
                    error!("Failed to parse dpkg output for {}", package_name);
                }
            }

            if !package_installed {
                if package_name == "smith" || package_name == "smith_amd64" {
                    update_smith = true;
                    continue;
                }
                to_install.push((package_name, package_file));
            }
        }

        // Install everything in a single apt transaction. For a flat set of
        // application debs like ours (no Essential/Pre-Depends relations), apt
        // unpacks every archive before it runs any package's postinst, so a
        // service restarted by its postinst starts once, against the complete
        // new file set. Installing one package at a time restarted services
        // mid-release against a mix of old and new files, in whatever order
        // the release manifest happened to list the packages.
        //
        // dpkg keeps going past a failing package for the independent rest of
        // the transaction, and packages that did reach their target version
        // are filtered out of `to_install` above — so after a partial failure
        // the next Check retries only what is still behind. No per-package
        // fallback is needed.
        if !to_install.is_empty() {
            match self.batch_install(&to_install).await {
                Ok(()) => {
                    for (package_name, _) in &to_install {
                        self.install_failures.remove(package_name);
                    }
                }
                Err(BatchInstallError::TimedOut { seconds }) => {
                    // kill_on_drop only reaches our direct child; apt (run via
                    // sudo) survives orphaned and may still hold the dpkg lock
                    // and complete on its own. Don't start anything else that
                    // needs dpkg — the periodic Check re-drives the upgrade.
                    error!(
                        "Batch install timed out after {} seconds; the apt transaction may still be running",
                        seconds
                    );
                    return Err(anyhow::anyhow!(
                        "batch install timed out after {seconds} seconds"
                    ));
                }
                Err(BatchInstallError::Failed { detail }) => {
                    error!("Batch install failed:\n{detail}");
                    self.handle_batch_failure(&to_install, &detail).await;
                    return Err(anyhow::anyhow!("batch install failed"));
                }
            }
        }

        if update_smith {
            let status = Command::new("sh")
                .arg("-c")
                .arg("sudo systemctl start smith-updater")
                .output()
                .await
                .with_context(|| "Failed to stop smith service")?;

            if !status.status.success() {
                error!("Failed to start smith updater {:?}", status);
            }
        }

        self.are_packages_up_to_date().await?;

        self.magic.set_release_id(target_release_id).await;

        self.clean_up_old_packages().await
    }

    /// Install all pending packages with one apt invocation so every archive is
    /// unpacked before any postinst runs. On timeout the caller must not start
    /// anything else that needs dpkg — the orphaned apt may still hold the lock.
    async fn batch_install(
        &self,
        to_install: &[(String, PathBuf)],
    ) -> Result<(), BatchInstallError> {
        let names = to_install
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        // Same 5-minute-per-package budget the sequential installs had.
        let timeout = Duration::from_secs(300 * to_install.len() as u64);
        info!(
            "Installing {} package(s) in one transaction with {} minute timeout: {}",
            to_install.len(),
            timeout.as_secs() / 60,
            names
        );

        // No shell: arguments are passed directly so file names from the
        // release manifest can't be interpreted by `sh`.
        let install_future = Command::new("sudo")
            .arg("apt")
            .arg("install")
            .arg("-y")
            .arg("--allow-downgrades")
            .args(to_install.iter().map(|(_, file)| file.as_os_str()))
            .kill_on_drop(true)
            .output();

        let output = match time::timeout(timeout, install_future).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                return Err(BatchInstallError::Failed {
                    detail: format!("failed to run batch install command: {e}"),
                });
            }
            Err(_) => {
                return Err(BatchInstallError::TimedOut {
                    seconds: timeout.as_secs(),
                });
            }
        };

        if output.status.success() {
            info!("Successfully installed {} package(s)", to_install.len());
            Ok(())
        } else {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(BatchInstallError::Failed {
                detail: format!(
                    "batch install exited with {}:\nstderr: {}\nstdout: {}",
                    output.status, stderr, stdout
                ),
            })
        }
    }

    /// Apply the per-package remedies to a failed batch: evict corrupt blobs
    /// named in the apt output so they get re-downloaded, and finish an
    /// interrupted dpkg configuration synchronously so the next attempt starts
    /// from a clean state.
    async fn handle_batch_failure(&mut self, to_install: &[(String, PathBuf)], detail: &str) {
        if matches!(
            classify_install_failure(detail),
            InstallFailureKind::CorruptPackage
        ) {
            for (package_name, package_file) in to_install {
                let named_in_output = package_file
                    .file_name()
                    .and_then(|f| f.to_str())
                    .is_some_and(|f| detail.contains(f));
                if !named_in_output {
                    continue;
                }

                self.handle_install_failure(package_name, InstallFailureKind::CorruptPackage);
                if let Err(e) = tokio::fs::remove_file(package_file).await {
                    error!(
                        "Failed to remove package file {}: {}",
                        package_file.display(),
                        e
                    );
                } else {
                    info!(
                        "Removed package file {} so it will be re-downloaded",
                        package_file.display()
                    );
                }
            }
        }

        // A batch that died between dpkg's unpack and configure phases leaves
        // packages half-installed; recover before the next attempt.
        if detail.contains("dpkg was interrupted") && detail.contains("dpkg --configure -a") {
            info!("Detected dpkg interruption after batch install, running recovery");
            if let Err(e) = Self::run_dpkg_recovery_static().await {
                error!("Dpkg recovery failed: {}", e);
            }
        }
    }

    async fn clean_up_old_packages(&self) -> Result<()> {
        // for now we are gonna delete packages in /packages
        // TODO: lets improve on the caching mechanism later
        let mut entries = tokio::fs::read_dir(&self.packages_dir).await?;
        let mut bytes_freed: u64 = 0;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("deb") {
                let size = entry.metadata().await.map(|m| m.len()).unwrap_or(0);
                if let Err(e) = tokio::fs::remove_file(&path).await {
                    error!("Failed to remove old package {}: {}", path.display(), e);
                } else {
                    bytes_freed += size;
                }
            }
        }

        info!(
            "Cleaned up old packages, freed {} MB",
            bytes_freed / 1024 / 1024
        );
        Ok(())
    }

    /// Checks whether packages are up to date.
    ///
    /// Returns `Ok` if all packages are, `Err` otherwise.
    async fn are_packages_up_to_date(&self) -> Result<()> {
        let target_release_id = self
            .magic
            .get_target_release_id()
            .await
            .with_context(|| "Failed to get Target Release ID")?;

        let release_cache = self
            .packages_dir
            .join("versions")
            .join(target_release_id.to_string());

        // read the file from release cache
        let content = tokio::fs::read(&release_cache).await?;
        let content = std::str::from_utf8(&content)?;

        let packages: Vec<ConfigPackage> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let mut parts = line.splitn(3, ' ');
                Ok::<_, anyhow::Error>(ConfigPackage {
                    name: parts
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("missing name"))?
                        .to_string(),
                    version: parts
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("missing version"))?
                        .to_string(),
                    file: parts
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("missing file"))?
                        .to_string(),
                })
            })
            .collect::<Result<_, _>>()?;

        // check the system version of the packages in the magic file
        for package in packages {
            let (status, installed_version) = package.get_system_state().await?;
            let magic_toml_version = package.version;

            if magic_toml_version != installed_version {
                return Err(anyhow::anyhow!(
                    "Package {} is not up to date",
                    package.name
                ));
            }

            // dpkg reports the target version as soon as an archive is
            // unpacked; require fully-configured "ii" so an install that died
            // between dpkg's unpack and configure phases can't pass this gate
            // and advance the release id.
            if status != "ii" {
                return Err(anyhow::anyhow!(
                    "Package {} is at the target version but not fully configured (dpkg status {})",
                    package.name,
                    status
                ));
            }
        }

        Ok(())
    }

    pub async fn run(&mut self) {
        info!("Updater Starting");
        let hostname = self.magic.get_server().await;
        self.network.set_hostname(hostname);

        let mut update_check_interval = tokio::time::interval(tokio::time::Duration::from_secs(60));

        loop {
            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    info!("Received Message");
                    self.handle_message(msg).await;
                }
                _ = update_check_interval.tick() => {
                    self.handle_message(ActorMessage::Check).await;
                }
                _ = self.shutdown.token.cancelled() => {
                    info!("Updater waiting for tasks to finish");
                    break;
                }
            }
        }
        info!("Updater shutting down");
    }
}
