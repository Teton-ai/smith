use crate::State;
use crate::device::Device;
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use tracing::error;

const AUTH_TAG: &str = "auth";

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DeviceTokenForVerification {
    pub token: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeviceAuthResponse {
    pub serial_number: String,
    pub authorized: bool,
}

#[utoipa::path(
    post,
    path = "/auth/token",
    responses(
        (status = StatusCode::OK, description = "Return found device auth", body = DeviceAuthResponse),
        (status = StatusCode::UNAUTHORIZED, description = "Failed to verify token"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve device auth"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = AUTH_TAG
)]
#[tracing::instrument]
#[deprecated(since = "0.2.66", note = "Since /device have been released")]
pub async fn verify_token(
    Extension(state): Extension<State>,
    Json(body): Json<DeviceTokenForVerification>,
) -> axum::response::Result<Json<DeviceAuthResponse>, StatusCode> {
    let device = Device::get_device_from_token(body.token, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get device {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    Ok(Json(DeviceAuthResponse {
        serial_number: device.serial_number,
        authorized: device.approved,
    }))
}
