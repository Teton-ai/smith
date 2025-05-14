use crate::State;
use crate::handlers::devices::types::{LeanDevice, LeanResponse};
use crate::handlers::distributions::db::db_get_latest_distribution_release;
use axum::{Extension, Json, extract::Path};
use axum::{http::StatusCode, response::Result};
use tracing::error;

pub mod db;
pub mod types;

const DISTRIBUTIONS_TAG: &str = "distributions";

#[utoipa::path(
    get,
    path = "/distributions",
    responses(
        (status = StatusCode::OK, description = "List of distributions retrieved successfully", body = Vec<types::Distribution>),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve distributions"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DISTRIBUTIONS_TAG
)]
pub async fn get_distributions(
    Extension(state): Extension<State>,
) -> Result<Json<Vec<types::Distribution>>, StatusCode> {
    let distributions = sqlx::query_as!(
        types::Distribution,
        r#"SELECT
            d.id,
            d.name,
            d.description,
            d.architecture,
            (
                SELECT COUNT(*)
                FROM release_packages rp
                JOIN release r ON r.id = rp.release_id
                WHERE r.distribution_id = d.id
                  AND r.version = '1.0.0'
            )::int AS num_packages
        FROM distribution d
        ORDER BY d.name"#
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get distributions {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(distributions))
}

#[utoipa::path(
    post,
    path = "/distributions",
    request_body = types::NewDistribution,
    responses(
        (status = StatusCode::CREATED, description = "Distribution created successfully"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to create distribution"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DISTRIBUTIONS_TAG
)]
pub async fn create_distribution(
    Extension(state): Extension<State>,
    Json(distribution): Json<types::NewDistribution>,
) -> Result<StatusCode, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    sqlx::query_scalar!(
        "
        INSERT INTO distribution (name, architecture, description)
        VALUES ($1, $2, $3) RETURNING id
        ",
        distribution.name,
        distribution.architecture,
        distribution.description
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to create distribution: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::CREATED)
}

#[utoipa::path(
    get,
    path = "/distributions/:distribution_id",
    responses(
        (status = StatusCode::OK, description = "Return found distribution", body = types::Distribution),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve distribution"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DISTRIBUTIONS_TAG
)]
pub async fn get_distribution_by_id(
    Path(distribution_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<types::Distribution>, StatusCode> {
    let distribution = sqlx::query_as!(
        types::Distribution,
        r#"SELECT
            d.id,
            d.name,
            d.description,
            d.architecture,
            (
                SELECT COUNT(*)
                FROM release_packages rp
                JOIN release r ON r.id = rp.release_id
                WHERE r.distribution_id = d.id
                  AND r.version = '1.0.0'
            )::int AS num_packages
        FROM distribution d
        WHERE d.id = $1"#,
        distribution_id
    )
    .fetch_one(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get distribution {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(distribution))
}

#[utoipa::path(
    get,
    path = "/distributions/:distribution_id/releases",
    responses(
        (status = StatusCode::OK, description = "List of releases from given distribution retrieved successfully", body = Vec<types::Release>),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to distribution releases"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DISTRIBUTIONS_TAG
)]
pub async fn get_distribution_releases(
    Path(distribution_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<Vec<types::Release>>, StatusCode> {
    let releases = sqlx::query_as!(
        types::Release,
        r#"
        SELECT release.*,
        distribution.name AS distribution_name,
        distribution.architecture AS distribution_architecture
        FROM release
        JOIN distribution ON release.distribution_id = distribution.id
        WHERE distribution_id = $1
        ORDER BY release.created_at DESC"#,
        distribution_id
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
    path = "/distributions/:distribution_id/releases/latest",
    responses(
        (status = StatusCode::OK, description = "Get the latest published release for the distribution", body = types::Release),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to latest release"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DISTRIBUTIONS_TAG
)]
pub async fn get_distribution_latest_release(
    Path(distribution_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<types::Release>, StatusCode> {
    let release = db_get_latest_distribution_release(distribution_id, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get latest release {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(release))
}

#[utoipa::path(
    post,
    path = "/distributions/:distribution_id/releases",
    request_body = types::NewDistributionRelease,
    responses(
        (status = StatusCode::CREATED, description = "Distribution release created successfully", body = i32),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to create distribution release"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DISTRIBUTIONS_TAG
)]
#[tracing::instrument]
pub async fn create_distribution_release(
    Extension(state): Extension<State>,
    Path(distribution_id): Path<i32>,
    Json(distribution_release): Json<types::NewDistributionRelease>,
) -> Result<Json<i32>, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let release = sqlx::query!(
        "INSERT INTO release (distribution_id, version) VALUES ($1, $2) RETURNING id",
        distribution_id,
        distribution_release.version
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to create release: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let packages_exist = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM package WHERE id = ANY($1)",
        &distribution_release.packages as &[i32]
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to check if packages exist: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if packages_exist != Option::from(distribution_release.packages.len() as i64) {
        error!("One or more packages do not exist");
        return Err(StatusCode::BAD_REQUEST);
    }

    sqlx::query_scalar!(
        "
        INSERT INTO release_packages (package_id, release_id)
        SELECT value AS package_id, $1 AS release_id
        FROM UNNEST($2::int[]) AS value
        ",
        release.id,
        &distribution_release.packages as &[i32],
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to insert packages into release_package: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(release.id))
}

#[utoipa::path(
    delete,
    path = "/distributions/:distribution_id",
    responses(
        (status = StatusCode::NO_CONTENT, description = "Successfully deleted the distribution"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to delete distribution"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DISTRIBUTIONS_TAG
)]
pub async fn delete_distribution_by_id(
    Path(distribution_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<StatusCode, StatusCode> {
    sqlx::query!(r#"DELETE FROM distribution WHERE id = $1"#, distribution_id)
        .execute(&state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to delete distribution {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/distributions/:distribution_id/devices",
    responses(
        (status = StatusCode::OK, description = "Get devices on this distribution", body = LeanDevice),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to delete distribution"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DISTRIBUTIONS_TAG
)]
pub async fn get_distribution_devices(
    Path(distribution_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<LeanResponse>, StatusCode> {
    let devices = sqlx::query_as!(
        LeanDevice,
        r#"
        SELECT device.id, serial_number, last_ping as last_seen, approved, release_id = target_release_id as up_to_date FROM device LEFT JOIN release on release_id = release.id where release.distribution_id = $1
        "#,
        distribution_id
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to fetch devices for distribution {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(LeanResponse {
        limit: 0,
        reverse: false,
        devices,
    }))
}
