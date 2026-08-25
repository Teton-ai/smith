use crate::downloader::DownloaderHandle;
use crate::magic::MagicHandle;
use crate::magic::structure::ConfigPackage;
use crate::session::SessionHandle;
use crate::shutdown::ShutdownSignals;
use crate::utils::network::NetworkClient;
use anyhow::Context;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time::{self, Duration};
use tracing::{error, info, warn};

const MAX_INSTALL_RETRIES: u32 = 3;
const MAX_BLOB_CACHE_BYTES: u64 = 3 * 1024 * 1024 * 1024;
const RETAINED_RELEASES: usize = 3;

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
struct CachedRelease {
    manifest_path: PathBuf,
    blobs: HashSet<PathBuf>,
    modified: SystemTime,
}

fn manifest_blob_paths(manifest: &str, blobs_dir: &Path) -> Result<HashSet<PathBuf>> {
    manifest
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let file = line
                .splitn(3, ' ')
                .nth(2)
                .ok_or_else(|| anyhow::anyhow!("malformed package manifest line: {line}"))?;
            let relative_path = Path::new(file);
            if relative_path.as_os_str().is_empty()
                || relative_path
                    .components()
                    .any(|component| !matches!(component, Component::Normal(_)))
            {
                anyhow::bail!("unsafe blob path in package manifest: {file}");
            }
            Ok(blobs_dir.join(relative_path))
        })
        .collect()
}

async fn files_in(directory: &Path) -> Result<Vec<(PathBuf, u64)>> {
    if !directory.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    let mut directories = vec![directory.to_path_buf()];
    while let Some(directory) = directories.pop() {
        let mut entries = tokio::fs::read_dir(&directory)
            .await
            .with_context(|| format!("reading {}", directory.display()))?;
        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                directories.push(entry.path());
            } else if file_type.is_file() {
                files.push((entry.path(), entry.metadata().await?.len()));
            }
        }
    }
    Ok(files)
}

