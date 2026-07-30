use crate::State;
use crate::handlers::AuthedDevice;
use crate::home::add_commands;
use crate::relay::{self, Direction, Kind};
use crate::user::CurrentUser;
use axum::{
    Extension,
    extract::{
        Path, Query, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::Response,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use smith::utils::schema::{SafeCommandRequest, SafeCommandTx};
use std::time::Duration;
use tracing::{error, info, warn};
use uuid::Uuid;

/// How long the dashboard waits for the device to notice the queued command and
/// dial back. Devices poll every ~20s when idle, so this must comfortably
/// exceed one poll interval.
const SESSION_CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

/// Log lines arrive far faster than file operations do, and every relayed frame
/// is an INSERT plus a NOTIFY. Coalescing what is already buffered into one
/// frame keeps a chatty unit from turning into thousands of rows per second.
const BATCH_WINDOW: Duration = Duration::from_millis(100);
const MAX_BATCH_LINES: usize = 200;

const START_LOG_STREAM_CMD_ID: i32 = -10;
const STOP_LOG_STREAM_CMD_ID: i32 = -11;

#[derive(Deserialize)]
pub struct WsAuthQuery {
    token: String,
}

const LOGSTREAM_TAG: &str = "logstream";

/// WebSocket endpoint for dashboard to receive log stream.
///
/// Frames are JSON so the browser can tell the handshake apart from log output:
/// `{"type":"ready"}` once the device attaches, then
/// `{"type":"lines","lines":[...]}`, or `{"type":"error","message":...}`.
#[utoipa::path(
    get,
    path = "/ws/devices/{device_serial}/logs/{service_name}",
    params(
        ("device_serial" = String, Path, description = "Device serial number"),
        ("service_name" = String, Path, description = "Service name to stream logs from"),
    ),
    responses(
        (status = StatusCode::SWITCHING_PROTOCOLS, description = "WebSocket connection established"),
        (status = StatusCode::NOT_FOUND, description = "Device not found"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = LOGSTREAM_TAG
)]
pub async fn dashboard_logs_ws(
    ws: WebSocketUpgrade,
    Path((device_serial, service_name)): Path<(String, String)>,
    Query(auth): Query<WsAuthQuery>,
    Extension(state): Extension<State>,
) -> Result<Response, StatusCode> {
    // Validate the JWT token and extract the sub claim for user attribution
    let claims = state
        .jwks_client
        .decode::<Value>(&auth.token, &[&state.config.auth0_audience])
        .await
        .map_err(|e| {
            error!("Token validation failed: {}", e);
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
            error!("Database error looking up user: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let device = sqlx::query!(
        "SELECT id FROM device WHERE serial_number = $1",
        device_serial
    )
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|e| {
        error!("Database error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    let session_id = Uuid::new_v4();

    info!(
        "Dashboard requesting logs for device {} service {} - session {}",
        device_serial, service_name, session_id
    );

    Ok(ws.on_upgrade(move |socket| {
        handle_dashboard_ws(
            socket,
            session_id,
            device_serial,
            service_name,
            device.id,
            user_id,
            state,
        )
    }))
}

async fn handle_dashboard_ws(
    socket: WebSocket,
    session_id: Uuid,
    device_serial: String,
    service_name: String,
    device_id: i32,
    user_id: i32,
    state: State,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    if let Err(e) =
        relay::create_session(&state.pg_pool, &session_id, Kind::Logs, device_id, user_id).await
    {
        error!("Failed to create log session {session_id}: {e}");
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
            error!("Failed to subscribe to log session {session_id}: {e}");
            relay::close_session(&state.pg_pool, &session_id).await;
            return;
        }
    };

    let command = SafeCommandRequest {
        id: START_LOG_STREAM_CMD_ID,
        command: SafeCommandTx::StreamLogs {
            session_id: session_id.to_string(),
            service_name: service_name.clone(),
        },
        continue_on_error: false,
    };

    if let Err(e) = add_commands(&device_serial, vec![command], &state.pg_pool, Some(user_id)).await
    {
        error!("Failed to queue StreamLogs command: {e}");
        relay::close_session(&state.pg_pool, &session_id).await;
        return;
    }

    info!("Queued StreamLogs command for session {session_id}");

    // The device's dial-back publishes a `ready` frame, so waiting for it is
    // just waiting for the first inbound message.
    match tokio::time::timeout(SESSION_CONNECT_TIMEOUT, inbound.next()).await {
        Ok(Some(frame)) => {
            send_json(&mut ws_tx, &frame).await;
        }
        _ => {
            warn!("Device did not connect to log session {session_id} in time");
            send_json(
                &mut ws_tx,
                &json!({"type": "error", "message": "Device did not connect in time"}),
            )
            .await;
            relay::close_session(&state.pg_pool, &session_id).await;
            return;
        }
    }

    // Relay frames outward until either end goes away. The browser only ever
    // sends control frames, so its half just watches for the close.
    loop {
        tokio::select! {
            frame = inbound.next() => {
                match frame {
                    Some(frame) => {
                        if !send_json(&mut ws_tx, &frame).await {
                            break;
                        }
                    }
                    None => break,
                }
            }
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => {
                        info!("Dashboard closed log session {session_id}");
                        break;
                    }
                    Some(Err(e)) => {
                        error!("Dashboard websocket error on log session {session_id}: {e}");
                        break;
                    }
                    Some(Ok(_)) => {}
                }
            }
        }
    }

    let stop_command = SafeCommandRequest {
        id: STOP_LOG_STREAM_CMD_ID,
        command: SafeCommandTx::StopLogStream {
            session_id: session_id.to_string(),
        },
        continue_on_error: false,
    };
    if let Err(e) = add_commands(
        &device_serial,
        vec![stop_command],
        &state.pg_pool,
        Some(user_id),
    )
    .await
    {
        error!("Failed to queue StopLogStream command: {e}");
    }

    relay::close_session(&state.pg_pool, &session_id).await;

    info!("Dashboard log stream ended for session {session_id}");
}

/// WebSocket endpoint for device to send log stream
#[utoipa::path(
    get,
    path = "/ws/stream-logs/{session_id}",
    params(
        ("session_id" = String, Path, description = "Log stream session ID"),
    ),
    responses(
        (status = StatusCode::SWITCHING_PROTOCOLS, description = "WebSocket connection established"),
        (status = StatusCode::NOT_FOUND, description = "Session not found or already closed"),
        (status = StatusCode::FORBIDDEN, description = "Session belongs to a different device"),
    ),
    security(
        ("device_token" = [])
    ),
    tag = LOGSTREAM_TAG
)]
pub async fn device_logs_ws(
    ws: WebSocketUpgrade,
    device: AuthedDevice,
    Path(session_id): Path<Uuid>,
    Extension(state): Extension<State>,
) -> Result<Response, StatusCode> {
    let session = relay::lookup_open(&state.pg_pool, &session_id, Kind::Logs)
        .await
        .map_err(|e| {
            error!("Database error looking up log session: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    if session.device_id != device.id {
        warn!(
            "Device {} tried to attach to log session {session_id} owned by device {}",
            device.id, session.device_id
        );
        return Err(StatusCode::FORBIDDEN);
    }

    info!("Device {} connected to log session {session_id}", device.id);

    Ok(ws.on_upgrade(move |socket| handle_device_ws(socket, session_id, state)))
}

async fn handle_device_ws(socket: WebSocket, session_id: Uuid, state: State) {
    let (mut ws_tx, mut ws_rx) = socket.split();

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
    .inspect_err(|e| error!("Failed to publish log session ready: {e}"))
    .ok();

    let mut batch: Vec<String> = Vec::new();
    let mut flush = Box::pin(tokio::time::sleep(BATCH_WINDOW));

    loop {
        tokio::select! {
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        batch.push(text);
                        if batch.len() >= MAX_BATCH_LINES
                            && !publish_lines(&state, &session_id, &mut batch).await
                        {
                            break;
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if ws_tx.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!("Device closed log session {session_id}");
                        break;
                    }
                    Some(Err(e)) => {
                        error!("Device websocket error on log session {session_id}: {e}");
                        break;
                    }
                    Some(Ok(_)) => {}
                }
            }
            _ = &mut flush => {
                if !publish_lines(&state, &session_id, &mut batch).await {
                    break;
                }
                flush = Box::pin(tokio::time::sleep(BATCH_WINDOW));
            }
        }
    }

    publish_lines(&state, &session_id, &mut batch).await;

    // Device-initiated teardown: without this the dashboard side would sit
    // blocked on its subscription until the browser gives up or the stale
    // session sweeper fires. Harmless when the dashboard closed first — the
    // session is already closed and the frame just ages out with the backlog.
    relay::publish(
        &state.pg_pool,
        &session_id,
        Direction::ToDashboard,
        &json!({"type": "error", "message": "Device disconnected"}),
    )
    .await
    .inspect_err(|e| error!("Failed to publish device disconnect for session {session_id}: {e}"))
    .ok();

    relay::close_session(&state.pg_pool, &session_id).await;

    info!("Device log stream ended for session {session_id}");
}

/// Relay whatever has accumulated as one frame. Returns false if the relay
/// itself failed, which means the session is no longer usable.
async fn publish_lines(state: &State, session_id: &Uuid, batch: &mut Vec<String>) -> bool {
    if batch.is_empty() {
        return true;
    }

    let payload = json!({"type": "lines", "lines": std::mem::take(batch)});
    relay::publish(&state.pg_pool, session_id, Direction::ToDashboard, &payload)
        .await
        .inspect_err(|e| error!("Failed to relay log lines for session {session_id}: {e}"))
        .is_ok()
}

async fn send_json(
    ws_tx: &mut futures::stream::SplitSink<WebSocket, Message>,
    payload: &Value,
) -> bool {
    ws_tx.send(Message::Text(payload.to_string())).await.is_ok()
}
