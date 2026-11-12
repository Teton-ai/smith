use crate::State;
use crate::deployment::{
    Deployment, DeploymentCreator, DeploymentDetail, DeploymentDeviceFilter, DeploymentDeviceInfo,
    DeploymentDeviceWithStatus, DeploymentKind, DeploymentListFilter, DeploymentPreflight,
    DeploymentSummary, DeviceServiceHealth, FinalizeResult, PreflightRequest, RollbackRequest,
    RollbackResult, RushResult, StaggerPolicy, confirm_full_rollout, deployment_creators,
    deployment_detail, deployment_devices, finalize, get_deployment, get_deployment_service_health,
    get_devices_in_deployment, list_deployments, new_deployment, notify_control, pause, preflight,
    promote, resume, rollback_release, rush_rollout, stagger_policy, yank,
};
use crate::error::ApiError;
use crate::user::CurrentUser;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::{Extension, Json};
use models::deployment::DeploymentRequest;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

const TAG: &str = "deployment";

/// The release's most recent deployment. A release can be deployed more than
/// once; `/deployments?release_id=` lists every run.
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
        (status = StatusCode::BAD_REQUEST, description = "Release is draft or yanked, another deployment is active, or no canary devices matched"),
        (status = StatusCode::FORBIDDEN, description = "Your role does not allow this deployment"),
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
        &current_user,
        user_email.as_deref(),
    )
    .await
    .inspect_err(|e| e.log_if_unexpected("Failed to create new deployment"))?;
    Ok(Json(release))
}

/// Canary devices of the release's most recent deployment. Deprecated: use
/// `/deployments/{deployment_id}/devices`, which names the run.
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
#[deprecated = "names no deployment run; use the /deployments/{deployment_id} routes"]
pub async fn api_get_deployment_devices(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<(StatusCode, Json<Vec<DeploymentDeviceWithStatus>>), StatusCode> {
    let devices = get_devices_in_deployment(release_id, &state.pg_pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::OK, Json(devices)))
}

/// Promote the release's most recent deployment. Deprecated: use
/// `/deployments/{deployment_id}/promote`, which names the run.
#[utoipa::path(
  post,
  path = "/releases/{release_id}/deployment/confirm",
    params(
        ("release_id" = i32, Path),
    ),
  responses(
        (status = StatusCode::OK, body = Deployment),
        (status = StatusCode::BAD_REQUEST, description = "Canary devices have not finished updating or are unhealthy, the release is yanked or its deployment was canceled, or it is a release candidate deployed without a pilot group"),
        (status = StatusCode::FORBIDDEN, description = "Your role does not allow this deployment"),
        (status = StatusCode::NOT_FOUND, description = "Deployment not found"),
  ),
  security(
      ("auth_token" = [])
  ),
  tag = TAG
)]
#[deprecated = "names no deployment run; use the /deployments/{deployment_id} routes"]
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
        &current_user,
        user_email.as_deref(),
    )
    .await
    .inspect_err(|e| {
        e.log_if_unexpected(format!(
            "Failed to confirm full rollout for release {release_id}"
        ))
    })?;
    Ok((StatusCode::OK, Json(deployment)))
}

/// Yank the release if it isn't yet, and in the same transaction move every
/// device on it or heading to it to another stable release in the same
/// distribution, skipping the canary phase. Its active deployments are
/// canceled, and the distribution's latest is repointed if it was this release.
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
        (status = StatusCode::FORBIDDEN, description = "Your role does not allow this deployment"),
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
        request
            .yank_reason
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty()),
        &state.pg_pool,
        state.config,
        &current_user,
        user_email.as_deref(),
    )
    .await
    .inspect_err(|e| e.log_if_unexpected(format!("Failed to roll back release {release_id}")))?;
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

#[derive(Deserialize, Debug, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DeploymentListParams {
    distribution_id: Option<i32>,
    release_id: Option<i32>,
    /// Rollbacks that moved devices off this (yanked) release.
    rolled_back_from: Option<i32>,
    /// Started by this user.
    created_by: Option<i32>,
    kind: Option<DeploymentKind>,
    /// Only running (canary, rolling out, paused) or only finished deployments.
    active: Option<bool>,
    /// At most 200; defaults to 50.
    limit: Option<i64>,
}

