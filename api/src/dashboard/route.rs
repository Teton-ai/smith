use crate::State;
use crate::dashboard::{Dashboard, RegistrationCount};
use axum::http::StatusCode;
use axum::{Extension, Json};

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
