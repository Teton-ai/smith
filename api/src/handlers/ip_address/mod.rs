use crate::State;
use crate::middlewares::authorization;
use crate::users::db::CurrentUser;
use axum::http::StatusCode;
use axum::response::Result;
use axum::{Extension, Json, extract::Path};
use serde::{Deserialize, Serialize};
use sqlx::types::{chrono, ipnetwork::IpNetwork};
use tracing::{debug, error};
use utoipa::ToSchema;

const IP_ADDRESS_TAG: &str = "ip_address";

#[derive(Serialize, Deserialize, ToSchema, Debug)]
pub struct IpAddressInfo {
    pub id: i32,
    #[schema(value_type = String, example = "192.168.1.1")]
    pub ip_address: IpNetwork,
    pub name: Option<String>,
    pub continent: Option<String>,
    pub continent_code: Option<String>,
    pub country_code: Option<String>,
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub isp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinates: Option<(f64, f64)>,
    pub proxy: Option<bool>,
    pub hosting: Option<bool>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[utoipa::path(
    get,
    path = "/ip_address/{ip_address_id}",
    responses(
        (status = 200, description = "IP address information retrieved successfully", body = IpAddressInfo),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "IP address not found"),
        (status = 500, description = "Failed to retrieve IP address information", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = IP_ADDRESS_TAG
)]
pub async fn get_ip_address_info(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Path(ip_address_id): Path<i32>,
) -> Result<Json<IpAddressInfo>, StatusCode> {
    let allowed = authorization::check(current_user, "devices", "read");

    if !allowed {
        return Err(StatusCode::FORBIDDEN);
    }

    debug!("Getting IP address info for ID: {}", ip_address_id);

    let ip_info = sqlx::query!(
        r#"
        SELECT 
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
            updated_at
        FROM ip_address 
        WHERE id = $1
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
        created_at: ip_info.created_at,
        updated_at: ip_info.updated_at,
    };

    Ok(Json(response))
}
