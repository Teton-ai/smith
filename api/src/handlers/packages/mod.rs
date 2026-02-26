use crate::State;
use crate::package::Package;
use axum::{
    Extension, Json,
    extract::Path,
    response::{IntoResponse, Response},
};
use axum::{http::StatusCode, response::Result};
use tracing::error;

// TODO: I believe this whole stuff is legacy and not documented, check and delete

pub async fn get_package_by_id(
    Path(package_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Response, Response> {
    get_package_info_by_id(Path(package_id), Extension(state))
        .await
        .map(|json| json.into_response())
}

async fn get_package_info_by_id(
    Path(package_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<Package>, Response> {
    let package = sqlx::query_as!(
        Package,
        "SELECT * FROM package WHERE package.id = $1",
        package_id
    )
    .fetch_one(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get package {err}");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    Ok(Json(package))
}

pub async fn delete_package_by_id(
    Path(package_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<StatusCode, StatusCode> {
    Package::delete(&package_id, state.config, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to delete the package {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}
