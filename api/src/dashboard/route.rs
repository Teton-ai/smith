use crate::State;
use crate::dashboard::Dashboard;
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
pub async fn api(
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Dashboard>, StatusCode> {
    let dashboard = Dashboard::new(&state.pg_pool).await;
    Ok(Json(dashboard))
}
