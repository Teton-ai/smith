use crate::bouncer::BouncerHandle;
use crate::commander::CommanderHandle;
use crate::dbus::DbusHandle;
use crate::downloader::DownloaderHandle;
use crate::filemanager::FileManagerHandle;
use crate::magic::MagicHandle;
use crate::police::PoliceHandle;
use crate::postman::PostmanHandle;
use crate::shutdown::ShutdownHandler;
use crate::tunnel::TunnelHandle;
use crate::updater::UpdaterHandle;
use crate::utils::system::new_system_info;
use tracing::{error, info};

pub async fn run() {
    match serde_json::to_string_pretty(&new_system_info().await) {
        Ok(json) => info!("{}", json),
        Err(_) => error!("Failed to parse system info"),
    };

    let shutdown = ShutdownHandler::new();

    let configuration = MagicHandle::new(shutdown.signals());

    configuration.load(None).await;

    let tunnel = TunnelHandle::new(shutdown.signals(), configuration.clone());

    let police = PoliceHandle::new(shutdown.signals());

    let downloader = DownloaderHandle::new(shutdown.signals(), configuration.clone());

    let updater = UpdaterHandle::new(
        shutdown.signals(),
        configuration.clone(),
        downloader.clone(),
    );

    let filemanager = FileManagerHandle::new(shutdown.signals(), configuration.clone());

    let commander = CommanderHandle::new(
        shutdown.signals(),
        tunnel.clone(),
        updater.clone(),
        downloader.clone(),
        filemanager.clone(),
    );

    let _postman = PostmanHandle::new(
        shutdown.signals(),
        police.clone(),
        commander.clone(),
        configuration.clone(),
    );

    let _dbus = DbusHandle::new(
        shutdown.signals(),
        updater.clone(),
        downloader.clone(),
        tunnel.clone(),
        filemanager.clone(),
    );

    let bouncer = BouncerHandle::new(shutdown.signals(), configuration.clone(), police.clone());

    // this will ensure we have a token
    configuration.wait_while_not_registered().await;

    // this will block while we try to have all the checks passing
    bouncer.ok().await;

    // wait for the sweet release of death
    shutdown.wait().await;

    info!("Agent is shutting down");
}
