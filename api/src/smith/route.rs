use crate::State;
use crate::db::{DBHandler, DeviceWithToken};
use crate::device::{Device, RegistrationError};
use crate::handlers::ip_address::IpAddressInfo;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, StatusCode};
use axum::{Extension, Json};
use smith::utils::schema::{
    DeviceRegistration, DeviceRegistrationResponse, HomePost, HomePostResponse,
};
use std::net::SocketAddr;
use std::time::SystemTime;
use tracing::{error, info};

#[utoipa::path(
  post,
  path = "/smith/register",
  responses(
        (status = 200, description = "Device registration successful"),
        (status = 403, description = "Device not approved"),
        (status = 409, description = "Device already has token"),
        (status = 500, description = "Internal server error")
  )
)]
#[tracing::instrument]
pub async fn register_device(
    Extension(state): Extension<State>,
    Json(payload): Json<DeviceRegistration>,
) -> (StatusCode, Json<DeviceRegistrationResponse>) {
    let token = Device::register_device(payload, &state.pg_pool, state.config).await;

    match token {
        Ok(token) => (StatusCode::OK, Json(token)),
        Err(e) => {
            info!("No token available for device: {:?}", e);
            let status_code = match e {
                RegistrationError::NotNullTokenError => StatusCode::CONFLICT,
                RegistrationError::NotApprovedDevice => StatusCode::FORBIDDEN,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };

            (status_code, Json(DeviceRegistrationResponse::default()))
        }
    }
}

#[utoipa::path(
  post,
  path = "/smith/home",
  responses(
        (status = 200, description = "Device home response")
  ),
  security(("Access Token" = []))
)]
#[tracing::instrument]
pub async fn home(
    headers: HeaderMap,
    device: DeviceWithToken,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Extension(state): Extension<State>,
    Json(payload): Json<HomePost>,
) -> (StatusCode, Json<HomePostResponse>) {
    let release_id = payload.release_id;
    DBHandler::save_responses(&device, payload, &state.pg_pool)
        .await
        .unwrap_or_else(|err| {
            error!("Error saving responses: {:?}", err);
        });

    let response = HomePostResponse {
        timestamp: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default(),
        commands: DBHandler::get_commands(&device, &state.pg_pool).await,
        target_release_id: Device::get_target_release(&device, &state.pg_pool).await,
    };

    let client_ip = Some(IpAddressInfo::extract_client_ip(&headers, addr));
    tokio::spawn(async move {
        Device::save_release_id(&device, release_id, &state.pg_pool)
            .await
            .unwrap_or_else(|err| {
                error!("Error saving release_id: {:?}", err);
            });
        Device::save_last_ping_with_ip(&device, client_ip, &state.pg_pool, state.config)
            .await
            .unwrap_or_else(|err| {
                error!("Error saving last ping with IP: {:?}", err);
            });
    });

    (StatusCode::OK, Json(response))
}