async fn remove_file(path: &Path) -> Result<u64> {
    let size = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("stat {}", path.display()))?
        .len();
    tokio::fs::remove_file(path)
        .await
        .with_context(|| format!("removing {}", path.display()))?;
    Ok(size)
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

        let previous_release_id = self.magic.get_release_id().await;
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
            if self.should_skip_install(&package.name) {
                continue;
            }

            // A failed postinst sits at the target version with status "iF" — require "ii".
            let package_installed = match package.get_system_state().await {
                Ok((status, version)) => {
                    info!("> {} | {} => {}", package.name, version, package.version);
                    status == "ii" && version == package.version
                }
                Err(_) => {
                    info!("> {} | not installed => {}", package.name, package.version);
                    false
                }
            };

            if !package_installed {
                if package.name == "smith" || package.name == "smith_amd64" {
                    update_smith = true;
                    continue;
                }
                let blob_path = blobs.join(&package.file);
                to_install.push((package.name, blob_path));
            }
        }

        // One apt transaction: everything is unpacked before any postinst runs.
        if !to_install.is_empty() {
            match self.batch_install(&to_install).await {
                Ok(()) => {
                    for (package_name, _) in &to_install {
                        self.install_failures.remove(package_name);
                    }
                }
                Err(BatchInstallError::TimedOut { seconds }) => {
                    // apt may still be running and holding the dpkg lock.
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

        match previous_release_id {
            Ok(previous_release_id) if previous_release_id != target_release_id => {
                if let Err(err) = Self::clean_up_old_packages(
                    &self.packages_dir,
                    target_release_id,
                    previous_release_id,
                )
                .await
                {
                    error!(
                        error = ?err,
                        target_release_id,
                        previous_release_id,
                        "Package cache cleanup failed after a successful upgrade"
                    );
                }
            }
            Ok(_) => {
                info!("Package cache cleanup skipped because the release did not change");
            }
            Err(err) => {
                warn!(
                    error = ?err,
                    "Package cache cleanup skipped because no previous release is known"
                );
            }
        }

        Ok(())
    }

    async fn batch_install(
        &self,
        to_install: &[(String, PathBuf)],
    ) -> Result<(), BatchInstallError> {
        let names = to_install
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        let timeout = Duration::from_secs(300 * to_install.len() as u64);
        info!(
            "Installing {} package(s) in one transaction with {} minute timeout: {}",
            to_install.len(),
            timeout.as_secs() / 60,
            names
        );

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

                if !self.handle_install_failure(package_name, InstallFailureKind::CorruptPackage) {
                    continue;
                }
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

        if detail.contains("dpkg was interrupted") && detail.contains("dpkg --configure -a") {
            info!("Detected dpkg interruption after batch install, running recovery");
            if let Err(e) = Self::run_dpkg_recovery_static().await {
                error!("Dpkg recovery failed: {}", e);
            }
        }
    }

    async fn clean_up_old_packages(
        packages_dir: &Path,
        current_release_id: i32,
        previous_release_id: i32,
    ) -> Result<()> {
        Self::clean_up_old_packages_with_limit(
            packages_dir,
            current_release_id,
            previous_release_id,
            MAX_BLOB_CACHE_BYTES,
        )
        .await
    }

    async fn clean_up_old_packages_with_limit(
        packages_dir: &Path,
        current_release_id: i32,
        previous_release_id: i32,
        max_blob_cache_bytes: u64,
    ) -> Result<()> {
        let versions_dir = packages_dir.join("versions");
        let blobs_dir = packages_dir.join("blobs");

        let mut releases = HashMap::new();
        let mut entries = tokio::fs::read_dir(&versions_dir)
            .await
            .with_context(|| format!("reading {}", versions_dir.display()))?;
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_file() {
                continue;
            }
            let Some(release_id) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<i32>().ok())
            else {
                continue;
            };
            let manifest_path = entry.path();
            let manifest = tokio::fs::read_to_string(&manifest_path)
                .await
                .with_context(|| format!("reading {}", manifest_path.display()))?;
            let modified = entry
                .metadata()
                .await?
                .modified()
                .with_context(|| format!("reading mtime for {}", manifest_path.display()))?;
            releases.insert(
                release_id,
                CachedRelease {
                    manifest_path,
                    blobs: manifest_blob_paths(&manifest, &blobs_dir)?,
                    modified,
                },
            );
        }

        let current_release = releases.get_mut(&current_release_id).with_context(|| {
            format!("current release {current_release_id} has no package manifest")
        })?;
        let current_manifest = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&current_release.manifest_path)
            .await
            .with_context(|| {
                format!(
                    "opening current manifest {}",
                    current_release.manifest_path.display()
                )
            })?
            .into_std()
            .await;
        current_manifest
            .set_modified(SystemTime::now())
            .with_context(|| {
                format!(
                    "updating mtime for {}",
                    current_release.manifest_path.display()
                )
            })?;
        current_release.modified = tokio::fs::metadata(&current_release.manifest_path)
            .await?
            .modified()
            .with_context(|| {
                format!(
                    "reading mtime for {}",
                    current_release.manifest_path.display()
                )
            })?;

        let mut older_releases = releases
            .iter()
            .filter(|(release_id, _)| {
                **release_id != current_release_id && **release_id != previous_release_id
            })
            .map(|(release_id, release)| (*release_id, release.modified))
            .collect::<Vec<_>>();
        older_releases.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));

        let protected_releases = [current_release_id, previous_release_id]
            .into_iter()
            .chain(
                older_releases
                    .iter()
                    .map(|(release_id, _)| *release_id)
                    .take(RETAINED_RELEASES.saturating_sub(2)),
            )
            .collect::<HashSet<_>>();
        for release_id in &protected_releases {
            if !releases.contains_key(release_id) {
                anyhow::bail!(
                    "protected release {release_id} has no manifest; retaining the package cache"
                );
            }
        }

        let blob_files = files_in(&blobs_dir).await?;
        let mut cache_bytes = blob_files.iter().map(|(_, size)| size).sum::<u64>();
        info!(
            cache_bytes,
            max_blob_cache_bytes,
            ?protected_releases,
            "Evaluating package blob cache"
        );

        let mut bytes_freed = 0_u64;
        let mut manifests_removed = 0_u64;
        let mut blobs_removed = 0_u64;

        if cache_bytes > max_blob_cache_bytes {
            let referenced_blobs = releases
                .values()
                .flat_map(|release| release.blobs.iter().cloned())
                .collect::<HashSet<_>>();
            for (path, size) in &blob_files {
                if referenced_blobs.contains(path) {
                    continue;
                }
                tokio::fs::remove_file(path)
                    .await
                    .with_context(|| format!("removing orphaned blob {}", path.display()))?;
                cache_bytes = cache_bytes.saturating_sub(*size);
                bytes_freed += *size;
                blobs_removed += 1;
                info!(blob = %path.display(), size, "Removed unreferenced package blob");
            }

            let mut candidates = releases
                .iter()
                .filter(|(release_id, _)| !protected_releases.contains(release_id))
                .map(|(release_id, release)| (*release_id, release.modified))
                .collect::<Vec<_>>();
            candidates.sort_by_key(|(_, modified)| *modified);

            for (release_id, _) in candidates {
                if cache_bytes <= max_blob_cache_bytes {
                    break;
                }
                let Some(release) = releases.remove(&release_id) else {
                    continue;
                };
                tokio::fs::remove_file(&release.manifest_path)
                    .await
                    .with_context(|| format!("removing {}", release.manifest_path.display()))?;
                manifests_removed += 1;
                info!(
                    release_id,
                    manifest = %release.manifest_path.display(),
                    "Evicted old release from the package cache"
                );

                let remaining_blobs = releases
                    .values()
                    .flat_map(|release| release.blobs.iter())
                    .collect::<HashSet<_>>();
                for blob in release.blobs {
                    if remaining_blobs.contains(&blob) || !blob.exists() {
                        continue;
                    }
                    let size = remove_file(&blob).await?;
                    cache_bytes = cache_bytes.saturating_sub(size);
                    bytes_freed += size;
                    blobs_removed += 1;
                    info!(blob = %blob.display(), size, "Removed unreferenced package blob");
                }
            }
        }

        let mut legacy_packages_removed = 0_u64;
        let mut entries = tokio::fs::read_dir(packages_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file()
                && path.extension().and_then(|extension| extension.to_str()) == Some("deb")
            {
                bytes_freed += remove_file(&path).await?;
                legacy_packages_removed += 1;
            }
        }

        info!(
            cache_bytes,
            max_blob_cache_bytes,
            bytes_freed,
            manifests_removed,
            blobs_removed,
            legacy_packages_removed,
            "Package cache cleanup finished"
        );
        if cache_bytes > max_blob_cache_bytes {
            warn!(
                cache_bytes,
                max_blob_cache_bytes,
                ?protected_releases,
                "Package blob cache remains above its soft limit because rollback releases are protected"
            );
        }
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

            // dpkg reports the target version at unpack already; require "ii".
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

