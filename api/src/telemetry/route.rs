use crate::State;
use crate::handlers::AuthedDevice;
use crate::modem::{clear_modem, save_modem};
use axum::body::{Body, to_bytes};
use axum::extract::Request;
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::Deserialize;
use tracing::error;
use utoipa::ToSchema;

#[utoipa::path(
    post,
    path = "/smith/telemetry/victoria",
    responses(
        (status = 200, description = "Victoria metrics data forwarded successfully"),
        (status = 501, description = "Victoria metrics not implemented"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("device_token" = [])
    ),
)]
pub async fn victoria(
    device: AuthedDevice,
    Extension(state): Extension<State>,
    req: Request<Body>,
) -> Result<StatusCode, StatusCode> {
    let clients = &state.config.victoria_metrics_clients;
    let Some((primary, secondaries)) = clients.split_first() else {
        return Err(StatusCode::NOT_IMPLEMENTED);
    };

    let (parts, body) = req.into_parts();
    let method = parts.method;
    let mut headers = parts.headers;

    headers.remove("authorization");
    let body_bytes = to_bytes(body, usize::MAX).await.map_err(|err| {
        error!("Failed to read body bytes: {}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    for secondary in secondaries {
        let client = secondary.client.clone();
        let url = secondary.url.clone();
        let headers = headers.clone();
        let body_bytes = body_bytes.clone();
        let method = method.clone();
        let serial_number = device.serial_number.clone();
        tokio::spawn(async move {
            if let Err(err) = client
                .request(method, &url)
                .headers(headers)
                .body(body_bytes)
                .send()
                .await
            {
                error!(
                    error = %err,
                    serial_number,
                    "Failed to forward telemetry to secondary VictoriaMetrics target"
                );
            }
        });
    }

    let response = primary
        .client
        .request(method, &primary.url)
        .headers(headers)
        .body(body_bytes)
        .send()
        .await;

    match response {
        Ok(res) => Ok(res.status()),
        Err(err) => {
            error!(error = %err, "Failed to forward request to VictoriaMetrics");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct NewModem {
    pub imei: String,
    pub network_provider: String,
}

#[utoipa::path(
    post,
    path = "/smith/telemetry/modem",
    responses(
        (status = 200, description = "Modem telemetry data processed successfully"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("device_token" = [])
    ),
)]
pub async fn modem(
    device: AuthedDevice,
    Extension(state): Extension<State>,
    Json(modem): Json<Option<NewModem>>,
) -> Result<StatusCode, StatusCode> {
    tokio::spawn(async move {
        match modem {
            Some(modem) => {
                let _ = save_modem(
                    device.serial_number,
                    modem.imei,
                    modem.network_provider,
                    &state.pg_pool,
                )
                .await;
            }
            None => {
                let _ = clear_modem(device.serial_number, &state.pg_pool).await;
            }
        }
    });
    Ok(StatusCode::OK)
}
