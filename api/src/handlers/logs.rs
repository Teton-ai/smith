use axum::{extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Path, Query}, response::Response, Extension};
use futures_util::StreamExt;
use serde::Deserialize;
use sqlx::Row;
use sqlx::types::chrono;
use tokio_tungstenite::{connect_async, tungstenite::Message as TungsteniteMessage};
use tracing::{error, info, warn};
use crate::State;

const LOGS_TAG: &str = "logs";

#[derive(Deserialize)]
pub struct LogsQuery {
    pub service: Option<String>,
}

#[utoipa::path(
    get,
    path = "/devices/{serial_number}/logs",
    responses(
        (status = StatusCode::OK, description = "WebSocket connection established for log streaming"),
        (status = StatusCode::NOT_FOUND, description = "Device not found or not online"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to establish log stream"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = LOGS_TAG
)]
pub async fn stream_device_logs(
    ws: WebSocketUpgrade,
    Extension(state): Extension<State>,
    Path(serial_number): Path<String>,
    Query(params): Query<LogsQuery>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state, serial_number, params.service))
}

async fn handle_socket(
    socket: WebSocket,
    state: State,
    serial_number: String,
    service: Option<String>,
) {
    let device = match get_device_and_check_online(&state, &serial_number).await {
        Ok(device) => device,
        Err(e) => {
            error!("Failed to get device: {}", e);
            return;
        }
    };

    let port = match send_log_stream_command(&state, device.id, service.clone()).await {
        Ok(port) => port,
        Err(e) => {
            error!("Failed to send log stream command: {}", e);
            return;
        }
    };

    if let Err(e) = proxy_logs(socket, port, device.token).await {
        error!("Error proxying logs: {}", e);
    }
}

struct DeviceInfo {
    id: i32,
    token: String,
}

async fn get_device_and_check_online(
    state: &State,
    serial_number: &str,
) -> Result<DeviceInfo, String> {
    let device = sqlx::query(
        "SELECT id, token, last_ping FROM device
         WHERE serial_number = $1 AND token IS NOT NULL"
    )
    .bind(serial_number)
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|e| format!("Database error: {}", e))?
    .ok_or_else(|| "Device not found or not registered".to_string())?;

    let id: i32 = device.try_get("id").map_err(|e| format!("Failed to get device id: {}", e))?;
    let token: String = device.try_get("token").map_err(|e| format!("Failed to get device token: {}", e))?;
    let last_ping: Option<chrono::DateTime<chrono::Utc>> = device.try_get("last_ping").ok();

    if let Some(last_ping) = last_ping {
        let now = chrono::Utc::now();
        let diff = (now - last_ping).num_minutes();
        if diff > 5 {
            return Err("Device is offline".to_string());
        }
    } else {
        return Err("Device never pinged".to_string());
    }

    Ok(DeviceInfo { id, token })
}

async fn send_log_stream_command(
    state: &State,
    device_id: i32,
    service: Option<String>,
) -> Result<u16, String> {
    let bundle_id = sqlx::query!("INSERT INTO command_bundles DEFAULT VALUES RETURNING uuid")
        .fetch_one(&state.pg_pool)
        .await
        .map_err(|e| format!("Failed to create command bundle: {}", e))?;

    let command = smith::utils::schema::SafeCommandTx::StartLogStream { service: service.clone() };

    let cmd_id = sqlx::query!(
        "INSERT INTO command_queue (device_id, cmd, continue_on_error, canceled, bundle)
         VALUES ($1, $2::jsonb, $3, false, $4) RETURNING id",
        device_id,
        serde_json::to_value(command).expect("Failed to serialize command"),
        false,
        bundle_id.uuid
    )
    .fetch_one(&state.pg_pool)
    .await
    .map_err(|e| format!("Failed to insert command: {}", e))?;

    info!("Sent StartLogStream command {} for device {} with service {:?}", cmd_id.id, device_id, service);

    // Wait up to 60 seconds for device response
    for i in 0..60 {
        if i % 10 == 0 {
            info!("Still waiting for log stream response from device {} ({}s elapsed)", device_id, i);
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        let response = sqlx::query!(
            "SELECT cr.response
             FROM command_response cr
             JOIN command_queue cq ON cq.id = cr.command_id
             WHERE cq.device_id = $1 AND cq.bundle = $2
             ORDER BY cr.created_at DESC
             LIMIT 1",
            device_id,
            bundle_id.uuid
        )
        .fetch_optional(&state.pg_pool)
        .await
        .map_err(|e| format!("Failed to check command response: {}", e))?;

        if let Some(row) = response {
            let response: smith::utils::schema::SafeCommandRx =
                serde_json::from_value(row.response)
                    .map_err(|e| format!("Failed to parse response: {}", e))?;

            if let smith::utils::schema::SafeCommandRx::StartLogStream { port } = response {
                if port > 0 {
                    info!("Device {} returned log stream port {}", device_id, port);
                    return Ok(port);
                } else {
                    error!("Device {} returned invalid port 0", device_id);
                    return Err("Device returned invalid port".to_string());
                }
            } else {
                warn!("Device {} returned unexpected response type: {:?}", device_id, response);
            }
        }
    }

    error!("Timeout waiting for log stream to start from device {} after 60s", device_id);
    Err("Timeout waiting for log stream to start".to_string())
}

async fn proxy_logs(
    mut client_ws: WebSocket,
    device_port: u16,
    _device_token: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("ws://127.0.0.1:{}", device_port);

    let (device_ws, _) = connect_async(&url).await?;
    let (device_write, mut device_read) = device_ws.split();

    loop {
        tokio::select! {
            msg = device_read.next() => {
                match msg {
                    Some(Ok(TungsteniteMessage::Text(text))) => {
                        if client_ws.send(Message::Text(text)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(TungsteniteMessage::Close(_))) | None => break,
                    Some(Err(e)) => {
                        error!("Device WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
            msg = client_ws.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => {
                        error!("Client WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
