use crate::State;
use crate::rollout::DistributionRolloutStats;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Extension, Json};

const TAG: &str = "rollout";

#[utoipa::path(
  get,
  path = "/distributions/{distribution_id}/rollout",
  responses(
        (status = StatusCode::OK, body = DistributionRolloutStats),
  ),
  security(
      ("Access Token" = [])
  ),
  tag = TAG
)]
pub async fn api_rollout(
    Path(distribution_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<(StatusCode, Json<DistributionRolloutStats>), StatusCode> {
    let distribution_rollout_stats = DistributionRolloutStats::get(distribution_id, &state.pg_pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::OK, Json(distribution_rollout_stats)))
}
