use crate::State;
use crate::rollout::{
    DistributionRolloutStats, get_distribution_rollout_stats, get_distributions_rollout_stats,
};
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Extension, Json};

const TAG: &str = "rollout";

#[utoipa::path(
  get,
  path = "/distributions/{distribution_id}/rollout",
    params(
        ("distribution_id" = i32, Path),
    ),
  responses(
        (status = StatusCode::OK, body = DistributionRolloutStats),
  ),
  security(
      ("auth_token" = [])
  ),
  tag = TAG
)]
pub async fn api_rollout(
    Path(distribution_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<(StatusCode, Json<DistributionRolloutStats>), StatusCode> {
    let distribution_rollout_stats =
        get_distribution_rollout_stats(distribution_id, &state.pg_pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::OK, Json(distribution_rollout_stats)))
}

#[utoipa::path(
  get,
  path = "/distributions/rollout",
  responses(
        (status = StatusCode::OK, body = Vec<DistributionRolloutStats>),
  ),
  security(
      ("auth_token" = [])
  ),
  tag = TAG
)]
pub async fn get_distribution_rollouts(
    Extension(state): Extension<State>,
) -> Result<(StatusCode, Json<Vec<DistributionRolloutStats>>), StatusCode> {
    let distribution_rollout_stats = get_distributions_rollout_stats(&state.pg_pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::OK, Json(distribution_rollout_stats)))
}
