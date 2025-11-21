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

/// Handle an HTTP GET for a package by ID and return it as an HTTP response.
///
/// Fetches the package for `package_id` and returns the package serialized as JSON in an HTTP response.
/// On failure, returns an HTTP error response produced by the handler's error path.
///
/// # Returns
///
/// `Ok(Response)` containing the package as JSON on success, `Err(Response)` containing an error HTTP response on failure.
///
/// # Examples
///
/// ```
/// # use axum::{extract::{Path, Extension}, response::Response};
/// # use std::sync::Arc;
/// # // `state` would be your application State containing DB pool/config.
/// # let state = todo!();
/// # let state_ext = Extension(state);
/// # let path = Path(42);
/// # async {
/// let res: Result<Response, Response> = get_package_by_id(path, state_ext).await;
/// match res {
///     Ok(response) => { /* 200/JSON response with package */ }
///     Err(err_response) => { /* error response */ }
/// }
/// # };
/// ```
pub async fn get_package_by_id(
    Path(package_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Response, Response> {
    get_package_info_by_id(Path(package_id), Extension(state))
        .await
        .map(|json| json.into_response())
}

/// Fetches a package by its ID from the database and returns it as JSON.
///
/// On database query failure this handler logs the error and returns an HTTP error response.
///
/// # Returns
///
/// `Json<Package>` containing the requested package on success, or an HTTP `Response` representing an error.
///
/// # Examples
///
/// ```
/// use axum::{routing::get, Router};
///
/// // attach the handler to a route; Axum will provide `Path` and `Extension<State>`
/// let app = Router::new().route("/packages/:id", get(get_package_info_by_id));
/// ```
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

/// Delete the package with the given ID and return the corresponding HTTP status.
///
/// Attempts to remove the package identified by `package_id` from storage; logs an error and
/// maps failures to an internal server error status.
///
/// # Returns
///
/// `StatusCode::NO_CONTENT` on success, `StatusCode::INTERNAL_SERVER_ERROR` on failure.
///
/// # Examples
///
/// ```
/// use axum::http::StatusCode;
/// // On success the handler yields NO_CONTENT
/// let status = StatusCode::NO_CONTENT;
/// assert_eq!(status, StatusCode::NO_CONTENT);
/// ```
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