use crate::State;
use crate::deployment::{
    Deployment, DeploymentDeviceWithStatus, DeviceServiceHealth, FinishNowResult, RollbackRequest,
    RollbackResult, RolloutProgress, StaggerPolicy, confirm_full_rollout, finish_now,
    get_deployment, get_deployment_service_health, get_devices_in_deployment, new_deployment,
    rollback_release, rollout_progress, stagger_policy,
};
use crate::error::ApiError;
use crate::user::CurrentUser;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::{Extension, Json};
use models::deployment::DeploymentRequest;
use serde::Deserialize;
use utoipa::IntoParams;

const TAG: &str = "deployment";

#[utoipa::path(
    get,
    path = "/releases/{release_id}/deployment",
    params(
        ("release_id" = i32, Path),
    ),
    responses(
        (status = StatusCode::OK, body = Deployment),
    ),
    security(
      ("auth_token" = [])
    ),
    tag = TAG
)]
pub async fn api_get_release_deployment(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<(StatusCode, Json<Deployment>), StatusCode> {
    let release = get_deployment(release_id, &state.pg_pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(release) = release {
        return Ok((StatusCode::OK, Json(release)));
    }
    Err(StatusCode::NOT_FOUND)
}

#[utoipa::path(
  post,
  path = "/releases/{release_id}/deployment",
    params(
        ("release_id" = i32, Path),
    ),
  request_body = DeploymentRequest,
  responses(
        (status = StatusCode::OK, body = Deployment),
        (status = StatusCode::BAD_REQUEST, description = "Release is draft or yanked, a deployment is already in progress, or no canary devices matched"),
        (status = StatusCode::NOT_FOUND, description = "Release not found"),
  ),
  security(
      ("auth_token" = [])
  ),
  tag = TAG
)]
pub async fn api_release_deployment(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    request: Option<Json<DeploymentRequest>>,
) -> Result<Json<Deployment>, ApiError> {
    let user_email = current_user_email(&state, &current_user).await;

    let release = new_deployment(
        release_id,
        request.map(|req| req.0),
        &state.pg_pool,
        state.config,
        user_email.as_deref(),
    )
    .await
    .inspect_err(|e| {
        tracing::error!("Failed to create new deployment: {e:?}");
    })?;
    Ok(Json(release))
}

#[utoipa::path(
  get,
  path = "/releases/{release_id}/deployment/devices",
    params(
        ("release_id" = i32, Path),
    ),
  responses(
        (status = StatusCode::OK, body = Vec<DeploymentDeviceWithStatus>),
  ),
  security(
      ("auth_token" = [])
  ),
  tag = TAG
)]
pub async fn api_get_deployment_devices(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<(StatusCode, Json<Vec<DeploymentDeviceWithStatus>>), StatusCode> {
    let devices = get_devices_in_deployment(release_id, &state.pg_pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::OK, Json(devices)))
}

#[utoipa::path(
  post,
  path = "/releases/{release_id}/deployment/confirm",
    params(
        ("release_id" = i32, Path),
    ),
  responses(
        (status = StatusCode::OK, body = Deployment),
        (status = StatusCode::BAD_REQUEST, description = "Canary devices have not finished updating or are unhealthy, or the release is a release candidate, yanked, or its deployment was canceled"),
        (status = StatusCode::NOT_FOUND, description = "Deployment not found"),
  ),
  security(
      ("auth_token" = [])
  ),
  tag = TAG
)]
pub async fn api_confirm_full_rollout(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<(StatusCode, Json<Deployment>), ApiError> {
    let user_email = current_user_email(&state, &current_user).await;

    let deployment = confirm_full_rollout(
        release_id,
        &state.pg_pool,
        state.config,
        user_email.as_deref(),
    )
    .await
    .inspect_err(|e| {
        tracing::error!("Failed to confirm full rollout for release {release_id}: {e:?}");
    })?;
    Ok((StatusCode::OK, Json(deployment)))
}

/// Immediately move every device targeting a yanked release to another stable
/// release in the same distribution, skipping the canary phase. Any in-progress
/// deployment of the yanked release is canceled, and the distribution's latest
/// release is repointed if it was the yanked one.
#[utoipa::path(
  post,
  path = "/releases/{release_id}/rollback",
    params(
        ("release_id" = i32, Path, description = "The yanked release to move devices off"),
    ),
  request_body = RollbackRequest,
  responses(
        (status = StatusCode::OK, body = RollbackResult),
        (status = StatusCode::BAD_REQUEST, description = "Release is not yanked, or the target is not a stable release in the same distribution"),
        (status = StatusCode::NOT_FOUND, description = "Release not found"),
  ),
  security(
      ("auth_token" = [])
  ),
  tag = TAG
)]
pub async fn api_rollback_release(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Json(request): Json<RollbackRequest>,
) -> Result<Json<RollbackResult>, ApiError> {
    let user_email = current_user_email(&state, &current_user).await;

    let result = rollback_release(
        release_id,
        request.target_release_id,
        &state.pg_pool,
        state.config,
        user_email.as_deref(),
    )
    .await
    .inspect_err(|e| {
        tracing::error!("Failed to roll back release {release_id}: {e:?}");
    })?;
    Ok(Json(result))
}

