use super::session::{self, OBJECT_PREFIX, SIGNED_URL_TTL_SECONDS};
use crate::State;
use crate::handlers::AuthedDevice;
use crate::home::add_commands;
use crate::middlewares::authorization;
use crate::relay::{self, Direction, Kind};
use crate::storage::Storage;
use crate::user::CurrentUser;
use axum::{
    Extension,
    body::Body,
    extract::{
        Path, Query, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::Response,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use smith::utils::schema::{FileOpRequest, FileOpResponse, SafeCommandRequest, SafeCommandTx};
use std::time::Duration;
use tracing::{error, info, warn};
use uuid::Uuid;

const FILES_TAG: &str = "files";

/// How long the dashboard waits for the device to notice the queued command and
/// dial back. Devices poll every ~20s when idle, so this must comfortably
/// exceed one poll interval.
const SESSION_CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

/// Negative ids mark server-originated commands, matching the log stream's
/// -10/-11 convention.
const OPEN_FILE_SESSION_CMD_ID: i32 = -12;
const CLOSE_FILE_SESSION_CMD_ID: i32 = -13;

/// The daemon version that first understands `OpenFileSession`. Older daemons
/// predate the tolerant command deserializer, so sending them an unrecognized
/// command costs them the whole batch plus their target release for that tick.
const MIN_DAEMON_VERSION: (u32, u32, u32) = (0, 2, 182);

#[derive(Deserialize)]
pub struct WsAuthQuery {
    token: String,
}

/// Dashboard control socket. Every filesystem operation for the session is
/// negotiated over this, so browsing pays the device poll interval once at
/// handshake rather than on every click.
#[utoipa::path(
    get,
    path = "/ws/devices/{device_serial}/files",
    params(
        ("device_serial" = String, Path, description = "Device serial number"),
    ),
    responses(
        (status = StatusCode::SWITCHING_PROTOCOLS, description = "WebSocket connection established"),
        (status = StatusCode::FORBIDDEN, description = "Missing commands:files permission"),
        (status = StatusCode::NOT_FOUND, description = "Device not found"),
        (status = StatusCode::CONFLICT, description = "Device daemon too old"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = FILES_TAG
)]
pub async fn dashboard_files_ws(
    ws: WebSocketUpgrade,
    Path(device_serial): Path<String>,
    Query(auth): Query<WsAuthQuery>,
    Extension(state): Extension<State>,
) -> Result<Response, StatusCode> {
    let claims = state
        .jwks_client
        .decode::<Value>(&auth.token, &[&state.config.auth0_audience])
        .await
        .map_err(|e| {
            error!("Token validation failed: {e}");
            StatusCode::UNAUTHORIZED
        })?;

    let sub = claims
        .get("sub")
        .and_then(|s| s.as_str())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let user_id = match CurrentUser::lookup(&state.pg_pool, sub).await {
        Ok((id, _)) => id,
        Err(sqlx::Error::RowNotFound) => return Err(StatusCode::UNAUTHORIZED),
        Err(e) => {
            error!("Database error looking up user: {e}");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // This endpoint queues a command on the user's behalf, so it must apply the
    // same gate `authorize_commands` would. Authenticating alone is not enough.
    let current_user = CurrentUser::build(&state.pg_pool, &state.authorization, user_id)
        .await
        .map_err(|e| {
            error!("Failed to build current user: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if !authorization::check(current_user, "commands", "files") {
        return Err(StatusCode::FORBIDDEN);
    }

    let device = sqlx::query!(
        r#"SELECT id, system_info->'smith'->>'version' as "version?" FROM device WHERE serial_number = $1"#,
        device_serial
    )
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|e| {
        error!("Database error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    if !daemon_supports_file_browsing(device.version.as_deref()) {
        warn!(
            "Refusing file session on device {device_serial}: daemon version {:?} is too old",
            device.version
        );
        return Err(StatusCode::CONFLICT);
    }

    let session_id = Uuid::new_v4();
    info!("Opening file session {session_id} on device {device_serial}");

    Ok(ws.on_upgrade(move |socket| {
        handle_dashboard_ws(socket, session_id, device_serial, device.id, user_id, state)
    }))
}

/// Devices report `SystemInfo.smith.version`. A device that has never reported
/// system info at all is refused rather than assumed current.
fn daemon_supports_file_browsing(version: Option<&str>) -> bool {
    let Some(version) = version else {
        return false;
    };

    let mut parts = version
        .trim()
        .split('.')
        .map(|part| part.trim().parse::<u32>());

    let (Some(Ok(major)), Some(Ok(minor)), Some(Ok(patch))) =
        (parts.next(), parts.next(), parts.next())
    else {
        return false;
    };

    (major, minor, patch) >= MIN_DAEMON_VERSION
}

async fn handle_dashboard_ws(
    socket: WebSocket,
    session_id: Uuid,
    device_serial: String,
    device_id: i32,
    user_id: i32,
    state: State,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    if let Err(e) =
        relay::create_session(&state.pg_pool, &session_id, Kind::Files, device_id, user_id).await
    {
        error!("Failed to create file session {session_id}: {e}");
        return;
    }

    // Subscribe before queueing, so a device that dials back quickly cannot
    // publish its ready frame into a channel nobody is listening on yet.
    let mut inbound = match relay::Subscription::open(
        &state.pg_pool,
        &state.config.database_url,
        &session_id,
        Direction::ToDashboard,
    )
    .await
    {
        Ok(subscription) => subscription,
        Err(e) => {
            error!("Failed to subscribe to file session {session_id}: {e}");
            relay::close_session(&state.pg_pool, &session_id).await;
            return;
        }
    };

    let command = SafeCommandRequest {
        id: OPEN_FILE_SESSION_CMD_ID,
        command: SafeCommandTx::OpenFileSession {
            session_id: session_id.to_string(),
        },
        continue_on_error: false,
    };

    if let Err(e) = add_commands(&device_serial, vec![command], &state.pg_pool, Some(user_id)).await
    {
        error!("Failed to queue OpenFileSession for {session_id}: {e}");
        relay::close_session(&state.pg_pool, &session_id).await;
        return;
    }

    // The device's dial-back publishes a `ready` frame, so waiting for it is
    // just waiting for the first inbound message.
    let ready = tokio::time::timeout(SESSION_CONNECT_TIMEOUT, inbound.next()).await;
    match ready {
        Ok(Some(frame)) => {
            send_json(&mut ws_tx, &frame).await;
        }
        _ => {
            warn!("Device did not connect to file session {session_id} in time");
            send_json(
                &mut ws_tx,
                &json!({"type": "error", "code": "Timeout", "message": "Device did not connect in time"}),
            )
            .await;
            relay::close_session(&state.pg_pool, &session_id).await;
            return;
        }
    }

    let forward_task = tokio::spawn(async move {
        while let Some(frame) = inbound.next().await {
            if !send_json(&mut ws_tx, &frame).await {
                break;
            }
        }
    });

    while let Some(msg) = ws_rx.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if forward_task.is_finished() {
                    break;
                }
                handle_dashboard_frame(&state, &session_id, device_id, user_id, &text).await;
            }
            Ok(Message::Close(_)) => break,
            Err(e) => {
                error!("Dashboard file websocket error: {e}");
                break;
            }
            _ => {}
        }
    }

    forward_task.abort();

    let stop = SafeCommandRequest {
        id: CLOSE_FILE_SESSION_CMD_ID,
        command: SafeCommandTx::CloseFileSession {
            session_id: session_id.to_string(),
        },
        continue_on_error: false,
    };
    if let Err(e) = add_commands(&device_serial, vec![stop], &state.pg_pool, Some(user_id)).await {
        error!("Failed to queue CloseFileSession for {session_id}: {e}");
    }

    relay::close_session(&state.pg_pool, &session_id).await;
    info!("File session {session_id} ended");
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum DashboardCommand {
    List { op_id: u64, path: String },
    Download { op_id: u64, path: String },
    Cancel { op_id: u64 },
}

async fn handle_dashboard_frame(
    state: &State,
    session_id: &Uuid,
    device_id: i32,
    user_id: i32,
    text: &str,
) {
    let command: DashboardCommand = match serde_json::from_str(text) {
        Ok(command) => command,
        Err(e) => {
            warn!("Ignoring malformed dashboard file command: {e}");
            return;
        }
    };

    // POSIX paths cannot contain NUL, and Postgres text rejects it. Refuse
    // rather than trust the far end to be well-behaved.
    let request = match command {
        DashboardCommand::List { op_id, path } => {
            if path.contains('\0') {
                return;
            }
            session::record_access(
                &state.pg_pool,
                device_id,
                Some(user_id),
                session_id,
                "list",
                &path,
                None,
                "ok",
                None,
            )
            .await;
            FileOpRequest::List { op_id, path }
        }
        DashboardCommand::Download { op_id, path } => {
            if path.contains('\0') {
                return;
            }
            session::record_access(
                &state.pg_pool,
                device_id,
                Some(user_id),
                session_id,
                "open",
                &path,
                None,
                "ok",
                None,
            )
            .await;
            FileOpRequest::Open { op_id, path }
        }
        DashboardCommand::Cancel { op_id } => FileOpRequest::Cancel { op_id },
    };

    publish_to_device(state, session_id, &request).await;
}

async fn publish_to_device(state: &State, session_id: &Uuid, request: &FileOpRequest) {
    let payload = match serde_json::to_value(request) {
        Ok(payload) => payload,
        Err(e) => {
            error!("Failed to encode file operation: {e}");
            return;
        }
    };

    relay::publish(&state.pg_pool, session_id, Direction::ToDevice, &payload)
        .await
        .inspect_err(|e| error!("Failed to relay file operation to device: {e}"))
        .ok();
}

async fn send_json(
    ws_tx: &mut futures::stream::SplitSink<WebSocket, Message>,
    value: &Value,
) -> bool {
    match serde_json::to_string(value) {
        Ok(text) => ws_tx.send(Message::Text(text)).await.is_ok(),
        Err(e) => {
            error!("Failed to encode frame for dashboard: {e}");
            true
        }
    }
}

/// Device control socket. Unlike the log stream's device endpoint, this requires
/// `AuthedDevice` *and* checks that the authenticated device owns the session —
/// knowing a session id is not a credential.
#[utoipa::path(
    get,
    path = "/ws/file-session/{session_id}",
    params(
        ("session_id" = String, Path, description = "File session id"),
    ),
    responses(
        (status = StatusCode::SWITCHING_PROTOCOLS, description = "WebSocket connection established"),
        (status = StatusCode::NOT_FOUND, description = "Session not found or already closed"),
        (status = StatusCode::FORBIDDEN, description = "Session belongs to a different device"),
    ),
    security(
        ("device_token" = [])
    ),
    tag = FILES_TAG
)]
pub async fn device_files_ws(
    ws: WebSocketUpgrade,
    device: AuthedDevice,
    Path(session_id): Path<Uuid>,
    Extension(state): Extension<State>,
) -> Result<Response, StatusCode> {
    let session = relay::lookup_open(&state.pg_pool, &session_id, Kind::Files)
        .await
        .map_err(|e| {
            error!("Database error looking up file session: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    if session.device_id != device.id {
        warn!(
            "Device {} tried to attach to file session {session_id} owned by device {}",
            device.id, session.device_id
        );
        return Err(StatusCode::FORBIDDEN);
    }

    info!(
        "Device {} connected to file session {session_id}",
        device.id
    );

    Ok(ws.on_upgrade(move |socket| handle_device_ws(socket, session_id, state)))
}

async fn handle_device_ws(socket: WebSocket, session_id: Uuid, state: State) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    let mut outbound = match relay::Subscription::open(
        &state.pg_pool,
        &state.config.database_url,
        &session_id,
        Direction::ToDevice,
    )
    .await
    {
        Ok(subscription) => subscription,
        Err(e) => {
            error!("Failed to subscribe device side of file session {session_id}: {e}");
            return;
        }
    };

    relay::mark_device_connected(&state.pg_pool, &session_id)
        .await
        .inspect_err(|e| error!("Failed to mark device connected: {e}"))
        .ok();

    // Tell the dashboard the session is live. This doubles as the handshake
    // signal it is blocked waiting on.
    relay::publish(
        &state.pg_pool,
        &session_id,
        Direction::ToDashboard,
        &json!({"type": "ready"}),
    )
    .await
    .inspect_err(|e| error!("Failed to publish file session ready: {e}"))
    .ok();

    // Anything queued between the dashboard subscribing and this socket
    // existing already fired its NOTIFY into the void; replay it.
    match relay::drain_pending(&state.pg_pool, &session_id, Direction::ToDevice).await {
        Ok(pending) => {
            for frame in pending {
                if !send_json(&mut ws_tx, &frame).await {
                    return;
                }
            }
        }
        Err(e) => error!("Failed to drain pending file operations: {e}"),
    }

    loop {
        tokio::select! {
            frame = outbound.next() => {
                let Some(frame) = frame else { break };
                if !send_json(&mut ws_tx, &frame).await {
                    break;
                }
            }
            msg = ws_rx.next() => {
                let Some(msg) = msg else { break };
                match msg {
                    Ok(Message::Text(text)) => {
                        if !relay_device_response(&state, &session_id, &text).await {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(Message::Ping(data)) => {
                        if ws_tx.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Device file websocket error: {e}");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    info!("Device disconnected from file session {session_id}");
}

/// Forward a device response to the dashboard, intercepting `Opened` to mint the
/// upload ticket the transfer needs.
async fn relay_device_response(state: &State, session_id: &Uuid, text: &str) -> bool {
    let response: FileOpResponse = match serde_json::from_str(text) {
        Ok(response) => response,
        Err(e) => {
            warn!("Ignoring malformed device file response: {e}");
            return true;
        }
    };

    if let FileOpResponse::Opened { op_id, name, size } = &response {
        start_transfer(state, session_id, *op_id, name, *size).await;
        return true;
    }

    let payload = match serde_json::to_value(&response) {
        Ok(payload) => payload,
        Err(e) => {
            error!("Failed to encode device response: {e}");
            return true;
        }
    };

    relay::publish(&state.pg_pool, session_id, Direction::ToDashboard, &payload)
        .await
        .inspect_err(|e| error!("Failed to relay device response: {e}"))
        .is_ok()
}

/// Mint a single-use upload token and ask the device to start streaming.
async fn start_transfer(state: &State, session_id: &Uuid, op_id: u64, name: &str, size: u64) {
    let upload_token = Uuid::new_v4().simple().to_string();
    // The object key includes the session so a bucket listing is attributable,
    // and the op id so two downloads of the same file don't collide. The
    // device-chosen file name stays out of the key so a device can't steer
    // where the object lands; it is stored separately as the display name.
    let object_key = format!(
        "{OBJECT_PREFIX}/{session_id}/{op_id}/{}",
        Uuid::new_v4().simple()
    );

    if let Err(e) = session::create_download(
        &state.pg_pool,
        session_id,
        op_id as i64,
        &upload_token,
        &object_key,
        name,
        size as i64,
    )
    .await
    {
        error!("Failed to create download ticket: {e}");
        return;
    }

    publish_to_device(
        state,
        session_id,
        &FileOpRequest::StartUpload {
            op_id,
            upload_token,
        },
    )
    .await;
}

/// Device upload endpoint. Streams the request body straight into S3 — the body
/// is never buffered, so a 512 MiB file costs a chunk of api memory, not 512 MiB.
#[utoipa::path(
    post,
    path = "/smith/files/upload",
    responses(
        (status = StatusCode::OK, description = "File staged"),
        (status = StatusCode::FORBIDDEN, description = "Unknown or already-used upload token"),
    ),
    security(
        ("device_token" = [])
    ),
    tag = FILES_TAG
)]
pub async fn upload_file(
    _device: AuthedDevice,
    Extension(state): Extension<State>,
    headers: HeaderMap,
    body: Body,
) -> Result<StatusCode, StatusCode> {
    let upload_token = headers
        .get("X-Upload-Token")
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;

    let pending = session::claim_upload(&state.pg_pool, upload_token)
        .await
        .map_err(|e| {
            error!("Database error claiming upload token: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::FORBIDDEN)?;

    let stream = body
        .into_data_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let mut reader = tokio_util::io::StreamReader::new(stream);

    let status = Storage::stream_to_s3(
        &state.config.assets_bucket_name,
        &pending.object_key,
        &mut reader,
    )
    .await
    .map_err(|e| {
        error!("Failed to stage file in S3: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if !(200..300).contains(&status) {
        error!("S3 rejected staged file with status {status}");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let url = Storage::signed_url(
        &state.config.cloudfront.package_domain_name,
        &state.config.cloudfront.package_key_pair_id,
        &state.config.cloudfront.package_private_key,
        &pending.object_key,
        SIGNED_URL_TTL_SECONDS,
    )
    .map_err(|e| {
        error!("Failed to sign download URL: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Whichever replica holds the dashboard socket picks this up and hands the
    // link to the browser, which then fetches straight from the CDN.
    relay::publish(
        &state.pg_pool,
        &pending.session_id,
        Direction::ToDashboard,
        &json!({
            "type": "download_ready",
            "op_id": pending.op_id,
            "url": url,
            "name": pending.file_name,
            "size": pending.size,
        }),
    )
    .await
    .map_err(|e| {
        error!("Failed to publish download link: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // The download is only real once the bytes are staged, so this is where the
    // audit row belongs — the earlier `open` row records intent, not transfer.
    match relay::lookup_any(&state.pg_pool, &pending.session_id).await {
        Ok(Some(row)) => {
            session::record_access(
                &state.pg_pool,
                row.device_id,
                row.user_id,
                &pending.session_id,
                "download",
                &pending.object_key,
                Some(pending.size),
                "ok",
                None,
            )
            .await;
        }
        Ok(None) => warn!(
            "Staged a file for unknown session {}; audit row skipped",
            pending.session_id
        ),
        Err(e) => error!("Failed to look up session for audit: {e}"),
    }

    info!(
        "Staged {} ({} bytes) for file session {}",
        pending.file_name, pending.size, pending.session_id
    );

    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_daemons_at_or_above_the_minimum_version() {
        assert!(daemon_supports_file_browsing(Some("0.2.182")));
        assert!(daemon_supports_file_browsing(Some("0.2.200")));
        assert!(daemon_supports_file_browsing(Some("0.3.0")));
        assert!(daemon_supports_file_browsing(Some("1.0.0")));
    }

    #[test]
    fn refuses_daemons_that_predate_the_tolerant_deserializer() {
        // These would lose their whole command batch, plus target_release_id,
        // on receiving a command they don't recognize.
        assert!(!daemon_supports_file_browsing(Some("0.2.181")));
        assert!(!daemon_supports_file_browsing(Some("0.1.999")));
    }

    #[test]
    fn refuses_devices_with_unusable_version_info() {
        assert!(!daemon_supports_file_browsing(None));
        assert!(!daemon_supports_file_browsing(Some("")));
        assert!(!daemon_supports_file_browsing(Some("unknown")));
        assert!(!daemon_supports_file_browsing(Some("0.2")));
    }
}
