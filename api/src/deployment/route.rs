use crate::State;
use crate::deployment::{
    Deployment, DeploymentDeviceWithStatus, confirm_full_rollout, get_deployment,
    get_devices_in_deployment, new_deployment,
};
use crate::error::ApiError;
use crate::user::CurrentUser;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Extension, Json};
use models::deployment::DeploymentRequest;

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
    Extension(current_user): Extension<CurrentUser>,
    request: Option<Json<DeploymentRequest>>,
) -> Result<Json<Deployment>, ApiError> {
    let user_email = sqlx::query_scalar!(
        "SELECT email FROM auth.users WHERE id = $1",
        current_user.user_id
    )
    .fetch_optional(&state.pg_pool)
    .await
    .ok()
    .flatten()
    .flatten();

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
    Extension(current_user): Extension<CurrentUser>,
) -> Result<(StatusCode, Json<Deployment>), StatusCode> {
    let user_email = sqlx::query_scalar!(
        "SELECT email FROM auth.users WHERE id = $1",
        current_user.user_id
    )
    .fetch_optional(&state.pg_pool)
    .await
    .ok()
    .flatten()
    .flatten();

    let deployment = confirm_full_rollout(release_id, &state.pg_pool, state.config, user_email.as_deref())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::OK, Json(deployment)))
}
