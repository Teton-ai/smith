use super::session::LogStreamSessions;
use crate::State;
use crate::home::add_commands;
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
use smith::utils::schema::{SafeCommandRequest, SafeCommandTx};
use tokio::sync::mpsc;
use tracing::{error, info};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct WsAuthQuery {
    token: String,
}

const LOGSTREAM_TAG: &str = "logstream";

/// WebSocket endpoint for dashboard to receive log stream
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
    Extension(sessions): Extension<LogStreamSessions>,
) -> Result<Response, StatusCode> {
    // Validate the JWT token
    state
        .jwks_client
        .decode::<serde_json::Value>(&auth.token, &[&state.config.auth0_audience])
        .await
        .map_err(|e| {
            error!("Token validation failed: {}", e);
            StatusCode::UNAUTHORIZED
        })?;

    let _device = sqlx::query!(
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

    let session_id = Uuid::new_v4().to_string();
    let ws_url = format!(
        "{}/ws/stream-logs/{}",
        state.config.api_public_url.replace("http", "ws"),
        session_id
    );

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
            ws_url,
            state,
            sessions,
        )
    }))
}

async fn handle_dashboard_ws(
    socket: WebSocket,
    session_id: String,
    device_serial: String,
    service_name: String,
    ws_url: String,
    state: State,
    sessions: LogStreamSessions,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (log_tx, mut log_rx) = mpsc::channel::<String>(100);

    sessions
        .create_session(
            session_id.clone(),
            device_serial.clone(),
            service_name.clone(),
            log_tx,
        )
        .await;

    let command = SafeCommandRequest {
        id: -10,
        command: SafeCommandTx::StreamLogs {
            session_id: session_id.clone(),
            service_name: service_name.clone(),
            ws_url,
        },
        continue_on_error: false,
    };

    if let Err(e) = add_commands(&device_serial, vec![command], &state.pg_pool).await {
        error!("Failed to queue StreamLogs command: {}", e);
        sessions.remove_session(&session_id).await;
        return;
    }

    info!("Queued StreamLogs command for session {}", session_id);

    let session_id_clone = session_id.clone();
    let sessions_clone = sessions.clone();

    let forward_task = tokio::spawn(async move {
        while let Some(log_line) = log_rx.recv().await {
            if ws_tx.send(Message::Text(log_line)).await.is_err() {
                break;
            }
        }
    });

    while let Some(msg) = ws_rx.next().await {
        match msg {
            Ok(Message::Close(_)) => {
                info!(
                    "Dashboard closed WebSocket for session {}",
                    session_id_clone
                );
                break;
            }
            Ok(Message::Ping(_)) => {
                if forward_task.is_finished() {
                    break;
                }
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    let stop_command = SafeCommandRequest {
        id: -11,
        command: SafeCommandTx::StopLogStream {
            session_id: session_id_clone.clone(),
        },
        continue_on_error: false,
    };
    let _ = add_commands(&device_serial, vec![stop_command], &state.pg_pool).await;

    sessions_clone.remove_session(&session_id_clone).await;
    forward_task.abort();

    info!(
        "Dashboard log stream ended for session {}",
        session_id_clone
    );
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
        (status = StatusCode::NOT_FOUND, description = "Session not found"),
        (status = StatusCode::FORBIDDEN, description = "Device not authorized for this session"),
    ),
    security(
        ("device_token" = [])
    ),
    tag = LOGSTREAM_TAG
)]
pub async fn device_logs_ws(
    ws: WebSocketUpgrade,
    Path(session_id): Path<String>,
    Extension(sessions): Extension<LogStreamSessions>,
) -> Result<Response, StatusCode> {
    let tx = sessions
        .get_session_tx(&session_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    info!("Device connecting to log stream session {}", session_id);

    Ok(ws.on_upgrade(move |socket| handle_device_ws(socket, session_id, tx)))
}

async fn handle_device_ws(
    socket: WebSocket,
    session_id: String,
    dashboard_tx: mpsc::Sender<String>,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    info!("Device connected to log stream session {}", session_id);

    while let Some(msg) = ws_rx.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if dashboard_tx.send(text).await.is_err() {
                    info!("Dashboard disconnected, stopping device stream");
                    break;
                }
            }
            Ok(Message::Close(_)) => {
                info!("Device closed WebSocket for session {}", session_id);
                break;
            }
            Ok(Message::Ping(data)) => {
                if ws_tx.send(Message::Pong(data)).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                error!("Device WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    info!("Device log stream ended for session {}", session_id);
}
