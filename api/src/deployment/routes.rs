use crate::State;
use crate::deployment::schema::Deployment;
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
      ("Access Token" = [])
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
      ("Access Token" = [])
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
      ("Access Token" = [])
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
