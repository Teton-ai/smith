use crate::State;
use crate::deployment::{Deployment, DeploymentDeviceWithStatus};
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Extension, Json};

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
    let release = Deployment::get(release_id, &state.pg_pool)
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
) -> Result<(StatusCode, Json<Deployment>), StatusCode> {
    let release = Deployment::new(release_id, &state.pg_pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::OK, Json(release)))
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
    let release = Deployment::check_done(release_id, &state.pg_pool)
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
    let devices = Deployment::get_devices(release_id, &state.pg_pool)
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
    let deployment = Deployment::confirm_full_rollout(
        release_id,
        &state.pg_pool,
        state.config.slack_hook_url.as_deref(),
    )
    .await
    .map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok((StatusCode::OK, Json(deployment)))
}
