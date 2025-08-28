use crate::State;
use crate::db::{DBHandler, DeviceWithToken};
use crate::device::RegistrationError;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, StatusCode};
use axum::{Extension, Json};
use smith::utils::schema::{
    DeviceRegistration, DeviceRegistrationResponse, HomePost, HomePostResponse,
};
use std::net::{IpAddr, SocketAddr};
use std::time::SystemTime;
use tracing::{debug, error, info};

fn extract_client_ip(headers: &HeaderMap, fallback_addr: SocketAddr) -> IpAddr {
    // Check X-Forwarded-For header first (load balancer/proxy)
    if let Some(forwarded_for) = headers.get("x-forwarded-for") {
        if let Ok(forwarded_str) = forwarded_for.to_str() {
            // X-Forwarded-For can contain multiple IPs, take the first (original client)
            if let Some(first_ip) = forwarded_str.split(',').next() {
                if let Ok(ip) = first_ip.trim().parse::<IpAddr>() {
                    return ip;
                }
            }
        }
    }

    // Check X-Real-IP header (alternative proxy header)
    if let Some(real_ip) = headers.get("x-real-ip") {
        if let Ok(real_ip_str) = real_ip.to_str() {
            if let Ok(ip) = real_ip_str.parse::<IpAddr>() {
                return ip;
            }
        }
    }

    // Fall back to direct connection IP
    fallback_addr.ip()
}

#[tracing::instrument]
pub async fn home(
    headers: HeaderMap,
    device: DeviceWithToken,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Extension(state): Extension<State>,
    Json(payload): Json<HomePost>,
) -> (StatusCode, Json<HomePostResponse>) {
    debug!(
        "Received payload {:?} from {}",
        payload, device.serial_number
    );

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
        target_release_id: crate::device::Device::get_target_release(&device, &state.pg_pool).await,
    };

    let client_ip = Some(extract_client_ip(&headers, addr));
    tokio::spawn(async move {
        crate::device::Device::save_release_id(&device, release_id, &state.pg_pool)
            .await
            .unwrap_or_else(|err| {
                error!("Error saving release_id: {:?}", err);
            });
        crate::device::Device::save_last_ping_with_ip(&device, client_ip, &state.pg_pool)
            .await
            .unwrap_or_else(|err| {
                error!("Error saving last ping with IP: {:?}", err);
            });
    });

    (StatusCode::OK, Json(response))
}

#[tracing::instrument]
pub async fn register_device(
    Extension(state): Extension<State>,
    Json(payload): Json<DeviceRegistration>,
) -> (StatusCode, Json<DeviceRegistrationResponse>) {
    debug!("Registering device {:?}", payload);

    let token = crate::device::Device::register_device(payload, &state.pg_pool, state.config).await;

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
