use crate::State;
use crate::middlewares::authorization;
use crate::user::CurrentUser;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Extension, Json};
use models::ip_address::IpAddressInfo;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};
use utoipa::ToSchema;

const IP_ADDRESS_TAG: &str = "ip_address";

#[utoipa::path(
    get,
    path = "/ip_address/{ip_address_id}",
    params(
        ("ip_address_id" = i64, Path),
    ),
    responses(
        (status = 200, description = "IP address information retrieved successfully", body = IpAddressInfo),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "IP address not found"),
        (status = 500, description = "Failed to retrieve IP address information", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = IP_ADDRESS_TAG
)]
pub async fn get_ip_address_info(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Path(ip_address_id): Path<i64>,
) -> axum::response::Result<Json<IpAddressInfo>, StatusCode> {
    let allowed = authorization::check(current_user, "devices", "read");

    if !allowed {
        return Err(StatusCode::FORBIDDEN);
    }

    debug!("Getting IP address info for ID: {}", ip_address_id);

    let ip_info = sqlx::query!(
        r#"
        SELECT
            ip.id,
            ip.ip_address,
            ip.name,
            ip.continent,
            ip.continent_code,
            ip.country_code,
            ip.country,
            ip.region,
            ip.city,
            ip.isp,
            ip.coordinates[0] as longitude,
            ip.coordinates[1] as latitude,
            ip.proxy,
            ip.hosting,
            ip.created_at,
            ip.updated_at,
            ip.max_concurrent_downloads,
            COUNT(d.id) as device_count
        FROM ip_address ip
        LEFT JOIN device d ON d.ip_address_id = ip.id
        WHERE ip.id = $1
        GROUP BY ip.id
        "#,
        ip_address_id
    )
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get IP address info: {}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let ip_info = ip_info.ok_or(StatusCode::NOT_FOUND)?;

    let coordinates = match (ip_info.longitude, ip_info.latitude) {
        (Some(lon), Some(lat)) => Some((lon, lat)),
        _ => None,
    };

    let response = IpAddressInfo {
        id: ip_info.id,
        ip_address: ip_info.ip_address,
        name: ip_info.name,
        continent: ip_info.continent,
        continent_code: ip_info.continent_code,
        country_code: ip_info.country_code,
        country: ip_info.country,
        region: ip_info.region,
        city: ip_info.city,
        isp: ip_info.isp,
        coordinates,
        proxy: ip_info.proxy,
        hosting: ip_info.hosting,
        device_count: ip_info.device_count,
        max_concurrent_downloads: ip_info.max_concurrent_downloads,
        created_at: ip_info.created_at,
        updated_at: ip_info.updated_at,
    };

    Ok(Json(response))
}

#[derive(Serialize, Deserialize, ToSchema, Debug)]
pub struct IpAddressListResponse {
    pub ip_addresses: Vec<IpAddressInfo>,
}