/// Everyone who has started a deployment, for filtering the list by user.
#[utoipa::path(
    get,
    path = "/deployments/creators",
    responses(
        (status = StatusCode::OK, body = Vec<DeploymentCreator>),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TAG
)]
pub async fn api_list_deployment_creators(
    Extension(state): Extension<State>,
) -> Result<Json<Vec<DeploymentCreator>>, ApiError> {
    let creators = deployment_creators(&state.pg_pool)
        .await
        .inspect_err(|e| tracing::error!("Failed to list deployment creators: {e}"))?;
    Ok(Json(creators))
}

/// Deployments, newest first, with how far each one has got.
#[utoipa::path(
    get,
    path = "/deployments",
    params(DeploymentListParams),
    responses(
        (status = StatusCode::OK, body = Vec<DeploymentSummary>),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TAG
)]
pub async fn api_list_deployments(
    Query(params): Query<DeploymentListParams>,
    Extension(state): Extension<State>,
) -> Result<Json<Vec<DeploymentSummary>>, ApiError> {
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let deployments = list_deployments(
        DeploymentListFilter {
            distribution_id: params.distribution_id,
            release_id: params.release_id,
            rolled_back_from: params.rolled_back_from,
            created_by: params.created_by,
            kind: params.kind,
            active: params.active,
            limit,
        },
        &state.pg_pool,
    )
    .await
    .inspect_err(|e| tracing::error!("Failed to list deployments: {e}"))?;
    Ok(Json(deployments))
}

/// A deployment's progress wave by wave, its history, what it changes, and what
/// the signed-in user can do to it.
#[utoipa::path(
    get,
    path = "/deployments/{deployment_id}",
    params(("deployment_id" = i32, Path)),
    responses(
        (status = StatusCode::OK, body = DeploymentDetail),
        (status = StatusCode::NOT_FOUND, description = "Deployment not found"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TAG
)]
pub async fn api_get_deployment(
    Path(deployment_id): Path<i32>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<Json<DeploymentDetail>, ApiError> {
    let detail = deployment_detail(deployment_id, &current_user, &state.pg_pool)
        .await
        .inspect_err(|e| {
            e.log_if_unexpected(format!("Failed to get deployment {deployment_id}"))
        })?;
    Ok(Json(detail))
}

#[derive(Deserialize, Debug, Default, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DeploymentDeviceParams {
    /// A device state (`updated`, `updating`, `offline`, ...), or `unhealthy`.
    pub state: Option<String>,
    pub wave: Option<i32>,
    /// Part of a serial number.
    pub search: Option<String>,
    /// At most 500; defaults to 25.
    pub limit: Option<i64>,
}

/// A deployment's devices and what each is doing, first by wave then serial.
/// Capped: the wave and state totals are on the deployment itself.
#[utoipa::path(
    get,
    path = "/deployments/{deployment_id}/devices",
    params(("deployment_id" = i32, Path), DeploymentDeviceParams),
    responses(
        (status = StatusCode::OK, body = Vec<DeploymentDeviceInfo>),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TAG
)]
pub async fn api_list_deployment_devices(
    Path(deployment_id): Path<i32>,
    Query(params): Query<DeploymentDeviceParams>,
    Extension(state): Extension<State>,
) -> Result<Json<Vec<DeploymentDeviceInfo>>, ApiError> {
    let filter = DeploymentDeviceFilter {
        state: params.state,
        wave: params.wave,
        search: params.search.filter(|s| !s.trim().is_empty()),
        limit: params.limit.unwrap_or(25).clamp(1, 500),
    };
    let devices = deployment_devices(deployment_id, filter, &state.pg_pool)
        .await
        .inspect_err(|e| {
            tracing::error!("Failed to list devices of deployment {deployment_id}: {e}");
        })?;
    Ok(Json(devices))
}

