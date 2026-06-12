use crate::commander::{CommanderHandle, Handles};
use crate::dbus::DbusHandle;
use crate::downloader::DownloaderHandle;
use crate::filemanager::FileManagerHandle;
use crate::logstream::LogStreamHandle;
use crate::magic::MagicHandle;
use crate::police::PoliceHandle;
use crate::postman::PostmanHandle;
use crate::session::SessionHandle;
use crate::shutdown::ShutdownHandler;
use crate::tunnel::TunnelHandle;
use crate::updater::UpdaterHandle;
use crate::utils::system::SystemInfo;
use tracing::info;

pub async fn run() {
    SystemInfo::new().await.print();

    let shutdown = ShutdownHandler::new();

    let configuration = MagicHandle::new(shutdown.signals());

    configuration.load(None).await;

    // Kill shared-password SSH logins on every boot (self-heals if the config
    // gets reset by an OS update or someone flips it back). A failure here must
    // not stop the daemon from starting.
    if let Err(err) = crate::utils::files::disable_ssh_password_auth().await {
        tracing::error!("Failed to disable SSH password auth: {err:#}");
    }

    let session = SessionHandle::new(shutdown.signals(), configuration.clone());

    let tunnel = TunnelHandle::new(shutdown.signals(), configuration.clone());

    let police = PoliceHandle::new(shutdown.signals());

    let downloader =
        DownloaderHandle::new(shutdown.signals(), configuration.clone(), session.clone());

    let updater = UpdaterHandle::new(
        shutdown.signals(),
        configuration.clone(),
        downloader.clone(),
        session.clone(),
    );

    let filemanager = FileManagerHandle::new(shutdown.signals(), configuration.clone());

    let logstream =
        LogStreamHandle::new(shutdown.signals(), configuration.clone(), session.clone());

    let commander = CommanderHandle::new(
        shutdown.signals(),
        Handles {
            magic: configuration.clone(),
            tunnel: tunnel.clone(),
            updater: updater.clone(),
            downloader: downloader.clone(),
            filemanager: filemanager.clone(),
            logstream: logstream.clone(),
        },
    );

    let _postman = PostmanHandle::new(
        shutdown.signals(),
        police.clone(),
        commander.clone(),
        configuration.clone(),
        session.clone(),
    );

    let _dbus = DbusHandle::new(
        shutdown.signals(),
        updater.clone(),
        downloader.clone(),
        tunnel.clone(),
        filemanager.clone(),
    );

    // this will ensure we have a token
    configuration.wait_while_not_registered().await;

    // wait for the sweet release of death
    shutdown.wait().await;

    info!("Agent is shutting down");
}