#[cfg(test)]
mod tests {
    use super::*;

    async fn write_release(versions_dir: &Path, release_id: i32, manifest: &str) -> Result<()> {
        tokio::fs::write(versions_dir.join(release_id.to_string()), manifest).await?;
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(())
    }

    #[tokio::test]
    async fn cleanup_keeps_current_and_two_most_recent_releases() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let packages_dir = temp.path().join("packages");
        let versions_dir = packages_dir.join("versions");
        let blobs_dir = packages_dir.join("blobs");
        tokio::fs::create_dir_all(&versions_dir).await?;
        tokio::fs::create_dir_all(&blobs_dir).await?;

        write_release(
            &versions_dir,
            10,
            "app 10 app-10.deb\nshared 1 shared.deb\n",
        )
        .await?;
        write_release(
            &versions_dir,
            11,
            "app 11 app-11.deb\nshared 1 shared.deb\n",
        )
        .await?;
        write_release(&versions_dir, 12, "app 12 app-12.deb\n").await?;
        write_release(&versions_dir, 13, "app 13 app-13.deb\n").await?;

        tokio::fs::write(blobs_dir.join("app-10.deb"), b"old").await?;
        tokio::fs::write(blobs_dir.join("app-11.deb"), b"rollback two").await?;
        tokio::fs::write(blobs_dir.join("app-12.deb"), b"rollback one").await?;
        tokio::fs::write(blobs_dir.join("app-13.deb"), b"current").await?;
        tokio::fs::write(blobs_dir.join("shared.deb"), b"shared").await?;
        tokio::fs::write(blobs_dir.join("orphan.deb.part"), b"partial").await?;