#[derive(Deserialize, Debug, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct StaggerPolicyParams {
    /// Devices a Deploy to Selected would target; sets `staggered` and
    /// `window_minutes` in the response.
    device_count: Option<i32>,
}

/// How staggered rollouts are paced: full rollouts and Deploy to Selected above
/// `bulk_threshold` devices are split into up to `max_buckets` buckets, opening
/// `interval_minutes` apart.
#[utoipa::path(
  get,
  path = "/deployments/stagger-policy",
  params(
        StaggerPolicyParams
  ),
  responses(
        (status = StatusCode::OK, body = StaggerPolicy),
  ),
  security(
      ("auth_token" = [])
  ),
  tag = TAG
)]
pub async fn api_get_stagger_policy(
    Query(params): Query<StaggerPolicyParams>,
) -> Json<StaggerPolicy> {
    let device_count = params
        .device_count
        .and_then(|count| usize::try_from(count).ok())
        .unwrap_or(0);
    Json(stagger_policy(device_count))
}

/// Let every device still waiting for its bucket move to the release now.
/// Emergency use only: a site whose devices all download together can lose
/// connectivity.
#[utoipa::path(
  post,
  path = "/releases/{release_id}/deployment/finish-now",
    params(
        ("release_id" = i32, Path),
    ),
  responses(
        (status = StatusCode::OK, body = FinishNowResult),
  ),
  security(
      ("auth_token" = [])
  ),
  tag = TAG
)]
pub async fn api_finish_now(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<Json<FinishNowResult>, ApiError> {
    let user_email = current_user_email(&state, &current_user).await;

    let result = finish_now(
        release_id,
        &state.pg_pool,
        state.config,
        user_email.as_deref(),
    )
    .await
    .inspect_err(|e| {
        tracing::error!("Failed to finish the rollout of release {release_id} now: {e:?}");
    })?;
    Ok(Json(result))
}

/// Progress of the staggered rollout of a release, over every device currently
/// targeting it.
#[utoipa::path(
  get,
  path = "/releases/{release_id}/deployment/rollout-progress",
    params(
        ("release_id" = i32, Path),
    ),
  responses(
        (status = StatusCode::OK, body = RolloutProgress),
  ),
  security(
      ("auth_token" = [])
  ),
  tag = TAG
)]
pub async fn api_get_rollout_progress(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<RolloutProgress>, ApiError> {
    let progress = rollout_progress(release_id, &state.pg_pool)
        .await
        .inspect_err(|e| {
            tracing::error!("Failed to get rollout progress for release {release_id}: {e:?}");
        })?;
    Ok(Json(progress))
}

async fn current_user_email(state: &State, current_user: &CurrentUser) -> Option<String> {
    // Only used to attribute Slack notifications, so a lookup failure must not
    // block the operation itself.
    sqlx::query_scalar!(
        "SELECT email FROM auth.users WHERE id = $1",
        current_user.user_id
    )
    .fetch_optional(&state.pg_pool)
    .await
    .inspect_err(|e| tracing::error!("Failed to look up user email: {e}"))
    .ok()
    .flatten()
    .flatten()
}

#[utoipa::path(
    get,
    path = "/releases/{release_id}/deployment/service-health",
    params(
        ("release_id" = i32, Path),
    ),
    responses(
        (status = StatusCode::OK, body = Vec<DeviceServiceHealth>),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TAG
)]
pub async fn api_get_deployment_service_health(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<(StatusCode, Json<Vec<DeviceServiceHealth>>), StatusCode> {
    let health = get_deployment_service_health(release_id, &state.pg_pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::OK, Json(health)))
}
