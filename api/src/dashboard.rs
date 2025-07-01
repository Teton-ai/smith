use axum::{Extension, Json};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;
use crate::State;

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Dashboard {
  pub total_count: u32,
  pub online_count: u32,
  pub offline_count: u32,
  pub outdated_count: u32,
  pub archived_count: u32
}

impl Dashboard {
  pub async fn new(pool: &PgPool) -> Self {
    Self {
      total_count: 0,
      online_count: 0,
      offline_count: 0,
      outdated_count: 0,
      archived_count: 0,
    }
  }
}

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