        Actor::clean_up_old_packages_with_limit(&packages_dir, 13, 12, 0).await?;

        assert!(!versions_dir.join("10").exists());
        assert!(versions_dir.join("11").exists());
        assert!(versions_dir.join("12").exists());
        assert!(versions_dir.join("13").exists());
        assert!(!blobs_dir.join("app-10.deb").exists());
        assert!(blobs_dir.join("app-11.deb").exists());
        assert!(blobs_dir.join("app-12.deb").exists());
        assert!(blobs_dir.join("app-13.deb").exists());
        assert!(blobs_dir.join("shared.deb").exists());
        assert!(!blobs_dir.join("orphan.deb.part").exists());

        Ok(())
    }

    #[tokio::test]
    async fn cleanup_leaves_extra_releases_while_cache_is_below_limit() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let packages_dir = temp.path().join("packages");
        let versions_dir = packages_dir.join("versions");
        let blobs_dir = packages_dir.join("blobs");
        tokio::fs::create_dir_all(&versions_dir).await?;
        tokio::fs::create_dir_all(&blobs_dir).await?;

        write_release(&versions_dir, 1, "app 1 app-1.deb\n").await?;
        write_release(&versions_dir, 2, "app 2 app-2.deb\n").await?;
        write_release(&versions_dir, 3, "app 3 app-3.deb\n").await?;
        write_release(&versions_dir, 4, "app 4 app-4.deb\n").await?;
        tokio::fs::write(blobs_dir.join("app-1.deb"), b"one").await?;
        tokio::fs::write(blobs_dir.join("app-2.deb"), b"two").await?;
        tokio::fs::write(blobs_dir.join("app-3.deb"), b"three").await?;
        tokio::fs::write(blobs_dir.join("app-4.deb"), b"four").await?;

        Actor::clean_up_old_packages_with_limit(&packages_dir, 4, 3, u64::MAX).await?;

        assert!(versions_dir.join("1").exists());
        assert!(blobs_dir.join("app-1.deb").exists());

        Ok(())
    }

    #[tokio::test]
    async fn cleanup_uses_recency_instead_of_release_id_for_rollbacks() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let packages_dir = temp.path().join("packages");
        let versions_dir = packages_dir.join("versions");
        let blobs_dir = packages_dir.join("blobs");
        tokio::fs::create_dir_all(&versions_dir).await?;
        tokio::fs::create_dir_all(&blobs_dir).await?;

        write_release(&versions_dir, 20, "app 20 app-20.deb\n").await?;
        write_release(&versions_dir, 8, "app 8 app-8.deb\n").await?;
        write_release(&versions_dir, 15, "app 15 app-15.deb\n").await?;
        write_release(&versions_dir, 7, "app 7 app-7.deb\n").await?;
        for release_id in [7, 8, 15, 20] {
            tokio::fs::write(
                blobs_dir.join(format!("app-{release_id}.deb")),
                release_id.to_string(),
            )
            .await?;
        }

        Actor::clean_up_old_packages_with_limit(&packages_dir, 7, 15, 0).await?;

        assert!(versions_dir.join("7").exists());
        assert!(versions_dir.join("15").exists());
        assert!(versions_dir.join("8").exists());
        assert!(!versions_dir.join("20").exists());

        Ok(())
    }

    #[test]
    fn manifest_rejects_paths_outside_blob_cache() {
        let result = manifest_blob_paths("app 1 ../../etc/passwd\n", Path::new("/packages/blobs"));

        assert!(result.is_err());
    }
}
