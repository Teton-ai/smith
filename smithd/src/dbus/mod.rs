use crate::downloader::DownloaderHandle;
use crate::filemanager::FileManagerHandle;
use crate::shutdown::ShutdownSignals;
use crate::tunnel::TunnelHandle;
use crate::updater::UpdaterHandle;
use tracing::info;
use zbus::{connection, interface};

mod client;
pub(crate) use client::SmithDbusProxy;

struct DBus {
    shutdown: ShutdownSignals,
    updater: UpdaterHandle,
    downloader: DownloaderHandle,
    tunnel: TunnelHandle,
    filemanager: FileManagerHandle,
}

struct PackagesInterface {
    updater: UpdaterHandle,
    downloader: DownloaderHandle,
    tunnel: TunnelHandle,
    filemanager: FileManagerHandle,
}

// interface for the D-Bus service, version 1
#[interface(name = "ai.teton.smith.Packages1")]
impl PackagesInterface {
    async fn update_packages(&mut self) -> String {
        self.updater.check_for_updates().await;
        "Packages update scheduled".to_string()
    }

    async fn upgrade_packages(&mut self) -> String {
        self.updater.upgrade_device().await;
        "Packages upgrade scheduled".to_string()
    }

    async fn updater_status(&mut self) -> String {
        self.updater.status().await
    }

    async fn expose_port(&mut self, port: u16) -> String {
        info!("Exposing port {}", port);
        let public_port = self.tunnel.start_tunnel(Some(port), None, None).await;
        public_port.to_string()
    }
    async fn download_file_rl(
        &mut self,
        remote_file: &str,
        local_file: &str,
        rate_mb: f64,
    ) -> String {
        match self
            .downloader
            .download(remote_file, local_file, rate_mb)
            .await
        {
            Ok(string) => string,

            Err(e) => {
                let error_str = format!("Download failed - {}", e);

                error_str
            }
        }
    }

    async fn start_ota(&self) -> String {
        match self
            .filemanager
            .extract_here("/otatool/ota_tools.tbz2")
            .await
        {
            Ok(_) => {
                let mut args = Vec::new();

                args.push("/ota/ota_payload_package.tar.gz".to_owned());

                match self
                    .filemanager
                    .execute_script(
                        "nv_ota_start.sh",
                        args,
                        Some("/otatool/Linux_for_Tegra/tools/ota_tools/version_upgrade/"),
                    )
                    .await
                {
                    Ok(script_result) => {
                        // Only proceed with reboot if script execution was successful

                        let reboot_args = Vec::new();

                        let _ = self
                            .filemanager
                            .execute_system_command("reboot", reboot_args, None)
                            .await;

                        script_result
                    }

                    Err(e) => {
                        format!("Script execution failed - {}", e)
                    }
                }
            }

            Err(err) => {
                format!("Failed to extract OTA tools - {}", err)
            }
        }
    }
}

impl DBus {
    fn new(
        shutdown: ShutdownSignals,
        updater: UpdaterHandle,
        downloader: DownloaderHandle,
        tunnel: TunnelHandle,
        filemanager: FileManagerHandle,
    ) -> Self {
        Self {
            shutdown,
            updater,
            downloader,
            tunnel,
            filemanager,
        }
    }

    async fn run(&mut self) {
        info!("DBus task is runnning");
        let greeter = PackagesInterface {
            updater: self.updater.clone(),
            downloader: self.downloader.clone(),
            tunnel: self.tunnel.clone(),
            filemanager: self.filemanager.clone(),
        };
        let _conn = connection::Builder::system()
            .expect("Failed to create D-Bus connection")
            .name("ai.teton.smith")
            .expect("Failed to set D-Bus name")
            .serve_at("/ai/teton/smith/Packages", greeter)
            .expect("Failed to serve D-Bus interface")
            .build()
            .await
            .expect("Failed to build D-Bus connection");

        // wait for the shutdown signal
        self.shutdown.token.cancelled().await;
    }
}

#[derive(Clone)]
pub struct DbusHandle {}

impl DbusHandle {
    pub fn new(
        shutdown: ShutdownSignals,
        updater: UpdaterHandle,
        downloader: DownloaderHandle,
        tunnel: TunnelHandle,
        filemanager: FileManagerHandle,
    ) -> Self {
        let mut actor = DBus::new(shutdown, updater, downloader, tunnel, filemanager);
        tokio::spawn(async move { actor.run().await });

        Self {}
    }
}
