use crate::auditor::AuditorHandle;
use crate::commander::{CommanderHandle, Handles};
use crate::dbus::DbusHandle;
use crate::downloader::DownloaderHandle;
use crate::filemanager::FileManagerHandle;
use crate::logstream::LogStreamHandle;
use crate::magic::MagicHandle;
use crate::nm_watcher::NMWatcherHandle;
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

    let session = SessionHandle::new(shutdown.signals(), configuration.clone());

    // Kill shared-password SSH logins on every boot (self-heals if the config
    // gets reset by an OS update or someone flips it back). A failure here must
    // not stop the daemon from starting.
    if let Err(err) = crate::utils::files::disable_ssh_password_auth().await {
        tracing::error!("Failed to disable SSH password auth: {err:#}");
    }

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

    // The auditor stages its results on the commander, so it must be created
    // after it. Audit on boot, once SSH hardening has been (re)applied above.
    let auditor = AuditorHandle::new(shutdown.signals(), commander.clone());
    auditor.run_audit().await;

    // Diagnose connectivity once on boot. Spawned so a slow sweep never delays
    // startup, and run even before registration so we can explain why a device
    // can't reach us. The report is queued and uploaded via /home when reachable.
    crate::netdiag::handler::run_on_startup(configuration.clone());

    let _postman = PostmanHandle::new(
        shutdown.signals(),
        police.clone(),
        commander.clone(),
        configuration.clone(),
        session.clone(),
    );

    let _nm_watcher = NMWatcherHandle::new(shutdown.signals(), commander.clone());

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
