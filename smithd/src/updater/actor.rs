use super::PENDING_SMITH_RELEASE_FILE;
use crate::downloader::DownloaderHandle;
use crate::magic::MagicHandle;
use crate::magic::structure::ConfigPackage;
use crate::session::SessionHandle;
use crate::shutdown::ShutdownSignals;
use crate::utils::network::NetworkClient;
use anyhow::Context;
use anyhow::Result;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time::{self, Duration};
use tracing::{error, info, warn};

const MAX_INSTALL_RETRIES: u32 = 3;
const PACKAGE_CACHE_RESERVE_BYTES: u64 = 3 * 1024 * 1024 * 1024;
const RETAINED_RELEASES: usize = 4;
const RELEASE_HISTORY_FILE: &str = "release-history";

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

fn manifest_blob_paths(manifest: &str, blobs_dir: &Path) -> Result<HashSet<PathBuf>> {
    ConfigPackage::parse_manifest(manifest)?
        .into_iter()
        .map(|package| package.safe_file_path(blobs_dir))
        .collect()
}

fn parse_release_history(history: &str) -> Result<Vec<i32>> {
    history
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.parse::<i32>()
                .with_context(|| format!("invalid release id in history: {line:?}"))
        })
        .collect()
}

async fn remove_file_and_count(path: &Path, bytes_freed: &mut u64) -> bool {
    let size = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata.len(),
        Err(err) => {
            warn!("Failed to stat old package {}: {}", path.display(), err);
            0
        }
    };

    if let Err(err) = tokio::fs::remove_file(path).await {
        error!("Failed to remove old package {}: {}", path.display(), err);
        false
    } else {
        *bytes_freed += size;
        true
    }
}

#[derive(Debug)]
pub enum ActorMessage {
    Apply,
    Prepare { rpc: oneshot::Sender<bool> },
    InstallPrepared { rpc: oneshot::Sender<bool> },
    Check,
    StatusReport { rpc: oneshot::Sender<String> },
}

#[derive(Debug, PartialEq)]
enum CheckAction {
    InstallPrepared(i32),
    Apply(i32),
}

