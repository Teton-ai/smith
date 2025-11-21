use crate::State;
use crate::device::{LeanDevice, LeanResponse};
use crate::distribution::Distribution;
use crate::release::Release;
use crate::user::CurrentUser;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::Deserialize;
use tracing::error;

const DISTRIBUTIONS_TAG: &str = "distributions";

#[utoipa::path(
    get,
    path = "/distributions",
    responses(
        (status = StatusCode::OK, description = "List of distributions retrieved successfully", body = Vec<Distribution>),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve distributions"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DISTRIBUTIONS_TAG
)]
pub async fn get_distributions(
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Vec<Distribution>>, StatusCode> {
    let distributions = sqlx::query_as!(
        Distribution,
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

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewDistribution {
    pub name: String,
    pub description: Option<String>,
    pub architecture: String,
}

#[utoipa::path(
    post,
    path = "/distributions",
    request_body = NewDistribution,
    responses(
        (status = StatusCode::CREATED, description = "Distribution created successfully"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to create distribution"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DISTRIBUTIONS_TAG
)]
pub async fn create_distribution(
    Extension(state): Extension<State>,
    Json(distribution): Json<NewDistribution>,
) -> axum::response::Result<StatusCode, StatusCode> {
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
    path = "/distributions/{distribution_id}",
    responses(
        (status = StatusCode::OK, description = "Return found distribution", body = Distribution),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve distribution"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DISTRIBUTIONS_TAG
)]
pub async fn get_distribution_by_id(
    Path(distribution_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Distribution>, StatusCode> {
    let distribution = sqlx::query_as!(
        Distribution,
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
    path = "/distributions/{distribution_id}/releases",
    responses(
        (status = StatusCode::OK, description = "List of releases from given distribution retrieved successfully", body = Vec<Release>),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to distribution releases"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DISTRIBUTIONS_TAG
)]
pub async fn get_distribution_releases(
    Path(distribution_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Vec<Release>>, StatusCode> {
    let releases = sqlx::query_as!(
        Release,
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
    path = "/distributions/{distribution_id}/releases/latest",
    responses(
        (status = StatusCode::OK, description = "Get the latest published release for the distribution", body = Release),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to latest release"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DISTRIBUTIONS_TAG
)]
pub async fn get_distribution_latest_release(
    Path(distribution_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Release>, StatusCode> {
    let release = Release::get_latest_distribution_release(distribution_id, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get latest release {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(release))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewDistributionRelease {
    pub version: String,
    pub packages: Vec<i32>,
}

/// Creates a new release for the specified distribution and associates the given packages with that release.
///
/// This starts a database transaction, inserts a new release record linked to the authenticated user, verifies all provided package IDs exist, inserts the package associations for the new release, and commits the transaction. If any package ID is missing, the request is rejected with a `400 Bad Request`.
///
/// # Returns
///
/// `Json<i32>` containing the newly created release `id`.
///
/// # Examples
///
/// ```
/// use my_crate::models::NewDistributionRelease;
///
/// let payload = NewDistributionRelease {
///     version: "1.0.0".to_string(),
///     packages: vec![1, 2, 3],
/// };
///
/// // The handler is async and expects application state and an authenticated user.
/// // This example only demonstrates creating the request payload.
/// let _ = payload;
/// ```
#[utoipa::path(
post,
path = "/distributions/{distribution_id}/releases",
request_body = NewDistributionRelease,
responses(
(status = StatusCode::CREATED, description = "Distribution release created successfully", body = i32),
(status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to create distribution release"),
),
security(
("auth_token" = [])
),
tag = DISTRIBUTIONS_TAG
)]
pub async fn create_distribution_release(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Path(distribution_id): Path<i32>,
    Json(distribution_release): Json<NewDistributionRelease>,
) -> axum::response::Result<Json<i32>, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let release = sqlx::query!(
        "INSERT INTO release (distribution_id, version, user_id) VALUES ($1, $2, $3) RETURNING id",
        distribution_id,
        distribution_release.version,
        current_user.user_id
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
    path = "/distributions/{distribution_id}",
    responses(
        (status = StatusCode::NO_CONTENT, description = "Successfully deleted the distribution"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to delete distribution"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DISTRIBUTIONS_TAG
)]
pub async fn delete_distribution_by_id(
    Path(distribution_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<StatusCode, StatusCode> {
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
    path = "/distributions/{distribution_id}/devices",
    responses(
        (status = StatusCode::OK, description = "Get devices on this distribution", body = LeanDevice),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to delete distribution"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DISTRIBUTIONS_TAG
)]
#[deprecated(note = "We are moving to `/devices` endpoint and use release param as filter")]
pub async fn get_distribution_devices(
    Path(distribution_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<LeanResponse>, StatusCode> {
    let devices = sqlx::query_as!(
        LeanDevice,
        r#"
        SELECT device.id, serial_number, last_ping as last_seen, approved, release_id = target_release_id as up_to_date, ip_address_id FROM device LEFT JOIN release on release_id = release.id where release.distribution_id = $1
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