#[derive(Serialize, Deserialize, ToSchema, Debug)]
pub struct UpdateIpAddressRequest {
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, ToSchema, Debug)]
pub struct UpdateDownloadLimitRequest {
    /// Null restores the default.
    pub max_concurrent_downloads: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/ip_addresses",
    responses(
        (status = 200, description = "List of all IP addresses retrieved successfully", body = IpAddressListResponse),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Failed to retrieve IP addresses", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = IP_ADDRESS_TAG
)]
pub async fn get_ip_addresses(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<Json<IpAddressListResponse>, StatusCode> {
    let allowed = authorization::check(current_user, "devices", "read");

    if !allowed {
        return Err(StatusCode::FORBIDDEN);
    }

    debug!("Getting all IP addresses");

    let ip_infos = sqlx::query!(
        r#"
        SELECT
            ip.id,
            ip.ip_address,
            ip.name,
            ip.continent,
            ip.continent_code,
            ip.country_code,
            ip.country,
            ip.region,
            ip.city,
            ip.isp,
            ip.coordinates[0] as longitude,
            ip.coordinates[1] as latitude,
            ip.proxy,
            ip.hosting,
            ip.created_at,
            ip.updated_at,
            ip.max_concurrent_downloads,
            COUNT(d.id) as device_count
        FROM ip_address ip
        LEFT JOIN device d ON d.ip_address_id = ip.id
        GROUP BY ip.id
        HAVING COUNT(d.id) > 0
        ORDER BY COUNT(d.id) DESC
        "#,
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get IP addresses: {}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let ip_addresses = ip_infos
        .into_iter()
        .map(|ip_info| {
            let coordinates = match (ip_info.longitude, ip_info.latitude) {
                (Some(lon), Some(lat)) => Some((lon, lat)),
                _ => None,
            };

            IpAddressInfo {
                id: ip_info.id,
                ip_address: ip_info.ip_address,
                name: ip_info.name,
                continent: ip_info.continent,
                continent_code: ip_info.continent_code,
                country_code: ip_info.country_code,
                country: ip_info.country,
                region: ip_info.region,
                city: ip_info.city,
                isp: ip_info.isp,
                coordinates,
                proxy: ip_info.proxy,
                hosting: ip_info.hosting,
                device_count: ip_info.device_count,
                max_concurrent_downloads: ip_info.max_concurrent_downloads,
                created_at: ip_info.created_at,
                updated_at: ip_info.updated_at,
            }
        })
        .collect();

    let response = IpAddressListResponse { ip_addresses };

    Ok(Json(response))
}

#[utoipa::path(
    put,
    path = "/ip_address/{ip_address_id}",
    params(
        ("ip_address_id" = i64, Path),
    ),
    request_body = UpdateIpAddressRequest,
    responses(
        (status = 200, description = "IP address updated successfully", body = IpAddressInfo),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "IP address not found"),
        (status = 500, description = "Failed to update IP address", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = IP_ADDRESS_TAG
)]
pub async fn update_ip_address(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Path(ip_address_id): Path<i64>,
    Json(request): Json<UpdateIpAddressRequest>,
) -> Result<Json<IpAddressInfo>, StatusCode> {
    let allowed = authorization::check(current_user, "devices", "write");

    if !allowed {
        return Err(StatusCode::FORBIDDEN);
    }

    debug!(
        "Updating IP address ID: {} with name: {:?}",
        ip_address_id, request.name
    );

    // Update the IP address
    let updated_ip = sqlx::query!(
        r#"
        UPDATE ip_address
        SET name = $1, updated_at = NOW()
        WHERE id = $2
        RETURNING
            id,
            ip_address,
            name,
            continent,
            continent_code,
            country_code,
            country,
            region,
            city,
            isp,
            coordinates[0] as longitude,
            coordinates[1] as latitude,
            proxy,
            hosting,
            created_at,
            updated_at,
            max_concurrent_downloads
        "#,
        request.name,
        ip_address_id
    )
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to update IP address: {}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let updated_ip = updated_ip.ok_or(StatusCode::NOT_FOUND)?;

    // Get device count for the updated IP
    let device_count = sqlx::query!(
        "SELECT COUNT(*) as count FROM device WHERE ip_address_id = $1",
        ip_address_id
    )
    .fetch_one(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get device count: {}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let coordinates = match (updated_ip.longitude, updated_ip.latitude) {
        (Some(lon), Some(lat)) => Some((lon, lat)),
        _ => None,
    };

    let response = IpAddressInfo {
        id: updated_ip.id,
        ip_address: updated_ip.ip_address,
        name: updated_ip.name,
        continent: updated_ip.continent,
        continent_code: updated_ip.continent_code,
        country_code: updated_ip.country_code,
        country: updated_ip.country,
        region: updated_ip.region,
        city: updated_ip.city,
        isp: updated_ip.isp,
        coordinates,
        proxy: updated_ip.proxy,
        hosting: updated_ip.hosting,
        device_count: device_count.count,
        max_concurrent_downloads: updated_ip.max_concurrent_downloads,
        created_at: updated_ip.created_at,
        updated_at: updated_ip.updated_at,
    };

    Ok(Json(response))
}

/// Sets how many devices behind this public IP may download a release at the
/// same time. During a rollout, a device whose turn has come only learns its
/// new target once fewer than this many other devices on the IP are
/// downloading; the others keep their current release and retry on their next
/// poll. Devices that are offline, archived or stuck for over two hours do not
/// count. Null restores the default of 2.
#[utoipa::path(
    put,
    path = "/ip_address/{ip_address_id}/download-limit",
    params(
        ("ip_address_id" = i64, Path),
    ),
    request_body = UpdateDownloadLimitRequest,
    responses(
        (status = 200, description = "Download limit updated successfully", body = IpAddressInfo),
        (status = 400, description = "The limit must be at least 1", body = String),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "IP address not found"),
        (status = 500, description = "Failed to update the download limit", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = IP_ADDRESS_TAG
)]
pub async fn update_download_limit(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Path(ip_address_id): Path<i64>,
    Json(request): Json<UpdateDownloadLimitRequest>,
) -> Result<Json<IpAddressInfo>, (StatusCode, String)> {
    if !authorization::check(current_user, "devices", "write") {
        return Err((StatusCode::FORBIDDEN, String::new()));
    }
    if request
        .max_concurrent_downloads
        .is_some_and(|limit| limit < 1)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "max_concurrent_downloads must be at least 1".to_string(),
        ));
    }

    set_download_limit(
        &state.pg_pool,
        ip_address_id,
        request.max_concurrent_downloads,
    )
    .await
    .map_err(|err| {
        error!("Failed to update download limit: {}", err);
        (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    })?
    .map(Json)
    .ok_or((StatusCode::NOT_FOUND, String::new()))
}

