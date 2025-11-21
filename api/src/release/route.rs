use crate::State;
use crate::release::Release;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use smith::utils::schema::Package;
use tracing::error;

const RELEASES_TAG: &str = "releases";

/// Retrieve all releases including their distribution name and architecture.
///
/// Fetches every row from the `release` table joined with `distribution` and returns them as a JSON array.
///
/// # Examples
///
/// ```no_run
/// use axum::Extension;
/// use axum::response::Json;
/// # async fn example(state: crate::State) {
/// let response: Json<Vec<crate::release::model::Release>> =
///     crate::release::route::get_releases(Extension(state)).await.unwrap();
/// let releases = response.0;
/// # }
/// ```
#[utoipa::path(
get,
path = "/releases",
responses(
(status = StatusCode::OK, description = "List of releases retrieved successfully", body = Vec<Release>),
(status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve releases"),
),
security(
("auth_token" = [])
),
tag = RELEASES_TAG
)]
pub async fn get_releases(
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Vec<Release>>, StatusCode> {
    let releases = sqlx::query_as!(
        Release,
        "
        SELECT release.*,
        distribution.name AS distribution_name,
        distribution.architecture AS distribution_architecture
        FROM release
        JOIN distribution ON release.distribution_id = distribution.id
        ",
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get releases {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(releases))
}

#[utoipa::path(
    get,
    path = "/releases/{release_id}",
    responses(
        (status = StatusCode::OK, description = "Release retrieved successfully", body = Release),
        (status = StatusCode::NOT_FOUND, description = "Release not found"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve release"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = RELEASES_TAG
)]
pub async fn get_release(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Release>, StatusCode> {
    let release = Release::get_release_by_id(release_id, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get releases {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if release.is_none() {
        error!("Release {release_id} not found");
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(Json(release.unwrap()))
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UpdateRelease {
    pub draft: Option<bool>,
    pub yanked: Option<bool>,
}

#[utoipa::path(
    post,
    path = "/releases/{release_id}",
    request_body = UpdateRelease,
    responses(
        (status = StatusCode::NO_CONTENT, description = "Release updated successfully"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to update release"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = RELEASES_TAG
)]
pub async fn update_release(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
    Json(update_release): Json<UpdateRelease>,
) -> axum::response::Result<StatusCode, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if let Some(draft) = update_release.draft {
        sqlx::query!(
            "UPDATE release SET draft = $1 WHERE id = $2",
            draft,
            release_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to update release: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }
    if let Some(yanked) = update_release.yanked {
        sqlx::query!(
            "UPDATE release SET yanked = $1 WHERE id = $2",
            yanked,
            release_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to update release: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }
    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ReplacementPackage {
    pub id: i32,
}

/// Adds a package to the specified release.
///
/// The handler inserts a mapping between the release and the provided package ID
/// only when the release exists and is a draft that is not yanked.
///
/// # Parameters
///
/// - `release_id`: ID of the target release.
/// - `package`: `ReplacementPackage` containing the `id` of the package to add.
///
/// # Returns
///
/// `StatusCode::OK` on successful insertion; `StatusCode::NOT_FOUND` if the release
/// does not exist; `StatusCode::CONFLICT` if the release is yanked or not a draft;
/// `StatusCode::INTERNAL_SERVER_ERROR` for database errors.
///
/// # Examples
///
/// ```
/// use crate::api::release::route::ReplacementPackage;
///
/// let pkg = ReplacementPackage { id: 42 };
/// // POST /releases/1/packages with JSON body `pkg`
/// ```
#[utoipa::path(
post,
path = "/releases/{release_id}/packages",
request_body = ReplacementPackage,
responses(
(status = StatusCode::CREATED, description = "Package added to release successfully"),
(status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to add package to release"),
),
security(
("auth_token" = [])
),
tag = RELEASES_TAG
)]
pub async fn add_package_to_release(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
    Json(package): Json<ReplacementPackage>,
) -> axum::response::Result<StatusCode, StatusCode> {
    let release = Release::get_release_by_id(release_id, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get release: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if release.is_none() {
        error!("Release {release_id} not found");
        return Err(StatusCode::NOT_FOUND);
    }
    let target_release = release.unwrap();
    if target_release.yanked || !target_release.draft {
        return Err(StatusCode::CONFLICT);
    }
    sqlx::query!(
        "
        INSERT INTO release_packages (release_id, package_id)
        VALUES ($1, $2)
        ",
        release_id,
        package.id
    )
    .execute(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to add package {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::OK)
}

/// Fetches all packages associated with a release, ordered by package name.
///
/// Returns a JSON array of `Package` objects belonging to the specified release.
///
/// # Examples
///
/// ```no_run
/// use reqwest::blocking::Client;
/// let client = Client::new();
/// let resp = client
///     .get("http://localhost:3000/releases/1/packages")
///     .header("Authorization", "Bearer <token>")
///     .send()
///     .expect("request failed");
/// assert!(resp.status().is_success());
/// let packages: Vec<your_crate::models::Package> = resp.json().expect("invalid json");
/// ```
#[utoipa::path(
get,
path = "/releases/{release_id}/packages",
responses(
(status = StatusCode::OK, description = "Release packages retrieved successfully"),
(status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve release packages"),
),
security(
("auth_token" = [])
),
tag = RELEASES_TAG
)]
pub async fn get_distribution_release_packages(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Vec<Package>>, StatusCode> {
    let packages = sqlx::query_as!(
        Package,
        "
        SELECT package.* FROM package
        JOIN release_packages ON package.id = release_packages.package_id
        JOIN release ON release.id = release_packages.release_id
        WHERE release.id = $1
        ORDER BY package.name
        ",
        release_id
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get packages {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(packages))
}

/// Update the package associated with a release.
///
/// Attempts to replace an existing package entry for the given `release_id` and `package_id` with the provided package ID.
///
/// Returns
///
/// `StatusCode::OK` on success. Returns `StatusCode::NOT_FOUND` if the release does not exist, `StatusCode::CONFLICT` if the release is yanked or not a draft, and `StatusCode::INTERNAL_SERVER_ERROR` for database errors.
///
/// # Examples
///
/// ```
/// use axum::http::StatusCode;
/// // On success the handler responds with `StatusCode::OK`.
/// let ok = StatusCode::OK;
/// assert_eq!(ok, StatusCode::OK);
/// ```
#[utoipa::path(
put,
path = "/releases/{release_id}/packages/{package_id}",
responses(
(status = StatusCode::OK, description = "Successfully updated release package "),
(status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to update release package"),
),
security(
("auth_token" = [])
),
tag = RELEASES_TAG
)]
pub async fn update_package_for_release(
    Path((release_id, package_id)): Path<(i32, i32)>,
    Extension(state): Extension<State>,
    Json(package): Json<ReplacementPackage>,
) -> axum::response::Result<StatusCode, StatusCode> {
    let release = Release::get_release_by_id(release_id, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get release: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if release.is_none() {
        error!("Release {release_id} not found");
        return Err(StatusCode::NOT_FOUND);
    }
    let target_release = release.unwrap();
    if target_release.yanked || !target_release.draft {
        return Err(StatusCode::CONFLICT);
    }
    sqlx::query!(
        "
        UPDATE release_packages SET package_id = $1
        WHERE release_id = $2 AND package_id = $3
        ",
        package.id,
        release_id,
        package_id
    )
    .execute(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to update package {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(StatusCode::OK)
}

#[utoipa::path(
    delete,
    path = "/releases/{release_id}/packages/{package_id}",
    responses(
        (status = StatusCode::NO_CONTENT, description = "Successfully deleted package from the release"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to delete the package from the release"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = RELEASES_TAG
)]
pub async fn delete_package_for_release(
    Path((release_id, package_id)): Path<(i32, i32)>,
    Extension(state): Extension<State>,
) -> axum::response::Result<StatusCode, StatusCode> {
    let release = Release::get_release_by_id(release_id, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get releases {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if release.is_none() {
        error!("Release {release_id} not found");
        return Err(StatusCode::NOT_FOUND);
    }
    let target_release = release.unwrap();
    if target_release.yanked || !target_release.draft {
        return Err(StatusCode::CONFLICT);
    }
    sqlx::query!(
        "DELETE FROM release_packages WHERE release_id = $1 AND package_id = $2",
        release_id,
        package_id
    )
    .execute(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to remove package {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::NO_CONTENT)
}