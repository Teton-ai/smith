use crate::State;
use crate::handlers::AuthedDevice;
use axum::http::StatusCode;
use axum::http::header::CACHE_CONTROL;
use axum::response::{IntoResponse, Response};
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
        ("auth_token" = [])
    ),
    tag = AUTH_TAG
)]
#[deprecated(since = "0.2.66", note = "Since /device have been released")]
pub async fn verify_token(
    Extension(state): Extension<State>,
    Json(body): Json<DeviceTokenForVerification>,
) -> axum::response::Result<Json<DeviceAuthResponse>, StatusCode> {
    let device = sqlx::query!(
        "SELECT serial_number, approved FROM device WHERE token IS NOT NULL AND token = $1",
        body.token
    )
    .fetch_optional(&state.pg_pool)
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

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeviceSessionResponse {
    /// Signed JWT to use as the bearer for subsequent requests.
    pub token: String,
    /// Lifetime in seconds. Clients should refresh well before this elapses.
    pub expires_in: u64,
    pub token_type: &'static str,
}

#[utoipa::path(
    get,
    path = "/auth/session",
    responses(
        (status = StatusCode::OK, description = "Mint a short-lived device JWT", body = DeviceSessionResponse),
        (status = StatusCode::UNAUTHORIZED, description = "Caller is not an authenticated device"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to mint JWT"),
    ),
    security(
        ("device_token" = [])
    ),
    tag = AUTH_TAG
)]
pub async fn session(
    Extension(state): Extension<State>,
    device: AuthedDevice,
) -> Result<Json<DeviceSessionResponse>, StatusCode> {
    let token = state
        .device_jwt_signer
        .mint(device.id, &device.serial_number)
        .map_err(|err| {
            error!(
                "Failed to mint device JWT for device {}: {err:?}",
                device.id
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(DeviceSessionResponse {
        token,
        expires_in: state.device_jwt_signer.ttl_seconds(),
        token_type: "Bearer",
    }))
}

/// `/.well-known/jwks.json` — public Ed25519 verification key for device JWTs.
/// Consumed by the TS api (and any other verifier) for offline signature checks.
pub async fn jwks_well_known(Extension(state): Extension<State>) -> Response {
    let body = state.device_jwt_signer.jwks();
    ([(CACHE_CONTROL, "public, max-age=3600")], Json(body)).into_response()
}
