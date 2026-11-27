use crate::State;
use crate::dashboard::{Dashboard, RegistrationAccess, RegistrationCount, UnhealthyServiceDevice};
use crate::ip_address::registration_client_ip;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, StatusCode};
use axum::{Extension, Json};
use std::net::SocketAddr;

#[utoipa::path(
  get,
  path = "/dashboard",
  responses(
        (status = StatusCode::OK, description = "Dashboard metrics", body = Dashboard),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve dashboard content"),
  )
)]
pub async fn get_dashboard(
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Dashboard>, StatusCode> {
    let dashboard = Dashboard::new(&state.pg_pool, &state.config.dashboard_excluded_labels).await;
    Ok(Json(dashboard))
}

#[utoipa::path(
  get,
  path = "/dashboard/registrations",
  responses(
        (status = StatusCode::OK, description = "Daily device registration counts", body = Vec<RegistrationCount>),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve registration counts"),
  )
)]
pub async fn get_registration_counts(
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Vec<RegistrationCount>>, StatusCode> {
    let counts = crate::dashboard::get_registration_counts(
        &state.pg_pool,
        &state.config.dashboard_excluded_labels,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to get registration counts: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(counts))
}

#[utoipa::path(
  get,
  path = "/dashboard/unhealthy-services",
  responses(
        (status = StatusCode::OK, description = "Devices with unhealthy services", body = Vec<UnhealthyServiceDevice>),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve unhealthy services"),
  )
)]
pub async fn get_unhealthy_services(
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Vec<UnhealthyServiceDevice>>, StatusCode> {
    let devices = crate::dashboard::get_unhealthy_service_devices(
        &state.pg_pool,
        &state.config.dashboard_excluded_labels,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to get unhealthy service devices: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(devices))
}

#[utoipa::path(
  get,
  path = "/dashboard/registration-access",
  responses(
        (status = StatusCode::OK, description = "Device registration IP allowlist and whether the caller's IP is on it", body = RegistrationAccess),
  )
)]
pub async fn get_registration_access(
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Extension(state): Extension<State>,
) -> Json<RegistrationAccess> {
    let allowlist = &state.config.registration_ip_allowlist;
    let client_ip = registration_client_ip(&headers, addr);
    let allowed_ips = allowlist.ips().map(|ips| {
        let mut ips: Vec<String> = ips.iter().map(ToString::to_string).collect();
        ips.sort();
        ips
    });
    Json(RegistrationAccess {
        allowed_ips,
        client_ip: client_ip.map(|ip| ip.to_string()),
        client_allowed: allowlist.allows(client_ip),
    })
}
