use crate::State;
use crate::deployment::{
    Deployment, DeploymentDeviceWithStatus, confirm_full_rollout, get_deployment,
    get_devices_in_deployment, new_deployment,
};
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Extension, Json};
use models::deployment::DeploymentRequest;

const TAG: &str = "deployment";

#[utoipa::path(
  get,
  path = "/releases/{release_id}/deployment",
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
  responses(
        (status = StatusCode::OK, body = Deployment),
  ),
  security(
      ("auth_token" = [])
  ),
  tag = TAG
)]
pub async fn api_release_deployment(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
    request: Option<Json<DeploymentRequest>>,
) -> Result<Json<Deployment>, StatusCode> {
    let release = new_deployment(release_id, request.map(|req| req.0), &state.pg_pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to create new deployment: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(release))
}

#[utoipa::path(
  patch,
  path = "/releases/{release_id}/deployment",
  responses(
        (status = StatusCode::OK, body = Deployment),
  ),
  security(
      ("auth_token" = [])
  ),
  tag = TAG
)]
pub async fn api_release_deployment_check_done(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<(StatusCode, Json<Deployment>), StatusCode> {
    let release = crate::deployment::check_done(release_id, &state.pg_pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::OK, Json(release)))
}

#[utoipa::path(
  get,
  path = "/releases/{release_id}/deployment/devices",
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
  responses(
        (status = StatusCode::OK, body = Deployment),
        (status = StatusCode::BAD_REQUEST, description = "Canary devices have not completed updating"),
  ),
  security(
      ("auth_token" = [])
  ),
  tag = TAG
)]
pub async fn api_confirm_full_rollout(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<(StatusCode, Json<Deployment>), StatusCode> {
    let deployment = confirm_full_rollout(release_id, &state.pg_pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::OK, Json(deployment)))
}
