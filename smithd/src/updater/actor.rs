use crate::control::update;
use crate::magic::MagicHandle;
use crate::shutdown::{ShutdownHandler, ShutdownSignals};
use crate::utils::network::NetworkClient;
use anyhow::Context;
use anyhow::Result;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time;
use tracing::{error, info, warn};

#[derive(Debug)]
pub enum ActorMessage {
    Update,
    Upgrade,
    Checking,
    StatusReport { rpc: oneshot::Sender<String> },
}

enum Status {
    Idle,
    Updating,
    Upgrading,
}

/// Updater Actor
pub struct Actor {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<ActorMessage>,
    magic: MagicHandle,
    status: Status,
    network: NetworkClient,
    last_update: Option<Result<time::Instant>>,
    last_upgrade: Option<Result<time::Instant>>,
}

impl Actor {
    pub fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<ActorMessage>,
        magic: MagicHandle,
    ) -> Self {
        let network = NetworkClient::new();
        Self {
            shutdown,
            receiver,
            magic,
            network,
            status: Status::Idle,
            last_update: None,
            last_upgrade: None,
        }
    }

    async fn handle_message(&mut self, msg: ActorMessage) {
        match msg {
            ActorMessage::Update => {
                self.update();
            }
            ActorMessage::Upgrade => {
                self.upgrade();
            }
            ActorMessage::Checking => {
                let release_id = self.magic.get_release_id().await;
                let target_release_id = self.magic.get_target_release_id().await;

                if release_id != target_release_id {
                    info!(
                        "Upgrading from release_id {release_id:?} to target_release_id {target_release_id:?}"
                    );

                    self.update();

                    if matches!(self.last_update, Some(Err(_)) | None) {
                        return;
                    }

                    self.upgrade();

                    if matches!(self.last_upgrade, Some(Err(_)) | None) {
                        return;
                    }

                    self.magic.set_release_id(target_release_id).await;
                }
            }
            ActorMessage::StatusReport { rpc } => {
                // take the instants and format them nicely with X seconds ago
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

                let _rpc = rpc.send(format!(
                    "Last Update: {} | Last Upgrade: {}",
                    last_update_string, last_upgrade_string
                ));
            }
        }
    }

    async fn update(&mut self) {
        info!("Checking for updates");
        self.status = Status::Updating;
        let res = self.check_for_updates().await.map(|_| time::Instant::now());
        info!("Check for updates result: {:?}", res);
        self.last_update = Some(res);
        self.status = Status::Idle;
    }

    async fn upgrade(&mut self) {
        info!("Upgrading device");
        self.status = Status::Upgrading;
        let res = self.upgrade_device().await.map(|_| time::Instant::now());
        info!("Upgrading result: {:?}, changing to app mode", res);
        self.last_upgrade = Some(res);
        self.status = Status::Idle;
    }

    async fn check_for_updates(&self) -> Result<()> {
        // apt update on check for updates
        Command::new("sh")
            .arg("-c")
            .arg("apt update -y")
            .output()
            .await
            .with_context(|| "Failed to run apt update")?;

        let target_release_id = self
            .magic
            .get_target_release_id()
            .await
            .with_context(|| "Failed to get Target Release ID")?;

        let token = self.magic.get_token().await.unwrap_or_default();

        info!("Checking for updates");
        info!("Target release id: {:?}", target_release_id);

        // get current configured packages
        let local_packages = self.magic.get_packages().await;

        // ask postman for the packages of the target release
        let target_packages = self
            .network
            .get_release_packages(target_release_id, &token)
            .await?;

        info!("== Current packages ==");
        for package in local_packages.iter() {
            info!(
                "Local: {} {} {}",
                package.name, package.version, package.file
            );
        }
        info!("++ Release packages ++");
        for package in target_packages.iter() {
            info!(
                "Remote: {} {} {}",
                package.name, package.version, package.file
            );
        }

        let mut up_to_date = true;
        // compare the packages and check if we need to update
        for target_package in target_packages.iter() {
            let package_not_on_magic_file = !local_packages.contains(target_package);
            let package_not_installed = tokio::process::Command::new("dpkg")
                .arg("-l")
                .arg(&target_package.name)
                .output()
                .await
                .map(|output| !output.status.success())
                .unwrap_or(true);

            if package_not_on_magic_file || package_not_installed {
                info!("Package {} is not installed", target_package.name);
                up_to_date = false;
                // we need to install the package
                self.network
                    .get_package(&target_package.file, &token)
                    .await?;
            }
        }

        if !up_to_date {
            self.magic.set_packages(target_packages).await;
        }

        Ok(())
    }

    async fn upgrade_device(&self) -> Result<()> {
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

        let packages_from_magic = self.magic.get_packages().await;

        // check if all packages are available locally
        for package in packages_from_magic.iter() {
            info!("Checking package: {}", package.name);
            let package_name = &package.name;
            let package_file = &package.file;

            // check if package is available locally
            let path = std::env::current_dir()?;
            let packages_folder = path.join("packages");
            let package_file = packages_folder.join(package_file);

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
        for package in packages_from_magic.into_iter() {
            let package_name = package.name;
            let package_file = package.file;
            let package_version = package.version;

            let path = std::env::current_dir()?;
            let packages_folder = path.join("packages");
            let package_file = packages_folder.join(&package_file);

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
                let install_command = format!(
                    "sudo apt install {} -y --allow-downgrades",
                    package_file.display()
                );
                match Command::new("sh")
                    .arg("-c")
                    .arg(&install_command)
                    .output()
                    .await
                {
                    Ok(status) => {
                        if status.status.success() {
                            info!("Successfully installed package {}", package_name);
                        } else {
                            let stderr = String::from_utf8_lossy(&status.stderr);
                            error!("Failed to install package {}: {}", package_name, stderr);
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to execute install command for {}: {}",
                            package_name, e
                        );
                    }
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

        self.are_packages_up_to_date().await
    }

    /// Checks whether packages are up to date.
    ///
    /// Returns `Ok` if all packages are, `Err` otherwise.
    async fn are_packages_up_to_date(&self) -> Result<()> {
        let shutdown = ShutdownHandler::new();
        let configuration = MagicHandle::new(shutdown.signals());
        configuration.load(None).await;

        let magic_packages = configuration.get_packages().await;

        // check the system version of the packages in the magic file
        for package in magic_packages {
            let installed_version = package.get_system_version().await?;
            let magic_toml_version = package.version;

            if magic_toml_version != installed_version {
                return Err(anyhow::anyhow!(
                    "Package {} is not up to date",
                    package.name
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
                    self.handle_message(ActorMessage::Checking).await;
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
