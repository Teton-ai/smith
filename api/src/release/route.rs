use crate::State;
use crate::package::{extract_services_from_deb, Package};
use crate::release::{Release, get_release_by_id};
use crate::storage::Storage;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Extension, Json};
use models::release::UpdateRelease;
use serde::{Deserialize, Serialize};
use sqlx::types::chrono;
use tracing::{error, warn};

const RELEASES_TAG: &str = "releases";

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
        distribution.architecture AS distribution_architecture,
        auth.users.email AS user_email
        FROM release
        JOIN distribution ON release.distribution_id = distribution.id
        LEFT JOIN auth.users ON release.user_id = auth.users.id
        ORDER BY release.id
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
    params(
        ("release_id" = i32, Path),
    ),
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
    let release = get_release_by_id(release_id, &state.pg_pool)
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

#[utoipa::path(
    post,
    path = "/releases/{release_id}",
    params(
        ("release_id" = i32, Path),
    ),
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

#[utoipa::path(
    post,
    path = "/releases/{release_id}/packages",
    params(
        ("release_id" = i32, Path),
    ),
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
    let release = get_release_by_id(release_id, &state.pg_pool)
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

    // Get package details to extract services
    let pkg = sqlx::query_as!(Package, "SELECT * FROM package WHERE id = $1", package.id)
        .fetch_optional(&state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get package: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let pkg = match pkg {
        Some(p) => p,
        None => {
            error!("Package {} not found", package.id);
            return Err(StatusCode::NOT_FOUND);
        }
    };

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

    // Extract and register services from the package (non-blocking, non-fatal)
    if pkg.file.ends_with(".deb") {
        match Storage::download_from_s3(&state.config.packages_bucket_name, &pkg.file).await {
            Ok(data) => match extract_services_from_deb(&data) {
                Ok(services) => {
                    for service in services {
                        if let Err(e) = sqlx::query!(
                            "INSERT INTO release_services (release_id, package_id, service_name, watchdog_sec)
                             VALUES ($1, $2, $3, $4)
                             ON CONFLICT (release_id, service_name) DO NOTHING",
                            release_id,
                            package.id,
                            service.name,
                            service.watchdog_sec
                        )
                        .execute(&state.pg_pool)
                        .await
                        {
                            warn!("Failed to insert service {}: {}", service.name, e);
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to extract services from package {}: {}",
                        pkg.file, e
                    );
                }
            },
            Err(e) => {
                warn!(
                    "Failed to download package {} for service extraction: {}",
                    pkg.file, e
                );
            }
        }
    }

    Ok(StatusCode::OK)
}

#[utoipa::path(
    get,
    path = "/releases/{release_id}/packages",
    params(
        ("release_id" = i32, Path),
    ),
    responses(
        (status = StatusCode::OK, description = "Release packages retrieved successfully", body = Vec<Package>),
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

#[utoipa::path(
    put,
    path = "/releases/{release_id}/packages/{package_id}",
    params(
        ("release_id" = i32, Path),
        ("package_id" = i32, Path),
    ),
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
    let release = get_release_by_id(release_id, &state.pg_pool)
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
    params(
        ("release_id" = i32, Path),
        ("package_id" = i32, Path),
    ),
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
    let release = get_release_by_id(release_id, &state.pg_pool)
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

// Service-related types and endpoints

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ReleaseService {
    pub id: i32,
    pub release_id: i32,
    pub package_id: Option<i32>,
    pub service_name: String,
    pub watchdog_sec: Option<i32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateReleaseService {
    pub service_name: String,
    pub watchdog_sec: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/releases/{release_id}/services",
    params(
        ("release_id" = i32, Path, description = "Release ID")
    ),
    responses(
        (status = StatusCode::OK, description = "List of services for the release", body = Vec<ReleaseService>),
        (status = StatusCode::NOT_FOUND, description = "Release not found"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve services"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = RELEASES_TAG
)]
pub async fn get_release_services(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Vec<ReleaseService>>, StatusCode> {
    // Verify release exists
    let release = get_release_by_id(release_id, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get release: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if release.is_none() {
        error!("Release {release_id} not found");
        return Err(StatusCode::NOT_FOUND);
    }

    let services = sqlx::query_as!(
        ReleaseService,
        "SELECT * FROM release_services WHERE release_id = $1 ORDER BY service_name",
        release_id
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get release services: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(services))
}

#[utoipa::path(
    post,
    path = "/releases/{release_id}/services",
    params(
        ("release_id" = i32, Path, description = "Release ID")
    ),
    request_body = CreateReleaseService,
    responses(
        (status = StatusCode::CREATED, description = "Service added to release successfully", body = ReleaseService),
        (status = StatusCode::NOT_FOUND, description = "Release not found"),
        (status = StatusCode::CONFLICT, description = "Release is yanked or not in draft, or service already exists"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to add service"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = RELEASES_TAG
)]
pub async fn create_release_service(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
    Json(service): Json<CreateReleaseService>,
) -> axum::response::Result<(StatusCode, Json<ReleaseService>), StatusCode> {
    // Verify release exists and is in draft mode
    let release = get_release_by_id(release_id, &state.pg_pool)
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

    // Insert the service (package_id is NULL for manually created services)
    let created_service = sqlx::query_as!(
        ReleaseService,
        r#"
        INSERT INTO release_services (release_id, package_id, service_name, watchdog_sec)
        VALUES ($1, NULL, $2, $3)
        RETURNING *
        "#,
        release_id,
        service.service_name,
        service.watchdog_sec
    )
    .fetch_one(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to create release service: {err}");
        // Check if it's a unique constraint violation
        if err
            .to_string()
            .contains("release_services_release_id_service_name_key")
        {
            StatusCode::CONFLICT
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    })?;

    Ok((StatusCode::CREATED, Json(created_service)))
}

#[utoipa::path(
    delete,
    path = "/releases/{release_id}/services/{service_id}",
    params(
        ("release_id" = i32, Path, description = "Release ID"),
        ("service_id" = i32, Path, description = "Service ID")
    ),
    responses(
        (status = StatusCode::NO_CONTENT, description = "Service removed from release successfully"),
        (status = StatusCode::NOT_FOUND, description = "Release or service not found"),
        (status = StatusCode::CONFLICT, description = "Release is yanked or not in draft"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to remove service"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = RELEASES_TAG
)]
pub async fn delete_release_service(
    Path((release_id, service_id)): Path<(i32, i32)>,
    Extension(state): Extension<State>,
) -> axum::response::Result<StatusCode, StatusCode> {
    // Verify release exists and is in draft mode
    let release = get_release_by_id(release_id, &state.pg_pool)
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

    let result = sqlx::query!(
        "DELETE FROM release_services WHERE id = $1 AND release_id = $2",
        service_id,
        release_id
    )
    .execute(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to delete release service: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if result.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(StatusCode::NO_CONTENT)
}
