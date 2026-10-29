use crate::auditor::AuditorHandle;
use crate::commander::{CommanderHandle, Handles};
use crate::control::ControlHandle;
use crate::downloader::DownloaderHandle;
use crate::filebrowser::FileBrowserHandle;
use crate::filemanager::FileManagerHandle;
use crate::link_health::LinkHealthHandle;
use crate::logstream::LogStreamHandle;
use crate::magic::MagicHandle;
use crate::nm_watcher::NMWatcherHandle;
use crate::police::PoliceHandle;
use crate::postman::PostmanHandle;
use crate::session::SessionHandle;
use crate::settings::SettingsHandle;
use crate::shutdown::ShutdownHandler;
use crate::tunnel::TunnelHandle;
use crate::updater::UpdaterHandle;
use crate::utils::net_guard::NetGuard;
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

    let filebrowser =
        FileBrowserHandle::new(shutdown.signals(), configuration.clone(), session.clone());

    // Shared by the commander's network commands and link health, which stays
    // out of NetworkManager while they run.
    let net_guard = NetGuard::new();

    let commander = CommanderHandle::new(
        shutdown.signals(),
        Handles {
            magic: configuration.clone(),
            tunnel: tunnel.clone(),
            updater: updater.clone(),
            downloader: downloader.clone(),
            filemanager: filemanager.clone(),
            logstream: logstream.clone(),
            filebrowser: filebrowser.clone(),
            net_guard: net_guard.clone(),
        },
    );

    // The auditor stages its results on the commander, so it must be created
    // after it. Audit on boot, once SSH hardening has been (re)applied above.
    let auditor = AuditorHandle::new(shutdown.signals(), commander.clone());
    auditor.run_audit().await;

    let _postman = PostmanHandle::new(
        shutdown.signals(),
        police.clone(),
        commander.clone(),
        configuration.clone(),
        session.clone(),
    );

    // Queues its reports on the commander, so it must be created after it.
    let settings = SettingsHandle::new(shutdown.signals(), commander.clone(), police.clone());

    let link_health = LinkHealthHandle::new(shutdown.signals(), net_guard.clone());

    let _nm_watcher = NMWatcherHandle::new(
        shutdown.signals(),
        commander.clone(),
        settings.clone(),
        link_health.device_events(),
    );

    let _control = ControlHandle::new(
        shutdown.signals(),
        updater.clone(),
        downloader.clone(),
        tunnel.clone(),
        filemanager.clone(),
        police.clone(),
    );

    // this will ensure we have a token
    configuration.wait_while_not_registered().await;

    // wait for the sweet release of death
    shutdown.wait().await;

    info!("Agent is shutting down");
}