/// Move a deployment from its canary to the staggered rollout: to the fleet, or
/// for a release candidate to the pilot group picked when it was deployed. A
/// pilot rollout doesn't change the distribution's latest release.
#[utoipa::path(
    post,
    path = "/deployments/{deployment_id}/promote",
    params(("deployment_id" = i32, Path)),
    responses(
        (status = StatusCode::OK, body = Deployment),
        (status = StatusCode::BAD_REQUEST, description = "Canary devices are not ready, the deployment is paused or has ended, another deployment is active, or the pilot group matches no device"),
        (status = StatusCode::FORBIDDEN, description = "Your role does not allow this deployment"),
        (status = StatusCode::NOT_FOUND, description = "Deployment not found"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TAG
)]
pub async fn api_promote_deployment(
    Path(deployment_id): Path<i32>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<Json<Deployment>, ApiError> {
    let user_email = current_user_email(&state, &current_user).await;
    let deployment = promote(
        deployment_id,
        &state.pg_pool,
        state.config,
        &current_user,
        user_email.as_deref(),
    )
    .await
    .inspect_err(|e| {
        e.log_if_unexpected(format!("Failed to promote deployment {deployment_id}"))
    })?;
    Ok(Json(deployment))
}

#[derive(Deserialize, Debug, ToSchema)]
pub struct DeploymentControlRequest {
    /// Why, for the deployment's history.
    pub reason: Option<String>,
}

/// Stop devices from moving. Needs `devices:write`, like resuming; a device
/// already downloading finishes and stays there.
#[utoipa::path(
    post,
    path = "/deployments/{deployment_id}/pause",
    params(("deployment_id" = i32, Path)),
    request_body = DeploymentControlRequest,
    responses(
        (status = StatusCode::NO_CONTENT, description = "Deployment paused"),
        (status = StatusCode::FORBIDDEN, description = "Your role does not allow deployments"),
        (status = StatusCode::BAD_REQUEST, description = "The deployment is not running"),
        (status = StatusCode::NOT_FOUND, description = "Deployment not found"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TAG
)]
pub async fn api_pause_deployment(
    Path(deployment_id): Path<i32>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Json(request): Json<DeploymentControlRequest>,
) -> Result<StatusCode, ApiError> {
    let reason = trimmed(&request.reason);
    pause(deployment_id, reason, &state.pg_pool, &current_user)
        .await
        .inspect_err(|e| {
            e.log_if_unexpected(format!("Failed to pause deployment {deployment_id}"))
        })?;
    let user_email = current_user_email(&state, &current_user).await;
    notify_control(
        ":double_vertical_bar: *Deployment Paused*",
        deployment_id,
        reason,
        &state.pg_pool,
        state.config,
        user_email.as_deref(),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Pick a paused deployment back up. Waves still to come keep their spacing.
#[utoipa::path(
    post,
    path = "/deployments/{deployment_id}/resume",
    params(("deployment_id" = i32, Path)),
    responses(
        (status = StatusCode::NO_CONTENT, description = "Deployment resumed"),
        (status = StatusCode::BAD_REQUEST, description = "The deployment is not paused"),
        (status = StatusCode::FORBIDDEN, description = "Your role does not allow this deployment"),
        (status = StatusCode::NOT_FOUND, description = "Deployment not found"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TAG
)]
pub async fn api_resume_deployment(
    Path(deployment_id): Path<i32>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<StatusCode, ApiError> {
    resume(deployment_id, &state.pg_pool, &current_user)
        .await
        .inspect_err(|e| {
            e.log_if_unexpected(format!("Failed to resume deployment {deployment_id}"))
        })?;
    let user_email = current_user_email(&state, &current_user).await;
    notify_control(
        ":arrow_forward: *Deployment Resumed*",
        deployment_id,
        None,
        &state.pg_pool,
        state.config,
        user_email.as_deref(),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Withdraw the deployment's release and move every device on it back to the
/// release the deployment started from, at once. Works on running and finished
/// deployments; this is how a rollout is stopped, so no device that follows
/// latest is left on a release that isn't latest.
#[utoipa::path(
    post,
    path = "/deployments/{deployment_id}/yank",
    params(("deployment_id" = i32, Path)),
    request_body = DeploymentControlRequest,
    responses(
        (status = StatusCode::OK, body = RollbackResult),
        (status = StatusCode::BAD_REQUEST, description = "The release is already yanked, the deployment is a rollback, or there is no earlier release to return to"),
        (status = StatusCode::FORBIDDEN, description = "Your role does not allow deployments"),
        (status = StatusCode::NOT_FOUND, description = "Deployment not found"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TAG
)]
pub async fn api_yank_deployment(
    Path(deployment_id): Path<i32>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Json(request): Json<DeploymentControlRequest>,
) -> Result<Json<RollbackResult>, ApiError> {
    let reason = trimmed(&request.reason);
    let user_email = current_user_email(&state, &current_user).await;
    let result = yank(
        deployment_id,
        reason,
        &state.pg_pool,
        state.config,
        &current_user,
        user_email.as_deref(),
    )
    .await
    .inspect_err(|e| e.log_if_unexpected(format!("Failed to yank deployment {deployment_id}")))?;
    Ok(Json(result))
}

/// Open every remaining wave at once. Emergency use only: a site whose devices
/// all download together can lose connectivity.
/// Stop waiting for devices that haven't updated: they are marked timed out and
/// the deployment completes. They keep the release as their target and still
/// update on their own once they come online. Every wave must have opened.
#[utoipa::path(
    post,
    path = "/deployments/{deployment_id}/finalize",
    params(("deployment_id" = i32, Path)),
    responses(
        (status = StatusCode::OK, body = FinalizeResult),
        (status = StatusCode::BAD_REQUEST, description = "The deployment isn't rolling out, or some waves haven't opened yet"),
        (status = StatusCode::FORBIDDEN, description = "Your role does not allow deployments"),
        (status = StatusCode::NOT_FOUND, description = "Deployment not found"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TAG
)]
pub async fn api_finalize_deployment(
    Path(deployment_id): Path<i32>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<Json<FinalizeResult>, ApiError> {
    let result = finalize(deployment_id, &state.pg_pool, &current_user)
        .await
        .inspect_err(|e| {
            e.log_if_unexpected(format!("Failed to finalize deployment {deployment_id}"))
        })?;
    let user_email = current_user_email(&state, &current_user).await;
    let detail = format!("{} devices timed out", result.devices_timed_out);
    notify_control(
        ":checkered_flag: *Deployment Finalized*",
        deployment_id,
        Some(&detail),
        &state.pg_pool,
        state.config,
        user_email.as_deref(),
    )
    .await;
    Ok(Json(result))
}

/// Rush the rollout: every wave still waiting opens, so its devices update now. For
/// emergencies: a site whose devices all download together can lose connectivity.
#[utoipa::path(
    post,
    path = "/deployments/{deployment_id}/rush",
    params(("deployment_id" = i32, Path)),
    responses(
        (status = StatusCode::OK, body = RushResult),
        (status = StatusCode::BAD_REQUEST, description = "The deployment is not rolling out"),
        (status = StatusCode::FORBIDDEN, description = "Your role does not allow this deployment"),
        (status = StatusCode::NOT_FOUND, description = "Deployment not found"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TAG
)]
pub async fn api_rush_rollout(
    Path(deployment_id): Path<i32>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<Json<RushResult>, ApiError> {
    let result = rush_rollout(deployment_id, &state.pg_pool, &current_user)
        .await
        .inspect_err(|e| {
            e.log_if_unexpected(format!("Failed to finish deployment {deployment_id}"))
        })?;
    if result.devices_released > 0 {
        let user_email = current_user_email(&state, &current_user).await;
        let detail = format!("{} devices released at once", result.devices_released);
        notify_control(
            ":fast_forward: *Rollout Rushed*",
            deployment_id,
            Some(&detail),
            &state.pg_pool,
            state.config,
            user_email.as_deref(),
        )
        .await;
    }
    Ok(Json(result))
}

/// What deploying a release would change and whether the signed-in user may do it.
/// Changes nothing, so a page can show the answer before anyone commits.
#[utoipa::path(
    post,
    path = "/releases/{release_id}/deployment/preflight",
    params(("release_id" = i32, Path)),
    request_body = PreflightRequest,
    responses(
        (status = StatusCode::OK, body = DeploymentPreflight),
        (status = StatusCode::NOT_FOUND, description = "Release not found"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TAG
)]
pub async fn api_deployment_preflight(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Json(request): Json<PreflightRequest>,
) -> Result<Json<DeploymentPreflight>, ApiError> {
    let result = preflight(release_id, request, &current_user, &state.pg_pool)
        .await
        .inspect_err(|e| {
            e.log_if_unexpected(format!("Failed preflight of release {release_id}"))
        })?;
    Ok(Json(result))
}

fn trimmed(reason: &Option<String>) -> Option<&str> {
    reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
}

pub(crate) async fn current_user_email(
    state: &State,
    current_user: &CurrentUser,
) -> Option<String> {
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