async fn set_download_limit(
    pool: &sqlx::PgPool,
    ip_address_id: i64,
    limit: Option<i32>,
) -> sqlx::Result<Option<IpAddressInfo>> {
    let row = sqlx::query!(
        r#"
        UPDATE ip_address ip
        SET max_concurrent_downloads = $1, updated_at = NOW()
        WHERE ip.id = $2
        RETURNING
            ip.id,
            ip.ip_address,
            ip.name,
            ip.continent,
            ip.continent_code,
            ip.country_code,
            ip.country,
            ip.region,
            ip.city,
            ip.isp,
            ip.coordinates[0] as longitude,
            ip.coordinates[1] as latitude,
            ip.proxy,
            ip.hosting,
            ip.created_at,
            ip.updated_at,
            ip.max_concurrent_downloads,
            (SELECT COUNT(*) FROM device d WHERE d.ip_address_id = ip.id) AS device_count
        "#,
        limit,
        ip_address_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| IpAddressInfo {
        id: row.id,
        ip_address: row.ip_address,
        name: row.name,
        continent: row.continent,
        continent_code: row.continent_code,
        country_code: row.country_code,
        country: row.country,
        region: row.region,
        city: row.city,
        isp: row.isp,
        coordinates: match (row.longitude, row.latitude) {
            (Some(lon), Some(lat)) => Some((lon, lat)),
            _ => None,
        },
        proxy: row.proxy,
        hosting: row.hosting,
        device_count: row.device_count,
        max_concurrent_downloads: row.max_concurrent_downloads,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn download_limit_can_be_set_and_cleared(pool: PgPool) -> anyhow::Result<()> {
        let ip: i64 = sqlx::query_scalar(
            "INSERT INTO ip_address (ip_address) VALUES ('203.0.113.9') RETURNING id",
        )
        .fetch_one(&pool)
        .await?;

        let info = set_download_limit(&pool, ip, Some(5)).await?;
        assert_eq!(info.map(|i| i.max_concurrent_downloads), Some(Some(5)));
        let info = set_download_limit(&pool, ip, None).await?;
        assert_eq!(info.map(|i| i.max_concurrent_downloads), Some(None));
        assert!(set_download_limit(&pool, ip + 1, Some(1)).await?.is_none());
        assert!(
            set_download_limit(&pool, ip, Some(0)).await.is_err(),
            "the CHECK constraint rejects zero"
        );
        Ok(())
    }
}