fn check_action(
    current_release_id: Option<i32>,
    target_release_id: Option<i32>,
    prepared_release_id: Option<i32>,
) -> Option<CheckAction> {
    if let Some(prepared_release_id) = prepared_release_id
        && current_release_id != Some(prepared_release_id)
    {
        return Some(CheckAction::InstallPrepared(prepared_release_id));
    }

    target_release_id
        .filter(|target_release_id| current_release_id != Some(*target_release_id))
        .map(CheckAction::Apply)
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
    prepared_release_id: Option<i32>,
    downloader: DownloaderHandle,
    install_failures: HashMap<String, PackageFailure>,
    install_failure_release_id: Option<i32>,
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
            prepared_release_id: None,
            downloader,
            install_failures: HashMap::new(),
            install_failure_release_id: None,
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

    fn begin_release_attempt(&mut self, target_release_id: i32) {
        if self.install_failure_release_id != Some(target_release_id) {
            self.install_failures.clear();
            self.install_failure_release_id = Some(target_release_id);
        }
    }

    async fn handle_message(&mut self, msg: ActorMessage) {
        match msg {
            ActorMessage::Apply => {
                if let Some(prepared_release_id) = self.prepared_release_id {
                    info!(
                        prepared_release_id,
                        "Applying the already prepared release instead of replacing its pinned target"
                    );
                    self.upgrade(prepared_release_id).await;
                    self.prepared_release_id = None;
                } else {
                    match self.magic.get_target_release_id().await {
                        Ok(target_release_id) => self.apply_release(target_release_id).await,
                        Err(err) => error!(
                            error = ?err,
                            "Cannot apply release because no target release is available"
                        ),
                    }
                }
            }
            ActorMessage::Prepare { rpc } => {
                if let Some(prepared_release_id) = self.prepared_release_id {
                    info!(
                        prepared_release_id,
                        "A release is already prepared; keeping its pinned target"
                    );
                    if rpc.send(true).is_err() {
                        warn!("Release preparation caller stopped waiting for the result");
                    }
                } else {
                    let current_release_id = self.magic.get_release_id().await.ok();
                    match self.magic.get_target_release_id().await {
                        Ok(target_release_id) if current_release_id == Some(target_release_id) => {
                            info!(
                                target_release_id,
                                "Release preparation skipped because the target is already active"
                            );
                            if rpc.send(false).is_err() {
                                warn!("Release preparation caller stopped waiting for the result");
                            }
                        }
                        Ok(target_release_id) => {
                            self.update(target_release_id).await;
                            let prepared = self.prepared_release_id == Some(target_release_id);
                            if rpc.send(prepared).is_err() {
                                warn!("Release preparation caller stopped waiting for the result");
                            }
                        }
                        Err(err) => {
                            error!(
                                error = ?err,
                                "Cannot prepare release because no target release is available"
                            );
                            if rpc.send(false).is_err() {
                                warn!("Release preparation caller stopped waiting for the result");
                            }
                        }
                    }
                }
            }
            ActorMessage::InstallPrepared { rpc } => match self.prepared_release_id {
                Some(target_release_id) => {
                    if rpc.send(true).is_err() {
                        warn!("Release installation caller stopped waiting for acceptance");
                    }
                    self.upgrade(target_release_id).await;
                    self.prepared_release_id = None;
                }
                None => {
                    warn!("Cannot install release because no prepared target is pinned");
                    if rpc.send(false).is_err() {
                        warn!("Release installation caller stopped waiting for rejection");
                    }
                }
            },
            ActorMessage::Check => {
                let release_id = self.magic.get_release_id().await.ok();
                let target_release_id = self.magic.get_target_release_id().await.ok();

                match check_action(release_id, target_release_id, self.prepared_release_id) {
                    Some(CheckAction::InstallPrepared(prepared_release_id)) => {
                        info!(
                            prepared_release_id,
                            latest_target_release_id = ?target_release_id,
                            "Finishing the already prepared release before considering a newer target"
                        );
                        self.upgrade(prepared_release_id).await;
                        self.prepared_release_id = None;
                    }
                    Some(CheckAction::Apply(target_release_id)) => {
                        self.apply_release(target_release_id).await;
                    }
                    None => {}
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

    async fn apply_release(&mut self, target_release_id: i32) {
        info!(
            target_release_id,
            "Starting updater transaction for pinned target release"
        );
        self.update(target_release_id).await;

        if self.prepared_release_id != Some(target_release_id) {
            warn!(
                target_release_id,
                "Release preparation failed; installation will not start"
            );
            return;
        }

        self.upgrade(target_release_id).await;
        self.prepared_release_id = None;
    }

    async fn ensure_no_pending_smith_update(&self, requested_release_id: i32) -> Result<()> {
        let pending_release = self.packages_dir.join(PENDING_SMITH_RELEASE_FILE);
        let pending_release_id = match tokio::fs::read_to_string(&pending_release).await {
            Ok(release_id) => release_id
                .trim()
                .parse::<i32>()
                .with_context(|| format!("Invalid release id in {}", pending_release.display()))?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("Failed to read {}", pending_release.display()));
            }
        };

        warn!(
            pending_release_id,
            requested_release_id,
            "Deferring the requested release because a pinned Smith self-update is still pending"
        );
        let output = Command::new("sudo")
            .arg("systemctl")
            .arg("start")
            .arg("smith-updater")
            .output()
            .await
            .with_context(|| "Failed to resume pending Smith updater service")?;
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to resume pending Smith updater: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Err(anyhow::anyhow!(
            "Smith self-update to release {pending_release_id} is still pending"
        ))
    }

    #[tracing::instrument(skip(self))]
    async fn update(&mut self, target_release_id: i32) {
        info!(target_release_id, "Preparing release update");
        self.begin_release_attempt(target_release_id);
        self.status = Status::Updating;
        self.prepared_release_id = None;
        if let Err(err) = self.ensure_no_pending_smith_update(target_release_id).await {
            warn!(
                error = ?err,
                target_release_id,
                "Release preparation deferred"
            );
            self.last_update = Some(Err(err));
            self.status = Status::Idle;
            return;
        }
        let res = self
            .check_for_updates(target_release_id)
            .await
            .map(|_| time::Instant::now());
        if res.is_ok() {
            self.prepared_release_id = Some(target_release_id);
        }
        info!("Updating result: {:?}", res);
        self.last_update = Some(res);
        self.status = Status::Idle;
    }

    async fn upgrade(&mut self, target_release_id: i32) {
        info!(target_release_id, "Upgrading device to pinned release");
        self.status = Status::Upgrading;
        let res = self
            .upgrade_device(target_release_id)
            .await
            .map(|_| time::Instant::now());
        info!("Upgrading result: {:?}", res);
        self.last_upgrade = Some(res);
        self.status = Status::Idle;
    }

    async fn write_atomic_file(path: &Path, contents: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let part_path = path.with_extension("part");
        tokio::fs::write(&part_path, contents)
            .await
            .with_context(|| format!("writing temporary file {}", part_path.display()))?;
        tokio::fs::rename(&part_path, path)
            .await
            .with_context(|| format!("publishing {}", path.display()))?;
        Ok(())
    }

    async fn fetch_blob(&self, package: &ConfigPackage, blob_path: &Path) -> Result<()> {
        if let Some(parent) = blob_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // TODO: remove when legacy /packages layout is fully migrated.
        let legacy_path = package.safe_file_path(&self.packages_dir)?;
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
        let blobs = self.packages_dir.join("blobs");

        if release_cache.exists() {
            let cached_manifest = tokio::fs::read_to_string(&release_cache)
                .await
                .with_context(|| format!("reading release cache {}", release_cache.display()))?;
            match ConfigPackage::parse_manifest(&cached_manifest) {
                Ok(packages) if !packages.is_empty() => {
                    let mut cache_complete = true;
                    for package in packages {
                        let blob_path = package.safe_file_path(&blobs)?;
                        if !self.blob_is_valid(&blob_path).await? {
                            cache_complete = false;
                        }
                    }
                    if cache_complete {
                        info!(release_id, "release cache is complete");
                        return Ok(());
                    }
                    warn!(
                        release_id,
                        "Release manifest references missing or empty blobs; rebuilding cache"
                    );
                }
                Ok(_) => {
                    warn!(release_id, "Release manifest is empty; rebuilding cache");
                }
                Err(err) => {
                    warn!(
                        error = ?err,
                        release_id,
                        "Release manifest is invalid; rebuilding cache"
                    );
                }
            }
            tokio::fs::remove_file(&release_cache)
                .await
                .with_context(|| format!("removing stale {}", release_cache.display()))?;
        }

        // Prefer the short-lived device JWT; falls back to the opaque token when
        // no valid JWT is cached (see SessionHandle::bearer_token).
        let token = self.session.bearer_token().await.unwrap_or_default();

        let release_packages = self
            .network
            .get_release_packages(release_id, &token)
            .await
            .with_context(|| "failed to fetch release packages manifest")?;
        if release_packages.is_empty() {
            return Err(anyhow::anyhow!("release {release_id} contains no packages"));
        }

        // Serialize first so unsafe server metadata is rejected before any path
        // construction, migration, or download can touch the filesystem.
        let manifest = ConfigPackage::serialize_manifest(&release_packages)?;

        for package in &release_packages {
            info!("Processing package: {}", package.file);
            let blob_path = package.safe_file_path(&blobs)?;

            if self.blob_is_valid(&blob_path).await? {
                info!("blob present in cache");
            } else {
                self.fetch_blob(package, &blob_path)
                    .await
                    .with_context(|| format!("fetching blob for package {}", package.file))?;
            }
        }

        Self::write_atomic_file(&release_cache, &manifest).await?;
        info!(release_id, "release cache ready");

        Ok(())
    }

    async fn clean_cache_before_download(&self, current_release_id: i32) {
        let history_path = self.packages_dir.join(RELEASE_HISTORY_FILE);
        let history = match tokio::fs::read_to_string(&history_path).await {
            Ok(history) => match parse_release_history(&history) {
                Ok(history) => history,
                Err(err) => {
                    warn!(
                        error = ?err,
                        history_path = %history_path.display(),
                        "Skipping pre-download cache cleanup because release history is invalid"
                    );
                    return;
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                info!("Skipping pre-download cache cleanup because no rollback history exists yet");
                return;
            }
            Err(err) => {
                warn!(
                    error = ?err,
                    history_path = %history_path.display(),
                    "Skipping pre-download cache cleanup because release history cannot be read"
                );
                return;
            }
        };

        let rollback_release_id = match history
            .into_iter()
            .find(|release_id| *release_id != current_release_id)
        {
            Some(release_id) => release_id,
            None => {
                info!(
                    current_release_id,
                    "Skipping pre-download cache cleanup because no distinct rollback release is known"
                );
                return;
            }
        };

        if let Err(err) =
            Self::clean_up_old_packages(&self.packages_dir, current_release_id, rollback_release_id)
                .await
        {
            warn!(
                error = ?err,
                current_release_id,
                rollback_release_id,
                "Pre-download package cache cleanup failed; continuing with release preparation"
            );
        }
    }

    #[tracing::instrument(skip(self))]
    async fn check_for_updates(&self, target_release_id: i32) -> Result<()> {
        // apt update on check for updates with timeout
        info!("Running apt update with 5 minute timeout");
        let apt_update_future = Command::new("sh")
            .arg("-c")
            .arg("apt update -y")
            .kill_on_drop(true)
            .output();

        match time::timeout(Duration::from_secs(300), apt_update_future).await {
            Ok(result) => {
                let output = result.with_context(|| "Failed to run apt update")?;
                if !output.status.success() {
                    return Err(anyhow::anyhow!(
                        "apt update failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }
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
                self.clean_cache_before_download(current_release_id).await;
            }
            Err(err) => {
                warn!(
                    error = ?err,
                    "Skipping current release cache warm-up because current release id is unavailable"
                );
            }
        }

        self.ensure_release_cache(target_release_id)
            .await
            .with_context(|| "Failed to ensure target release cache")?;

        Ok(())
    }

    async fn upgrade_device(&mut self, target_release_id: i32) -> Result<()> {
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

        let previous_release_id = match self.magic.get_release_id().await {
            Ok(release_id) => Some(release_id),
            Err(err) => {
                warn!(
                    error = ?err,
                    "Current release id is unavailable; package cleanup will be skipped to preserve rollback packages"
                );
                None
            }
        };

        let blobs = self.packages_dir.join("blobs");
        let release_cache = self
            .packages_dir
            .join("versions")
            .join(target_release_id.to_string());

        // read the file from release cache
        let content = tokio::fs::read(&release_cache).await?;
        let content = std::str::from_utf8(&content)?;

        let packages = ConfigPackage::parse_manifest(content)?;

        // check if all packages are available locally
        for package in &packages {
            info!("Checking package: {}", package.name);
            let package_name = &package.name;

            // check if package is available locally
            let package_file = package.safe_file_path(&blobs)?;

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
                let blob_path = package.safe_file_path(&blobs)?;
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
            let pending_release = self.packages_dir.join(PENDING_SMITH_RELEASE_FILE);
            Self::write_atomic_file(&pending_release, &format!("{target_release_id}\n"))
                .await
                .with_context(|| "Failed to pin target release for smith-updater")?;
            info!(
                target_release_id,
                pending_release = %pending_release.display(),
                "Pinned Smith self-update target before starting smith-updater"
            );

            let status = Command::new("sudo")
                .arg("systemctl")
                .arg("start")
                .arg("smith-updater")
                .output()
                .await
                .with_context(|| "Failed to start smith updater service")?;

            if !status.status.success() {
                return Err(anyhow::anyhow!(
                    "Failed to start smith updater: {}",
                    String::from_utf8_lossy(&status.stderr)
                ));
            }
        }

        self.are_packages_up_to_date(target_release_id).await?;

        self.magic.set_release_id(target_release_id).await;

        match self.magic.get_target_release_id().await {
            Ok(latest_target_release_id) if latest_target_release_id != target_release_id => {
                warn!(
                    installed_release_id = target_release_id,
                    latest_target_release_id,
                    "Target release changed during the upgrade; committed the pinned installed release and will process the newer target on the next check"
                );
            }
            Ok(_) => {
                info!(
                    target_release_id,
                    "Pinned target remained stable throughout the upgrade"
                );
            }
            Err(err) => {
                warn!(
                    error = ?err,
                    installed_release_id = target_release_id,
                    "Could not read the latest target after committing the pinned installed release"
                );
            }
        }

        if let Some(previous_release_id) = previous_release_id
            && previous_release_id != target_release_id
        {
            if let Err(err) = Self::clean_up_old_packages(
                &self.packages_dir,
                target_release_id,
                previous_release_id,
            )
            .await
            {
                warn!(
                    error = ?err,
                    target_release_id,
                    previous_release_id,
                    "Package cache cleanup failed after successful release activation"
                );
            }
        } else {
            info!(
                "Skipping package cleanup because there is no distinct previous release to retain for rollback"
            );
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
        rollback_release_id: i32,
    ) -> Result<()> {
        Self::clean_up_old_packages_with_reserve(
            packages_dir,
            current_release_id,
            rollback_release_id,
            PACKAGE_CACHE_RESERVE_BYTES,
        )
        .await
    }

    async fn clean_up_old_packages_with_reserve(
        packages_dir: &Path,
        current_release_id: i32,
        rollback_release_id: i32,
        reserve_bytes: u64,
    ) -> Result<()> {
        info!(
            current_release_id,
            rollback_release_id,
            reserve_bytes,
            packages_dir = %packages_dir.display(),
            "Evaluating package cache cleanup after successful release activation"
        );

        let versions_dir = packages_dir.join("versions");
        let blobs_dir = packages_dir.join("blobs");
        let history_path = packages_dir.join(RELEASE_HISTORY_FILE);
        let history = match tokio::fs::read_to_string(&history_path).await {
            Ok(history) => {
                let parsed_history = parse_release_history(&history);
                let history = match parsed_history {
                    Ok(history) => history,
                    Err(err) => {
                        warn!(
                            error = ?err,
                            history_path = %history_path.display(),
                            "Discarding invalid release history and rebuilding it from the current activation"
                        );
                        Vec::new()
                    }
                };
                info!(?history, "Loaded package release activation history");
                history
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                info!(
                    history_path = %history_path.display(),
                    "No package release history exists yet; creating it from the current activation"
                );
                Vec::new()
            }
            Err(err) => {
                return Err(err).with_context(|| format!("reading {}", history_path.display()));
            }
        };

        let mut seen_releases = HashSet::new();
        let release_history = [current_release_id, rollback_release_id]
            .into_iter()
            .chain(history)
            .filter(|release_id| seen_releases.insert(*release_id))
            .take(RETAINED_RELEASES)
            .collect::<Vec<_>>();
        let history_contents = release_history
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let history_part = packages_dir.join(format!("{RELEASE_HISTORY_FILE}.part"));
        tokio::fs::write(&history_part, history_contents).await?;
        tokio::fs::rename(&history_part, &history_path).await?;
        info!(
            ?release_history,
            history_path = %history_path.display(),
            "Recorded release activation history in newest-first order"
        );

        let available_bytes = fs2::available_space(packages_dir)?;
        if available_bytes >= reserve_bytes {
            info!(
                available_bytes,
                reserve_bytes,
                "Package cache cleanup deferred: available disk space meets the configured reserve; all cached releases will be kept"
            );
            return Ok(());
        }

        warn!(
            available_bytes,
            reserve_bytes,
            deficit_bytes = reserve_bytes.saturating_sub(available_bytes),
            "Available disk space is below the package cache reserve; starting cleanup"
        );

        let retained_releases = release_history
            .iter()
            .take(RETAINED_RELEASES)
            .copied()
            .collect::<Vec<_>>();
        let mut retained_blobs = HashSet::new();

        // Read every protected manifest before deleting anything. If one is unavailable
        // or malformed, retaining the whole cache is safer than losing fast rollback.
        for release_id in &retained_releases {
            let manifest_path = versions_dir.join(release_id.to_string());
            let manifest = match tokio::fs::read_to_string(&manifest_path).await {
                Ok(manifest) => manifest,
                Err(err) => {
                    warn!(
                        error = ?err,
                        release_id,
                        manifest = %manifest_path.display(),
                        "Skipping package cache cleanup because a protected release manifest cannot be read"
                    );
                    return Ok(());
                }
            };
            let manifest_blobs = match manifest_blob_paths(&manifest, &blobs_dir) {
                Ok(manifest_blobs) => manifest_blobs,
                Err(err) => {
                    warn!(
                        error = ?err,
                        release_id,
                        manifest = %manifest_path.display(),
                        "Skipping package cache cleanup because a protected release manifest is invalid"
                    );
                    return Ok(());
                }
            };
            retained_blobs.extend(manifest_blobs);
        }
        info!(
            ?retained_releases,
            retained_blob_count = retained_blobs.len(),
            "Protected the current release and newest rollback releases; their manifests and blobs will not be removed"
        );

        let mut bytes_freed: u64 = 0;
        let mut manifests_removed = 0_u64;
        let mut versions = tokio::fs::read_dir(&versions_dir).await?;
        while let Some(entry) = versions.next_entry().await? {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let retained = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<i32>().ok())
                .is_some_and(|release_id| retained_releases.contains(&release_id));
            if retained {
                continue;
            }

            info!(
                manifest = %path.display(),
                "Removing release manifest because it is outside the protected rollback history"
            );
            if remove_file_and_count(&path, &mut bytes_freed).await {
                manifests_removed += 1;
            }
        }

        let mut blobs_removed = 0_u64;
        if blobs_dir.exists() {
            let mut directories = vec![blobs_dir.clone()];
            let mut visited_directories = Vec::new();
            while let Some(directory) = directories.pop() {
                visited_directories.push(directory.clone());
                let mut entries = tokio::fs::read_dir(&directory).await?;
                while let Some(entry) = entries.next_entry().await? {
                    let path = entry.path();
                    let file_type = entry.file_type().await?;
                    if file_type.is_dir() {
                        directories.push(path);
                    } else if file_type.is_file()
                        && !retained_blobs.contains(&path)
                        && remove_file_and_count(&path, &mut bytes_freed).await
                    {
                        blobs_removed += 1;
                    }
                }
            }

            // Remove now-empty nested directories, but keep the blobs root.
            for directory in visited_directories.into_iter().rev() {
                if directory != blobs_dir
                    && let Err(err) = tokio::fs::remove_dir(&directory).await
                    && err.kind() != std::io::ErrorKind::DirectoryNotEmpty
                {
                    warn!(
                        "Failed to remove package cache directory {}: {}",
                        directory.display(),
                        err
                    );
                }
            }
        }

        // Remove packages left behind by the legacy, pre-blob cache layout.
        let mut legacy_packages_removed = 0_u64;
        let mut entries = tokio::fs::read_dir(packages_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file()
                && path.extension().and_then(|e| e.to_str()) == Some("deb")
                && remove_file_and_count(&path, &mut bytes_freed).await
            {
                legacy_packages_removed += 1;
            }
        }

        let available_bytes_after_cleanup = fs2::available_space(packages_dir)?;
        info!(
            current_release_id,
            rollback_release_id,
            ?retained_releases,
            retained_release_count = retained_releases.len(),
            manifests_removed,
            blobs_removed,
            legacy_packages_removed,
            bytes_freed,
            available_bytes_after_cleanup,
            reserve_bytes,
            "Package cache cleanup finished"
        );
        if available_bytes_after_cleanup < reserve_bytes {
            warn!(
                available_bytes_after_cleanup,
                reserve_bytes,
                "Package cache cleanup could not restore the disk reserve without deleting protected rollback releases"
            );
        } else {
            info!(
                available_bytes_after_cleanup,
                reserve_bytes, "Package cache cleanup restored the configured disk reserve"
            );
        }
        Ok(())
    }

    /// Checks whether packages are up to date.
    ///
    /// Returns `Ok` if all packages are, `Err` otherwise.
    async fn are_packages_up_to_date(&self, target_release_id: i32) -> Result<()> {
        let release_cache = self
            .packages_dir
            .join("versions")
            .join(target_release_id.to_string());

        // read the file from release cache
        let content = tokio::fs::read(&release_cache).await?;
        let content = std::str::from_utf8(&content)?;

        let packages = ConfigPackage::parse_manifest(content)?;

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

    #[test]
    fn prepared_release_wins_when_postman_changes_target() {
        assert_eq!(
            check_action(Some(10), Some(12), Some(11)),
            Some(CheckAction::InstallPrepared(11))
        );
    }

    #[test]
    fn latest_target_is_applied_after_prepared_release_is_current() {
        assert_eq!(
            check_action(Some(11), Some(12), Some(11)),
            Some(CheckAction::Apply(12))
        );
    }

    #[test]
    fn no_action_when_current_release_matches_target() {
        assert_eq!(check_action(Some(12), Some(12), None), None);
    }

    #[tokio::test]
    async fn release_manifest_is_published_before_preparation_succeeds() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let manifest_path = temp.path().join("versions").join("12");

        Actor::write_atomic_file(&manifest_path, "app 12 app-12.deb\n").await?;

        assert_eq!(
            tokio::fs::read_to_string(&manifest_path).await?,
            "app 12 app-12.deb\n"
        );
        assert!(!manifest_path.with_extension("part").exists());

        Ok(())
    }

    #[tokio::test]
    async fn cleanup_retains_current_and_actual_previous_release() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let packages = temp.path().join("packages");
        let versions = packages.join("versions");
        let blobs = packages.join("blobs");
        tokio::fs::create_dir_all(&versions).await?;
        tokio::fs::create_dir_all(blobs.join("nested")).await?;

        tokio::fs::write(
            versions.join("12"),
            "app 12 app-12.deb\nshared 2 shared.deb\n",
        )
        .await?;
        tokio::fs::write(
            versions.join("8"),
            "app 8 nested/app-8.deb\nshared 2 shared.deb\n",
        )
        .await?;
        tokio::fs::write(versions.join("10"), "app 10 app-10.deb\n").await?;
        tokio::fs::write(versions.join("9"), "app 9 app-9.deb\n").await?;
        tokio::fs::write(versions.join("11"), "app 11 app-11.deb\n").await?;
        tokio::fs::write(packages.join(RELEASE_HISTORY_FILE), "10\n9\n7\n").await?;

        tokio::fs::write(blobs.join("app-12.deb"), b"current").await?;
        tokio::fs::write(blobs.join("nested/app-8.deb"), b"rollback").await?;
        tokio::fs::write(blobs.join("app-10.deb"), b"older rollback").await?;
        tokio::fs::write(blobs.join("app-9.deb"), b"oldest rollback").await?;
        tokio::fs::write(blobs.join("shared.deb"), b"shared").await?;
        tokio::fs::write(blobs.join("app-11.deb"), b"stale").await?;
        tokio::fs::write(blobs.join("abandoned.deb.part"), b"partial").await?;
        tokio::fs::write(packages.join("legacy.deb"), b"legacy").await?;

        Actor::clean_up_old_packages_with_reserve(&packages, 12, 8, u64::MAX).await?;

        assert!(versions.join("12").exists());
        assert!(versions.join("8").exists());
        assert!(versions.join("10").exists());
        assert!(versions.join("9").exists());
        assert!(!versions.join("11").exists());
        assert!(blobs.join("app-12.deb").exists());
        assert!(blobs.join("nested/app-8.deb").exists());
        assert!(blobs.join("app-10.deb").exists());
        assert!(blobs.join("app-9.deb").exists());
        assert!(blobs.join("shared.deb").exists());
        assert!(!blobs.join("app-11.deb").exists());
        assert!(!blobs.join("abandoned.deb.part").exists());
        assert!(!packages.join("legacy.deb").exists());
        assert_eq!(
            tokio::fs::read_to_string(packages.join(RELEASE_HISTORY_FILE)).await?,
            "12\n8\n10\n9\n"
        );

        Ok(())
    }

    #[tokio::test]
    async fn cleanup_does_not_delete_anything_when_rollback_manifest_is_missing() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let packages = temp.path().join("packages");
        let versions = packages.join("versions");
        let blobs = packages.join("blobs");
        tokio::fs::create_dir_all(&versions).await?;
        tokio::fs::create_dir_all(&blobs).await?;

        tokio::fs::write(versions.join("12"), "app 12 app-12.deb\n").await?;
        tokio::fs::write(versions.join("11"), "app 11 app-11.deb\n").await?;
        tokio::fs::write(blobs.join("app-12.deb"), b"current").await?;
        tokio::fs::write(blobs.join("app-11.deb"), b"stale").await?;

        let result = Actor::clean_up_old_packages_with_reserve(&packages, 12, 8, u64::MAX).await;

        assert!(result.is_ok());
        assert!(versions.join("11").exists());
        assert!(blobs.join("app-11.deb").exists());

        Ok(())
    }

    #[tokio::test]
    async fn cleanup_is_deferred_while_disk_reserve_is_healthy() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let packages = temp.path().join("packages");
        let versions = packages.join("versions");
        let blobs = packages.join("blobs");
        tokio::fs::create_dir_all(&versions).await?;
        tokio::fs::create_dir_all(&blobs).await?;

        tokio::fs::write(versions.join("12"), "app 12 app-12.deb\n").await?;
        tokio::fs::write(versions.join("8"), "app 8 app-8.deb\n").await?;
        tokio::fs::write(versions.join("7"), "app 7 app-7.deb\n").await?;
        tokio::fs::write(blobs.join("app-7.deb"), b"still cached").await?;
        tokio::fs::write(packages.join(RELEASE_HISTORY_FILE), "7\n6\n5\n").await?;

        Actor::clean_up_old_packages_with_reserve(&packages, 12, 8, 0).await?;

        assert!(versions.join("7").exists());
        assert!(blobs.join("app-7.deb").exists());
        assert_eq!(
            tokio::fs::read_to_string(packages.join(RELEASE_HISTORY_FILE)).await?,
            "12\n8\n7\n6\n"
        );

        Ok(())
    }

    #[tokio::test]
    async fn cleanup_repairs_invalid_release_history() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let packages = temp.path().join("packages");
        tokio::fs::create_dir_all(packages.join("versions")).await?;
        tokio::fs::write(packages.join(RELEASE_HISTORY_FILE), "11\ninvalid\n").await?;

        Actor::clean_up_old_packages_with_reserve(&packages, 12, 8, 0).await?;

        assert_eq!(
            tokio::fs::read_to_string(packages.join(RELEASE_HISTORY_FILE)).await?,
            "12\n8\n"
        );

        Ok(())
    }

    #[test]
    fn manifest_rejects_invalid_blob_entries() {
        let blobs = Path::new("/packages/blobs");

        assert!(manifest_blob_paths("app 1 ../../etc/passwd\n", blobs).is_err());
        assert!(manifest_blob_paths("app 1 /etc/passwd\n", blobs).is_err());
        assert!(manifest_blob_paths("app 1\n", blobs).is_err());
    }

    #[test]
    fn server_package_metadata_is_validated_before_serialization() {
        let unsafe_package = ConfigPackage {
            name: "app".to_string(),
            version: "1".to_string(),
            file: "../../etc/passwd".to_string(),
        };
        let injected_package = ConfigPackage {
            name: "app".to_string(),
            version: "1".to_string(),
            file: "app.deb\nother 1 other.deb".to_string(),
        };

        assert!(ConfigPackage::serialize_manifest(&[unsafe_package]).is_err());
        assert!(ConfigPackage::serialize_manifest(&[injected_package]).is_err());
    }
}
