use crate::State;
use crate::auth::{DeviceAuth, DeviceTokenForVerification};
use axum::http::StatusCode;
use axum::{Extension, Json};
use tracing::error;

const AUTH_TAG: &str = "auth";

#[utoipa::path(
    post,
    path = "/auth/token",
    responses(
        (status = StatusCode::OK, description = "Return found device auth", body = DeviceAuth),
        (status = StatusCode::UNAUTHORIZED, description = "Failed to verify token"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve device auth"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = AUTH_TAG
)]
#[tracing::instrument]
pub async fn verify_token(
    Extension(state): Extension<State>,
    Json(token): Json<DeviceTokenForVerification>,
) -> axum::response::Result<Json<DeviceAuth>, StatusCode> {
    let device = sqlx::query_as!(
        DeviceAuth,
        "
        SELECT device.serial_number AS serial_number, device.approved AS authorized
        FROM device
        WHERE device.token = $1
        ",
        token.token
    )
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if let Some(device) = device {
        Ok(Json(device))
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
