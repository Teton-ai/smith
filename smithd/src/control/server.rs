//! Local control API.
//!
//! Serves HTTP over a Unix socket at [`CONTROL_SOCKET`] so the `smithd`
//! subcommands and other services on the device can query and drive the running
//! daemon. This replaces the former `ai.teton.smith.Packages1` D-Bus interface.
//!
//! The socket is root-only (0660): this surface can flash an OS image and open
//! tunnels to the internet, so it is deliberately narrower than the D-Bus policy
//! it replaces, which allowed any local user to call every method.

use super::{
    CONTROL_SOCKET, CheckResponse, DownloadRequest, ErrorResponse, HoldRequest, MessageResponse,
    TunnelRequest, TunnelResponse,
};
use crate::downloader::DownloaderHandle;
use crate::filemanager::FileManagerHandle;
use crate::police::{PoliceHandle, RebootStatus};
use crate::shutdown::ShutdownSignals;
use crate::tunnel::TunnelHandle;
use crate::updater::UpdaterHandle;
use anyhow::Context;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tokio::net::UnixListener;
use tracing::{error, info};

#[derive(Clone)]
struct ControlState {
    updater: UpdaterHandle,
    downloader: DownloaderHandle,
    tunnel: TunnelHandle,
    filemanager: FileManagerHandle,
}

/// Any handler failure becomes a 500 with a JSON body, and is logged once here
/// so individual handlers stay free of error plumbing.
struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        error!("Control request failed: {:#}", self.0);
        let body = ErrorResponse {
            error: format!("{:#}", self.0),
        };
        (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
    }
}

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

async fn watchdog(State(police): State<PoliceHandle>) -> Json<RebootStatus> {
    Json(police.status().await)
}

async fn watchdog_hold(
    State(police): State<PoliceHandle>,
    body: Option<Json<HoldRequest>>,
) -> Json<RebootStatus> {
    let ttl_seconds = body.and_then(|Json(request)| request.ttl_seconds);
    Json(police.hold(ttl_seconds).await)
}

async fn watchdog_release(State(police): State<PoliceHandle>) -> Json<RebootStatus> {
    Json(police.release_hold().await)
}

async fn updater_status(State(state): State<ControlState>) -> String {
    state.updater.status().await
}

async fn updater_check(State(state): State<ControlState>) -> Json<CheckResponse> {
    Json(CheckResponse {
        updates_available: state.updater.prepare_release().await,
    })
}

async fn updater_upgrade(State(state): State<ControlState>) -> Json<MessageResponse> {
    let scheduled = state.updater.install_prepared_release().await;
    Json(MessageResponse {
        message: if scheduled {
            "Packages upgrade scheduled".to_owned()
        } else {
            "Unable to schedule packages upgrade".to_owned()
        },
    })
}

async fn open_tunnel(
    State(state): State<ControlState>,
    body: Option<Json<TunnelRequest>>,
) -> Json<TunnelResponse> {
    let port = body.and_then(|Json(request)| request.port);
    info!("Exposing port {port:?}");
    let public_port = state.tunnel.start_tunnel(port, None, None).await;
    Json(TunnelResponse { public_port })
}

async fn start_download(
    State(state): State<ControlState>,
    Json(request): Json<DownloadRequest>,
) -> Result<Json<MessageResponse>, ApiError> {
    state
        .downloader
        .download(&request.remote_file, &request.local_file, request.rate_mb)
        .await?;

    Ok(Json(MessageResponse {
        message: "Download started. Not waiting for result".to_owned(),
    }))
}

/// Applies a staged OTA payload and reboots the device on success.
async fn start_ota(State(state): State<ControlState>) -> Result<Json<MessageResponse>, ApiError> {
    state
        .filemanager
        .extract_here("/otatool/ota_tools.tbz2")
        .await
        .context("Failed to extract OTA tools")?;

    let script_result = state
        .filemanager
        .execute_script(
            "nv_ota_start.sh",
            vec!["/ota/ota_payload_package.tar.gz".to_owned()],
            Some("/otatool/Linux_for_Tegra/tools/ota_tools/version_upgrade/"),
        )
        .await
        .context("Script execution failed")?;

    // Only proceed with reboot if script execution was successful
    if let Err(e) = state
        .filemanager
        .execute_system_command("reboot", Vec::new(), None)
        .await
    {
        error!("Failed to reboot after OTA: {e:#}");
    }

    Ok(Json(MessageResponse {
        message: script_result,
    }))
}

/// The police owns its own state, so the watchdog route is kept separate from
/// the device-operations routes rather than widening [`ControlState`].
fn watchdog_router(police: PoliceHandle) -> Router {
    Router::new()
        .route("/watchdog", get(watchdog))
        .route(
            "/watchdog/hold",
            post(watchdog_hold).delete(watchdog_release),
        )
        .with_state(police)
}

fn router(state: ControlState, police: PoliceHandle) -> Router {
    watchdog_router(police).merge(
        Router::new()
            .route("/updater/status", get(updater_status))
            .route("/updater/check", post(updater_check))
            .route("/updater/upgrade", post(updater_upgrade))
            .route("/tunnel", post(open_tunnel))
            .route("/downloads", post(start_download))
            .route("/ota/start", post(start_ota))
            .with_state(state),
    )
}

async fn serve(socket: &Path, app: Router, shutdown: ShutdownSignals) -> anyhow::Result<()> {
    if let Some(parent) = socket.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create {}", parent.display()))?;

        // bind() creates the socket with the process umask, so it is briefly
        // world-accessible before the chmod below. Locking the directory down
        // first closes that window, and also covers a pre-existing directory
        // created with looser permissions.
        tokio::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .await
            .with_context(|| format!("Failed to set permissions on {}", parent.display()))?;
    }

    // A hard kill leaves the socket file behind and bind() would then fail with
    // EADDRINUSE, so clear any stale one first.
    if let Err(e) = tokio::fs::remove_file(socket).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        return Err(e).with_context(|| format!("Failed to remove stale {}", socket.display()));
    }

    let listener = UnixListener::bind(socket)
        .with_context(|| format!("Failed to bind {}", socket.display()))?;

    tokio::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o660))
        .await
        .with_context(|| format!("Failed to set permissions on {}", socket.display()))?;

    info!("Control API listening on {}", socket.display());

    axum::serve(listener, app)
        .with_graceful_shutdown(async move { shutdown.token.cancelled().await })
        .await
        .context("Control API server failed")?;

    info!("Control API shut down");
    Ok(())
}

#[derive(Clone)]
pub struct ControlHandle;

impl ControlHandle {
    pub fn new(
        shutdown: ShutdownSignals,
        updater: UpdaterHandle,
        downloader: DownloaderHandle,
        tunnel: TunnelHandle,
        filemanager: FileManagerHandle,
        police: PoliceHandle,
    ) -> Self {
        let state = ControlState {
            updater,
            downloader,
            tunnel,
            filemanager,
        };

        tokio::spawn(async move {
            let app = router(state, police);
            if let Err(e) = serve(Path::new(CONTROL_SOCKET), app, shutdown).await {
                error!("Control API stopped: {e:#}");
            }
        });

        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shutdown::ShutdownHandler;

    /// Exercises the real transport end to end: axum serving over a Unix socket,
    /// reqwest dialling it via `unix_socket`, and the police reporting status.
    #[tokio::test]
    async fn watchdog_is_queryable_over_the_control_socket() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("s");

        let shutdown = ShutdownHandler::new();
        let police = PoliceHandle::new(shutdown.signals());

        let app = watchdog_router(police);
        let serve_socket = socket.clone();
        let signals = shutdown.signals();
        tokio::spawn(async move { serve(&serve_socket, app, signals).await });

        let client = reqwest::Client::builder()
            .unix_socket(socket.as_path())
            .build()
            .expect("client");

        // bind() creates the socket file before serve() chmods it, so a serving
        // response - not the file existing - is what proves setup finished.
        let mut response = None;
        for _ in 0..100 {
            if let Ok(r) = client.get("http://localhost/watchdog").send().await {
                response = Some(r);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let response = response.expect("request");

        let mode = std::fs::metadata(&socket)
            .expect("socket metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o660, "control socket must not be world-accessible");

        assert!(response.status().is_success());

        let status: RebootStatus = response.json().await.expect("json");

        // Nothing has reported a problem, so no reboot should be scheduled.
        assert!(!status.reboot_pending);
        assert_eq!(status.seconds_remaining, 0);

        // A hold can be placed before any reboot is scheduled (e.g. a technician
        // connects to the AP during plex's own outage window, ahead of smithd
        // arming) and must survive to defer a later schedule.
        let status: RebootStatus = client
            .post("http://localhost/watchdog/hold")
            .json(&HoldRequest {
                ttl_seconds: Some(300),
            })
            .send()
            .await
            .expect("hold request")
            .json()
            .await
            .expect("hold json");

        assert!(status.held);
        assert!(status.hold_seconds_remaining > 0 && status.hold_seconds_remaining <= 300);
        assert!(!status.reboot_pending);

        let status: RebootStatus = client
            .delete("http://localhost/watchdog/hold")
            .send()
            .await
            .expect("release request")
            .json()
            .await
            .expect("release json");

        assert!(!status.held);
        assert_eq!(status.hold_seconds_remaining, 0);
    }
}
