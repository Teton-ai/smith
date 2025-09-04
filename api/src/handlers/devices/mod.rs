use super::distributions::types::Release;
use crate::State;
use crate::handlers::events::PublicEvent;
use crate::middlewares::authorization;
use crate::users::db::CurrentUser;
use axum::extract::Host;
use axum::{Extension, Json, extract::Path};
use axum::{http::StatusCode, response::Result};
use axum_extra::extract::Query;
use schema::SafeCommandRequest;
use serde::Deserialize;
use sqlx::Row;
use tracing::{debug, error};
pub mod helpers;
pub mod types;
use crate::device::Device;
use crate::handlers::devices::types::DeviceHealth;
use crate::handlers::distributions::db::db_get_release_by_id;
use smith::utils::schema;

const DEVICES_TAG: &str = "devices";

#[derive(Deserialize, Debug)]
pub struct DeviceFilter {
    pub serial_number: Option<String>,
    pub approved: Option<bool>,
    pub tag: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct LeanDeviceFilter {
    reverse: Option<bool>,
    limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/lean/{filter_kind}/{filter_value}",
    responses(
        (status = 200, description = "Filtered devices", body = types::LeanResponse),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Failed to retrieve devices", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
// TODO: this is getting crazy huge, maybe it would be nice to have an handler
// per filter type instead of only 1 to handle all, maybe that could also have
// some performance beneficts to let axum handle the matching of the arms
pub async fn get_devices_new(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Path((filter_kind, filter_value)): Path<(String, String)>,
    Query(query_params): Query<LeanDeviceFilter>,
) -> Result<Json<types::LeanResponse>, StatusCode> {
    let reverse = query_params.reverse.unwrap_or(false);
    let limit = query_params.limit.unwrap_or(100);

    let allowed = authorization::check(current_user, "devices", "read");

    if !allowed {
        return Err(StatusCode::FORBIDDEN);
    }

    debug!(
        "Fetching devices with filter kind: {filter_kind}, filter value: {filter_value}, reverse: {reverse}, limit: {limit}"
    );
    let devices = match (filter_kind.as_str(), reverse) {
        ("sn", true) => {
            sqlx::query_as!(types::LeanDevice, "SELECT id, serial_number, last_ping as last_seen, approved, release_id = target_release_id as up_to_date, ip_address_id FROM device WHERE serial_number LIKE '%' || $1 || '%' AND archived = false LIMIT $2", filter_value, limit)
                .fetch_all(&state.pg_pool)
                .await
                .map_err(|err| {
                    error!("Failed to get devices {err}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })
        },
        ("sn", false) => {
            sqlx::query_as!(types::LeanDevice, "SELECT id, serial_number, last_ping as last_seen, approved, release_id = target_release_id as up_to_date, ip_address_id FROM device WHERE serial_number LIKE '%' || $1 || '%' AND archived = false LIMIT $2", filter_value, limit)
                .fetch_all(&state.pg_pool)
                .await
                .map_err(|err| {
                    error!("Failed to get devices {err}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })
        },
        ("approved", true) => {
            let value = filter_value.parse().unwrap_or(false);
            sqlx::query_as!(types::LeanDevice, "SELECT id, serial_number, last_ping as last_seen, approved, release_id = target_release_id as up_to_date, ip_address_id FROM device WHERE approved != $1 AND archived = false LIMIT $2", value, limit)
                .fetch_all(&state.pg_pool)
                .await
                .map_err(|err| {
                    error!("Failed to get devices {err}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })
        },
        ("approved", false) => {
            let value = filter_value.parse().unwrap_or(false);
            sqlx::query_as!(types::LeanDevice, "SELECT id, serial_number, last_ping as last_seen, approved, release_id = target_release_id as up_to_date, ip_address_id FROM device WHERE approved = $1 AND archived = false LIMIT $2", value, limit)
                .fetch_all(&state.pg_pool)
                .await
                .map_err(|err| {
                    error!("Failed to get devices {err}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })
        }
        ("tag", true) => {
            sqlx::query_as!(types::LeanDevice, r#"SELECT
                            d.id,
                            d.serial_number,
                            d.last_ping as last_seen,
                            d.approved,
                            release_id = target_release_id as up_to_date,
                            d.ip_address_id
                        FROM device d
                        JOIN tag_device td ON d.id = td.device_id
                        JOIN tag t ON td.tag_id = t.id
                        WHERE t.name != $1 AND d.archived = false
                        LIMIT $2
                "#, filter_value, limit)
                .fetch_all(&state.pg_pool)
                .await
                .map_err(|err| {
                    error!("Failed to get devices {err}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })
        }
        ("tag", false) => {
            sqlx::query_as!(types::LeanDevice, r#"SELECT
                d.id,
                d.serial_number,
                d.last_ping as last_seen,
                d.approved,
                release_id = target_release_id as up_to_date,
                d.ip_address_id
                FROM device d
                JOIN tag_device td ON d.id = td.device_id
                JOIN tag t ON td.tag_id = t.id
                WHERE t.name = $1 AND d.archived = false
                LIMIT $2"#, filter_value, limit)
            .fetch_all(&state.pg_pool)
            .await
            .map_err(|err| {
                error!("Failed to get devices {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })
        }
        ("distro", false) => {
            sqlx::query_as!(types::LeanDevice, r#"
                SELECT d.id, d.serial_number, d.last_ping as last_seen, d.approved,
                release_id = target_release_id as up_to_date, d.ip_address_id
                FROM device d
                LEFT JOIN release r ON r.id = d.release_id
                LEFT JOIN distribution dist ON r.distribution_id = dist.id
                WHERE dist.name = $1 AND d.archived = false
                LIMIT $2"#, filter_value, limit)
            .fetch_all(&state.pg_pool)
            .await
            .map_err(|err| {
                error!("Failed to get devices {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })
        }
        ("distro", true) => {
            sqlx::query_as!(types::LeanDevice, r#"
                SELECT d.id, d.serial_number, d.last_ping as last_seen, d.approved,
                release_id = target_release_id as up_to_date, d.ip_address_id
                FROM device d
                LEFT JOIN release r ON r.id = d.release_id
                LEFT JOIN distribution dist ON r.distribution_id = dist.id
                WHERE dist.name != $1 AND d.archived = false
                ORDER BY d.id DESC
                LIMIT $2"#, filter_value, limit)
            .fetch_all(&state.pg_pool)
            .await
            .map_err(|err| {
                error!("Failed to get devices {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })
        }
        ("release", false) => {
            sqlx::query_as!(types::LeanDevice, r#"
                SELECT d.id, d.serial_number, d.last_ping as last_seen, d.approved,
                release_id = target_release_id as up_to_date, d.ip_address_id
                FROM device d
                LEFT JOIN release r ON r.id = d.release_id
                WHERE r.version = $1 AND d.archived = false
                LIMIT $2"#
            , filter_value, limit)
            .fetch_all(&state.pg_pool)
            .await
            .map_err(|err| {
                error!("Failed to get devices {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })
        }
        ("release", true) => {
            sqlx::query_as!(types::LeanDevice, r#"
                SELECT d.id, d.serial_number, d.last_ping as last_seen, d.approved,
                release_id = target_release_id as up_to_date, d.ip_address_id
                FROM device d
                LEFT JOIN release r ON r.id = d.release_id
                WHERE r.version != $1 AND d.archived = false
                ORDER BY d.id DESC
                LIMIT $2"#
            , filter_value, limit)
            .fetch_all(&state.pg_pool)
            .await
            .map_err(|err| {
                error!("Failed to get devices {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })
        }
        ("online", _) => {
            let value = filter_value.parse::<bool>().unwrap_or(false);
            let is_online = if reverse { !value } else { value };

            let query = if is_online {
                r#"
                SELECT d.id, d.serial_number, d.last_ping as last_seen, d.approved,
                release_id = target_release_id as up_to_date, d.ip_address_id
                FROM device d
                WHERE d.last_ping >= now() - INTERVAL '5 min'
                AND d.archived = false
                LIMIT $1"#
            } else {
                r#"
                SELECT d.id, d.serial_number, d.last_ping as last_seen, d.approved,
                release_id = target_release_id as up_to_date, d.ip_address_id
                FROM device d
                WHERE d.last_ping < now() - INTERVAL '5 min'
                AND d.archived = false
                LIMIT $1"#
            };

            sqlx::query_as::<_, types::LeanDevice>(query)
                  .bind(limit)
                  .fetch_all(&state.pg_pool)
                  .await
                  .map_err(|err| {
                      error!("Failed to get devices {err}");
                      StatusCode::INTERNAL_SERVER_ERROR
                  })
        }
        ("updated", _) => {
            let value = filter_value.parse::<bool>().unwrap_or(false);
            let is_updated = if reverse { !value } else { value };

            let query = if is_updated {
                r#"
                SELECT d.id, d.serial_number, d.last_ping as last_seen, d.approved,
                release_id = target_release_id as up_to_date, d.ip_address_id
                FROM device d
                WHERE release_id = target_release_id
                AND d.archived = false
                LIMIT $1"#
            } else {
                r#"
                SELECT d.id, d.serial_number, d.last_ping as last_seen, d.approved,
                release_id = target_release_id as up_to_date, d.ip_address_id
                FROM device d
                WHERE release_id != target_release_id
                AND d.archived = false
                LIMIT $1"#
            };

            sqlx::query_as::<_, types::LeanDevice>(query)
                  .bind(limit)
                  .fetch_all(&state.pg_pool)
                  .await
                  .map_err(|err| {
                      error!("Failed to get devices {err}");
                      StatusCode::INTERNAL_SERVER_ERROR
                  })
        }
        _ => Err(StatusCode::BAD_REQUEST),
    }?;

    Ok(Json(types::LeanResponse {
        limit,
        reverse,
        devices,
    }))
}

#[utoipa::path(
    get,
    path = "/devices",
    responses(
        (status = StatusCode::OK, description = "List of devices retrieved successfully", body = Vec<Device>),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve devices"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_devices(
    Extension(state): Extension<State>,
    filter: Query<DeviceFilter>,
) -> Result<Json<Vec<Device>>, StatusCode> {
    debug!("Getting devices {:?}", filter);

    if let Some(tag) = &filter.tag {
        let devices = sqlx::query!(
            r#"SELECT
                d.id,
                d.serial_number,
                d.note,
                d.last_ping as last_seen,
                d.created_on,
                d.approved,
                d.token IS NOT NULL as has_token,
                d.release_id,
                d.target_release_id,
                d.system_info,
                d.modem_id,
                d.ip_address_id,
                ip.id as "ip_id?",
                ip.ip_address as "ip_address?",
                ip.name as "ip_name?",
                ip.continent as "ip_continent?",
                ip.continent_code as "ip_continent_code?",
                ip.country_code as "ip_country_code?",
                ip.country as "ip_country?",
                ip.region as "ip_region?",
                ip.city as "ip_city?",
                ip.isp as "ip_isp?",
                ip.coordinates[0] as "ip_longitude?",
                ip.coordinates[1] as "ip_latitude?",
                ip.proxy as "ip_proxy?",
                ip.hosting as "ip_hosting?",
                ip.created_at as "ip_created_at?",
                ip.updated_at as "ip_updated_at?",
                m.id as "modem_id_nested?",
                m.imei as "modem_imei?",
                m.network_provider as "modem_network_provider?",
                m.updated_at as "modem_updated_at?",
                m.created_at as "modem_created_at?",
                r.id as "release_id_nested?",
                r.distribution_id as "release_distribution_id?",
                rd.architecture as "release_distribution_architecture?",
                rd.name as "release_distribution_name?",
                r.version as "release_version?",
                r.draft as "release_draft?",
                r.yanked as "release_yanked?",
                r.created_at as "release_created_at?",
                r.user_id as "release_user_id?",
                tr.id as "target_release_id_nested?",
                tr.distribution_id as "target_release_distribution_id?",
                trd.architecture as "target_release_distribution_architecture?",
                trd.name as "target_release_distribution_name?",
                tr.version as "target_release_version?",
                tr.draft as "target_release_draft?",
                tr.yanked as "target_release_yanked?",
                tr.created_at as "target_release_created_at?",
                tr.user_id as "target_release_user_id?"
            FROM device d
            JOIN tag_device td ON d.id = td.device_id
            JOIN tag t ON td.tag_id = t.id
            LEFT JOIN ip_address ip ON d.ip_address_id = ip.id
            LEFT JOIN modem m ON d.modem_id = m.id
            LEFT JOIN release r ON d.release_id = r.id
            LEFT JOIN distribution rd ON r.distribution_id = rd.id
            LEFT JOIN release tr ON d.target_release_id = tr.id
            LEFT JOIN distribution trd ON tr.distribution_id = trd.id
            WHERE t.name = $1
            ORDER BY d.serial_number"#,
            tag
        )
        .fetch_all(&state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get devices {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        let devices: Vec<Device> = devices
            .into_iter()
            .map(|row| {
                let ip_address = if row.ip_id.is_some() {
                    let coordinates = match (row.ip_longitude, row.ip_latitude) {
                        (Some(lon), Some(lat)) => Some((lon, lat)),
                        _ => None,
                    };

                    Some(crate::handlers::ip_address::IpAddressInfo {
                        id: row.ip_id.unwrap(),
                        ip_address: row.ip_address.unwrap(),
                        name: row.ip_name,
                        continent: row.ip_continent,
                        continent_code: row.ip_continent_code,
                        country_code: row.ip_country_code,
                        country: row.ip_country,
                        region: row.ip_region,
                        city: row.ip_city,
                        isp: row.ip_isp,
                        coordinates,
                        proxy: row.ip_proxy,
                        hosting: row.ip_hosting,
                        device_count: None,
                        created_at: row.ip_created_at.unwrap(),
                        updated_at: row.ip_updated_at.unwrap(),
                    })
                } else {
                    None
                };

                let modem = if row.modem_id_nested.is_some() {
                    Some(crate::modem::schema::Modem {
                        id: row.modem_id_nested.unwrap(),
                        imei: row.modem_imei.unwrap(),
                        network_provider: row.modem_network_provider.unwrap(),
                        updated_at: row.modem_updated_at.unwrap(),
                        created_at: row.modem_created_at.unwrap(),
                    })
                } else {
                    None
                };

                let release = if row.release_id_nested.is_some() {
                    Some(crate::handlers::distributions::types::Release {
                        id: row.release_id_nested.unwrap(),
                        distribution_id: row.release_distribution_id.unwrap(),
                        distribution_architecture: row.release_distribution_architecture.unwrap(),
                        distribution_name: row.release_distribution_name.unwrap(),
                        version: row.release_version.unwrap(),
                        draft: row.release_draft.unwrap(),
                        yanked: row.release_yanked.unwrap(),
                        created_at: row.release_created_at.unwrap(),
                        user_id: row.release_user_id,
                    })
                } else {
                    None
                };

                let target_release = if row.target_release_id_nested.is_some() {
                    Some(crate::handlers::distributions::types::Release {
                        id: row.target_release_id_nested.unwrap(),
                        distribution_id: row.target_release_distribution_id.unwrap(),
                        distribution_architecture: row
                            .target_release_distribution_architecture
                            .unwrap(),
                        distribution_name: row.target_release_distribution_name.unwrap(),
                        version: row.target_release_version.unwrap(),
                        draft: row.target_release_draft.unwrap(),
                        yanked: row.target_release_yanked.unwrap(),
                        created_at: row.target_release_created_at.unwrap(),
                        user_id: row.target_release_user_id,
                    })
                } else {
                    None
                };

                Device {
                    id: row.id,
                    serial_number: row.serial_number,
                    note: row.note,
                    last_seen: row.last_seen,
                    created_on: row.created_on,
                    approved: row.approved,
                    has_token: row.has_token,
                    release_id: row.release_id,
                    target_release_id: row.target_release_id,
                    system_info: row.system_info,
                    modem_id: row.modem_id,
                    ip_address_id: row.ip_address_id,
                    ip_address,
                    modem,
                    release,
                    target_release,
                }
            })
            .collect();

        return Ok(Json(devices));
    }

    let devices = sqlx::query!(
        r#"SELECT
            d.id,
            d.serial_number,
            d.note,
            d.last_ping as last_seen,
            d.created_on,
            d.approved,
            d.token IS NOT NULL as has_token,
            d.release_id,
            d.target_release_id,
            d.system_info,
            d.modem_id,
            d.ip_address_id,
            ip.id as "ip_id?",
            ip.ip_address as "ip_address?",
            ip.name as "ip_name?",
            ip.continent as "ip_continent?",
            ip.continent_code as "ip_continent_code?",
            ip.country_code as "ip_country_code?",
            ip.country as "ip_country?",
            ip.region as "ip_region?",
            ip.city as "ip_city?",
            ip.isp as "ip_isp?",
            ip.coordinates[0] as "ip_longitude?",
            ip.coordinates[1] as "ip_latitude?",
            ip.proxy as "ip_proxy?",
            ip.hosting as "ip_hosting?",
            ip.created_at as "ip_created_at?",
            ip.updated_at as "ip_updated_at?",
            m.id as "modem_id_nested?",
            m.imei as "modem_imei?",
            m.network_provider as "modem_network_provider?",
            m.updated_at as "modem_updated_at?",
            m.created_at as "modem_created_at?",
            r.id as "release_id_nested?",
            r.distribution_id as "release_distribution_id?",
            rd.architecture as "release_distribution_architecture?",
            rd.name as "release_distribution_name?",
            r.version as "release_version?",
            r.draft as "release_draft?",
            r.yanked as "release_yanked?",
            r.created_at as "release_created_at?",
            r.user_id as "release_user_id?",
            tr.id as "target_release_id_nested?",
            tr.distribution_id as "target_release_distribution_id?",
            trd.architecture as "target_release_distribution_architecture?",
            trd.name as "target_release_distribution_name?",
            tr.version as "target_release_version?",
            tr.draft as "target_release_draft?",
            tr.yanked as "target_release_yanked?",
            tr.created_at as "target_release_created_at?",
            tr.user_id as "target_release_user_id?"
        FROM device d
        LEFT JOIN ip_address ip ON d.ip_address_id = ip.id
        LEFT JOIN modem m ON d.modem_id = m.id
        LEFT JOIN release r ON d.release_id = r.id
        LEFT JOIN distribution rd ON r.distribution_id = rd.id
        LEFT JOIN release tr ON d.target_release_id = tr.id
        LEFT JOIN distribution trd ON tr.distribution_id = trd.id
        WHERE ($1::text IS NULL OR d.serial_number = $1)
          AND ($2::boolean IS NULL OR d.approved = $2)
        ORDER BY d.serial_number"#,
        filter.serial_number,
        filter.approved
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get devices {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let devices: Vec<Device> = devices
        .into_iter()
        .map(|row| {
            let ip_address = if row.ip_id.is_some() {
                let coordinates = match (row.ip_longitude, row.ip_latitude) {
                    (Some(lon), Some(lat)) => Some((lon, lat)),
                    _ => None,
                };

                Some(crate::handlers::ip_address::IpAddressInfo {
                    id: row.ip_id.unwrap(),
                    ip_address: row.ip_address.unwrap(),
                    name: row.ip_name,
                    continent: row.ip_continent,
                    continent_code: row.ip_continent_code,
                    country_code: row.ip_country_code,
                    country: row.ip_country,
                    region: row.ip_region,
                    city: row.ip_city,
                    isp: row.ip_isp,
                    coordinates,
                    proxy: row.ip_proxy,
                    hosting: row.ip_hosting,
                    device_count: None,
                    created_at: row.ip_created_at.unwrap(),
                    updated_at: row.ip_updated_at.unwrap(),
                })
            } else {
                None
            };

            let modem = if row.modem_id_nested.is_some() {
                Some(crate::modem::schema::Modem {
                    id: row.modem_id_nested.unwrap(),
                    imei: row.modem_imei.unwrap(),
                    network_provider: row.modem_network_provider.unwrap(),
                    updated_at: row.modem_updated_at.unwrap(),
                    created_at: row.modem_created_at.unwrap(),
                })
            } else {
                None
            };

            let release = if row.release_id_nested.is_some() {
                Some(crate::handlers::distributions::types::Release {
                    id: row.release_id_nested.unwrap(),
                    distribution_id: row.release_distribution_id.unwrap(),
                    distribution_architecture: row.release_distribution_architecture.unwrap(),
                    distribution_name: row.release_distribution_name.unwrap(),
                    version: row.release_version.unwrap(),
                    draft: row.release_draft.unwrap(),
                    yanked: row.release_yanked.unwrap(),
                    created_at: row.release_created_at.unwrap(),
                    user_id: row.release_user_id,
                })
            } else {
                None
            };

            let target_release = if row.target_release_id_nested.is_some() {
                Some(crate::handlers::distributions::types::Release {
                    id: row.target_release_id_nested.unwrap(),
                    distribution_id: row.target_release_distribution_id.unwrap(),
                    distribution_architecture: row
                        .target_release_distribution_architecture
                        .unwrap(),
                    distribution_name: row.target_release_distribution_name.unwrap(),
                    version: row.target_release_version.unwrap(),
                    draft: row.target_release_draft.unwrap(),
                    yanked: row.target_release_yanked.unwrap(),
                    created_at: row.target_release_created_at.unwrap(),
                    user_id: row.target_release_user_id,
                })
            } else {
                None
            };

            Device {
                id: row.id,
                serial_number: row.serial_number,
                note: row.note,
                last_seen: row.last_seen,
                created_on: row.created_on,
                approved: row.approved,
                has_token: row.has_token,
                release_id: row.release_id,
                target_release_id: row.target_release_id,
                system_info: row.system_info,
                modem_id: row.modem_id,
                ip_address_id: row.ip_address_id,
                ip_address,
                modem,
                release,
                target_release,
            }
        })
        .collect();

    Ok(Json(devices))
}

#[utoipa::path(
    get,
    path = "/devices/tags",
    responses(
        (status = 200, description = "List of all device tags", body = Vec<types::Tag>),
        (status = 500, description = "Failed to retrieve tags", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_tags(
    Extension(state): Extension<State>,
) -> Result<Json<Vec<types::Tag>>, StatusCode> {
    let tags = sqlx::query_as!(
        types::Tag,
        r#"SELECT
            t.id,
            td.device_id as device,
            t.name,
            t.color
        FROM tag t
        JOIN tag_device td ON t.id = td.tag_id
        ORDER BY t.id"#
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get tags {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(tags))
}

#[utoipa::path(
    get,
    path = "/devices/variables",
    responses(
        (status = 200, description = "List of all device variables", body = Vec<types::Variable>),
        (status = 500, description = "Failed to retrieve variables", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_variables(
    Extension(state): Extension<State>,
) -> Result<Json<Vec<types::Variable>>, StatusCode> {
    let variables = sqlx::query_as!(
        types::Variable,
        r#"SELECT
            id,
            device,
            name,
            value
        FROM variable
        ORDER BY device, name"#
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get variables {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(variables))
}

#[utoipa::path(
    get,
    path = "/devices/{device_id}/tags",
    responses(
        (status = 200, description = "List of tags for device", body = Vec<types::Tag>),
        (status = 500, description = "Failed to retrieve tags", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_tag_for_device(
    Path(device_id): Path<String>,
    Extension(state): Extension<State>,
) -> Result<Json<Vec<types::Tag>>, StatusCode> {
    debug!("Getting tags for device {}", device_id);
    let tags = sqlx::query_as!(
        types::Tag,
        r#"SELECT
            t.id,
            td.device_id as device,
            t.name,
            t.color
        FROM tag t
        JOIN tag_device td ON t.id = td.tag_id
        JOIN device d ON td.device_id = d.id
        WHERE
            CASE
                WHEN $1 ~ '^[0-9]+$' AND length($1) <= 10 THEN
                    d.id = $1::int4
                ELSE
                    d.serial_number = $1
            END
        ORDER BY t.id"#,
        device_id
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get tags for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(tags))
}

#[utoipa::path(
    get,
    path = "/devices/{device_id}/health",
    responses(
        (status = 200, description = "Device health status", body = Vec<DeviceHealth>),
        (status = 404, description = "Device not found", body = String),
        (status = 500, description = "Failed to retrieve device", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_health_for_device(
    Path(device_id): Path<String>,
    Extension(state): Extension<State>,
) -> Result<Json<DeviceHealth>, StatusCode> {
    let device_health = sqlx::query_as!(
        DeviceHealth,
        r#"
        SELECT
        id,
        serial_number,
        last_ping,
        CASE
        WHEN last_ping > NOW() - INTERVAL '5 minutes'
        THEN true
        ELSE false
        END AS is_healthy
        FROM device
        WHERE
            CASE
                WHEN $1 ~ '^[0-9]+$' AND length($1) <= 10 THEN
                    id = $1::int4
                ELSE
                    serial_number = $1
            END
        "#,
        device_id
    )
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get device: {}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let device_health = device_health.ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(device_health))
}

#[utoipa::path(
    delete,
    path = "/devices/{device_id}/tags/{tag_id}",
    responses(
        (status = 204, description = "Tag deleted successfully"),
        (status = 500, description = "Failed to delete tag", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn delete_tag_from_device(
    Path((device_id, tag_id)): Path<(i32, i32)>,
    Extension(state): Extension<State>,
) -> Result<StatusCode, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    sqlx::query!(
        r#"DELETE FROM tag_device WHERE device_id = $1 AND tag_id = $2"#,
        device_id,
        tag_id
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to delete tag for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    sqlx::query!(
        r#"INSERT INTO ledger (device_id, "class", "text") VALUES ($1, $2, $3)"#,
        device_id,
        "tag",
        format!("Deleted tag from device.")
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to insert ledger entry for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    put,
    path = "/devices/{device_id}/tags/{tag_id}",
    responses(
        (status = 201, description = "Tag added successfully"),
        (status = 304, description = "Tag already exists"),
        (status = 500, description = "Failed to add tag", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn add_tag_to_device(
    Path((device_id, tag_id)): Path<(i32, i32)>,
    Extension(state): Extension<State>,
) -> Result<StatusCode, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let result = sqlx::query!(
        r#"INSERT INTO tag_device (device_id, tag_id) VALUES ($1, $2)
        ON CONFLICT (device_id, tag_id) DO NOTHING"#,
        device_id,
        tag_id
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to add tag to device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if result.rows_affected() == 0 {
        return Ok(StatusCode::NOT_MODIFIED);
    }

    sqlx::query!(
        r#"INSERT INTO ledger (device_id, "class", "text") VALUES ($1, $2, $3)"#,
        device_id,
        "tag",
        format!("Added tag to device.")
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to insert ledger entry for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::CREATED)
}

#[utoipa::path(
    delete,
    path = "/devices/{device_id}/variables/{variable_id}",
    responses(
        (status = 204, description = "Variable deleted successfully"),
        (status = 500, description = "Failed to delete variable", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn delete_variable_from_device(
    Path((device_id, variable_id)): Path<(i32, i32)>,
    Extension(state): Extension<State>,
) -> Result<StatusCode, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let deleted_variable = sqlx::query!(
        r#"DELETE FROM variable WHERE device = $1 AND id = $2 RETURNING name"#,
        device_id,
        variable_id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to delete variable from device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    sqlx::query!(
        r#"INSERT INTO ledger (device_id, "class", "text") VALUES ($1, $2, $3)"#,
        device_id,
        "variable",
        format!("Variable \"{}\" deleted.", deleted_variable.name)
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to insert ledger entry for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    helpers::refresh_device(&state.pg_pool, device_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    put,
    path = "/devices/{device_id}/variables/{variable_id}",
    request_body = types::NewVariable,
    responses(
        (status = 200, description = "Variable updated successfully"),
        (status = 500, description = "Failed to update variable", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn update_variable_for_device(
    Path((device_id, variable_id)): Path<(i32, i32)>,
    Extension(state): Extension<State>,
    Json(variable): Json<types::NewVariable>,
) -> Result<StatusCode, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let result = sqlx::query!(
        r#"UPDATE variable SET name = $1, value = $2 WHERE device = $3 AND id = $4"#,
        variable.name,
        variable.value,
        device_id,
        variable_id
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to update variable for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if result.rows_affected() == 0 {
        return Ok(StatusCode::NOT_MODIFIED);
    }

    sqlx::query!(
        r#"INSERT INTO ledger (device_id, "class", "text") VALUES ($1, $2, $3)"#,
        device_id,
        "variable",
        format!(
            "Variable \"{}\" updated with value \"{}\".",
            variable.name, variable.value
        )
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to insert ledger entry for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    helpers::refresh_device(&state.pg_pool, device_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/devices/{device_id}/variables",
    responses(
        (status = 200, description = "List of variables for device", body = Vec<types::Variable>),
        (status = 500, description = "Failed to retrieve variables", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_variables_for_device(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<Vec<types::Variable>>, StatusCode> {
    let variables = sqlx::query_as!(
        types::Variable,
        r#"SELECT
            id,
            device,
            name,
            value
        FROM variable
        WHERE device = $1
        ORDER BY device, name"#,
        device_id
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get variables for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(variables))
}

#[utoipa::path(
    post,
    path = "/devices/{device_id}/variables",
    request_body = types::NewVariable,
    responses(
        (status = 201, description = "Variable added successfully"),
        (status = 500, description = "Failed to add variable", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn add_variable_to_device(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
    Json(variable): Json<types::NewVariable>,
) -> Result<StatusCode, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let result = sqlx::query!(
        r#"INSERT INTO variable (name, value, device) VALUES ($1, $2, $3)"#,
        variable.name,
        variable.value,
        device_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to insert variable for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if result.rows_affected() == 0 {
        return Ok(StatusCode::NOT_MODIFIED);
    }

    sqlx::query!(
        r#"INSERT INTO ledger (device_id, "class", "text") VALUES ($1, $2, $3)"#,
        device_id,
        "variable",
        format!(
            "Variable \"{}\" added with value \"{}\".",
            variable.name, variable.value
        )
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to insert ledger entry for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    helpers::refresh_device(&state.pg_pool, device_id).await?;

    Ok(StatusCode::CREATED)
}

#[utoipa::path(
    put,
    path = "/devices/{device_id}/note",
    request_body = types::Note,
    responses(
        (status = 200, description = "Note updated successfully"),
        (status = 304, description = "Note not modified"),
        (status = 500, description = "Failed to update note", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn update_note_for_device(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
    Json(note): Json<types::Note>,
) -> Result<StatusCode, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let result = sqlx::query!(
        r#"UPDATE device SET note = $1 WHERE id = $2"#,
        note.note,
        device_id
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to update note for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if result.rows_affected() == 0 {
        return Ok(StatusCode::NOT_MODIFIED);
    }

    sqlx::query!(
        r#"INSERT INTO ledger (device_id, "class", "text") VALUES ($1, $2, $3)"#,
        device_id,
        "note",
        note.note
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to insert ledger entry for device note {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::NO_CONTENT)
}

#[allow(clippy::collapsible_else_if)]
#[utoipa::path(
    get,
    path = "/devices/{device_id}/ledger",
    responses(
        (status = 200, description = "Device ledger entries", body = types::DeviceLedgerItemPaginated),
        (status = 400, description = "Invalid pagination parameters"),
        (status = 500, description = "Failed to retrieve device ledger", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_ledger_for_device(
    host: Host,
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
    pagination: Query<PaginationId>,
) -> Result<Json<types::DeviceLedgerItemPaginated>, StatusCode> {
    if pagination.starting_after.is_some() && pagination.ending_before.is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let limit = pagination.limit.unwrap_or(5).clamp(0, 5);

    let mut ledger = if let Some(starting_after) = pagination.starting_after {
        sqlx::query_as!(
            types::DeviceLedgerItem,
            r#"SELECT id, timestamp, "class", "text" FROM ledger
            WHERE device_id = $1
                AND id < $2
            ORDER BY timestamp DESC
            LIMIT $3::int"#,
            device_id,
            starting_after,
            limit
        )
        .fetch_all(&mut *tx)
        .await
    } else if let Some(ending_before) = pagination.ending_before {
        sqlx::query_as!(
            types::DeviceLedgerItem,
            r#"SELECT id, timestamp, "class", "text" FROM ledger
            WHERE device_id = $1
                AND id > $2
            ORDER BY timestamp ASC
            LIMIT $3::int"#,
            device_id,
            ending_before,
            limit
        )
        .fetch_all(&mut *tx)
        .await
    } else {
        sqlx::query_as!(
            types::DeviceLedgerItem,
            r#"SELECT id, timestamp, "class", "text" FROM ledger
            WHERE device_id = $1
            ORDER BY timestamp DESC
            LIMIT $2::int"#,
            device_id,
            limit
        )
        .fetch_all(&mut *tx)
        .await
    }
    .map_err(|err| {
        error!("Failed to fetch device ledger {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Sort by timestamp (most recent first).
    ledger.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let first_id = ledger.first().map(|t| t.id);
    let last_id = ledger.last().map(|t| t.id);

    let has_more_first_id = if let Some(first_id) = first_id {
        let more = sqlx::query_scalar!(
            r#"select exists(
                select 1 from ledger
                where
                    device_id = $1
                    and id > $2
                order by timestamp asc
                limit 1
            )"#,
            device_id,
            first_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to check if there is more telemetry {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        more.unwrap_or(false)
    } else {
        false
    };

    let has_more_last_id = if let Some(last_id) = last_id {
        let more = sqlx::query_scalar!(
            r#"select exists(
                select 1 from ledger
                where
                    device_id = $1
                    and id < $2
                order by timestamp desc
                limit 1
            )"#,
            device_id,
            last_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to check if there is more telemetry {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        more.unwrap_or(false)
    } else {
        false
    };

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let next = if has_more_last_id {
        Some(format!(
            "https://{}/devices/{}/ledger?starting_after={}&limit={}",
            host.0,
            device_id,
            last_id.expect("error: last telemetry id is None"),
            limit
        ))
    } else {
        None
    };
    let previous = if has_more_first_id {
        Some(format!(
            "https://{}/devices/{}/ledger?ending_before={}&limit={}",
            host.0,
            device_id,
            first_id.expect("error: first telemetry id is None"),
            limit
        ))
    } else {
        None
    };

    let ledger_paginated = types::DeviceLedgerItemPaginated {
        ledger,
        next,
        previous,
    };

    Ok(Json(ledger_paginated))
}

#[derive(Deserialize, Debug)]
pub struct PaginationId {
    pub starting_after: Option<i32>,
    pub ending_before: Option<i32>,
    pub limit: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/devices/{device_id}/commands",
    responses(
        (status = StatusCode::OK, description = "Command successfully fetch from to the device"),
        (status = StatusCode::NOT_FOUND, description = "Device not found"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to fetch device commands"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
#[allow(clippy::collapsible_else_if)]
#[tracing::instrument]
pub async fn get_all_commands_for_device(
    host: Host,
    Path(device_id): Path<String>,
    Extension(state): Extension<State>,
    pagination: Query<PaginationId>,
) -> Result<Json<types::CommandsPaginated>, StatusCode> {
    if pagination.starting_after.is_some() && pagination.ending_before.is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let device = sqlx::query!(
        "
        SELECT id
        FROM device
        WHERE
            CASE
                WHEN $1 ~ '^[0-9]+$' AND length($1) <= 10 THEN
                    id = $1::int4
                ELSE
                    serial_number = $1
            END
        ",
        device_id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to fetch device id {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let device_id = match device {
        Some(device) => device.id,
        None => {
            return Err(StatusCode::NOT_FOUND);
        }
    };

    let limit = pagination.limit.unwrap_or(5).clamp(0, 5);

    let mut commands = if let Some(starting_after) = pagination.starting_after {
        sqlx::query_as!(
            types::DeviceCommandResponse,
            r#"SELECT
                cq.device_id as device,
                d.serial_number,
                cq.id as cmd_id,
                cq.created_at as issued_at,
                cq.cmd as cmd_data,
                cq.canceled as cancelled,
                cq.fetched,
                cq.fetched_at,
                cr.id as "response_id?",
                cr.created_at as "response_at?",
                cr.response as "response?",
                cr.status as "status?"
            FROM command_queue cq
            LEFT JOIN command_response cr ON cq.id = cr.command_id
            LEFT JOIN device d ON cq.device_id = d.id
            WHERE cq.device_id = $1
                AND cq.id < $2
            ORDER BY cq.created_at DESC
            LIMIT $3::int"#,
            device_id,
            starting_after,
            limit
        )
        .fetch_all(&mut *tx)
        .await
    } else if let Some(ending_before) = pagination.ending_before {
        sqlx::query_as!(
            types::DeviceCommandResponse,
            r#"SELECT
                cq.device_id as device,
                d.serial_number,
                cq.id as cmd_id,
                cq.created_at as issued_at,
                cq.cmd as cmd_data,
                cq.canceled as cancelled,
                cq.fetched,
                cq.fetched_at,
                cr.id as "response_id?",
                cr.created_at as "response_at?",
                cr.response as "response?",
                cr.status as "status?"
            FROM command_queue cq
            LEFT JOIN command_response cr ON cq.id = cr.command_id
            LEFT JOIN device d ON cq.device_id = d.id
            WHERE cq.device_id = $1
                AND cq.id > $2
            ORDER BY cq.created_at ASC
            LIMIT $3::int"#,
            device_id,
            ending_before,
            limit
        )
        .fetch_all(&mut *tx)
        .await
    } else {
        sqlx::query_as!(
            types::DeviceCommandResponse,
            r#"SELECT
                cq.device_id as device,
                d.serial_number,
                cq.id as cmd_id,
                cq.created_at as issued_at,
                cq.cmd as cmd_data,
                cq.canceled as cancelled,
                cq.fetched,
                cq.fetched_at,
                cr.id as "response_id?",
                cr.created_at as "response_at?",
                cr.response as "response?",
                cr.status as "status?"
            FROM command_queue cq
            LEFT JOIN command_response cr ON cq.id = cr.command_id
            LEFT JOIN device d ON cq.device_id = d.id
            WHERE cq.device_id = $1
            ORDER BY cq.created_at DESC
            LIMIT $2::int"#,
            device_id,
            limit
        )
        .fetch_all(&mut *tx)
        .await
    }
    .map_err(|err| {
        error!("Failed to get commands for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Sort by timestamp (most recent first).
    commands.sort_by(|a, b| b.issued_at.cmp(&a.issued_at));

    let first_id = commands.first().map(|c| c.cmd_id);
    let last_id = commands.last().map(|c| c.cmd_id);

    let has_more_first_id = if let Some(first_id) = first_id {
        let more = sqlx::query_scalar!(
            r#"select exists(
                select 1 from command_queue
                where
                    device_id = $1
                    and id > $2
                order by created_at asc
                limit 1
            )"#,
            device_id,
            first_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to check if there is more telemetry {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        more.unwrap_or(false)
    } else {
        false
    };

    let has_more_last_id = if let Some(last_id) = last_id {
        let more = sqlx::query_scalar!(
            r#"select exists(
                select 1 from command_queue
                where
                    device_id = $1
                    and id < $2
                order by created_at desc
                limit 1
            )"#,
            device_id,
            last_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to check if there is more telemetry {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        more.unwrap_or(false)
    } else {
        false
    };

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let next = if has_more_last_id {
        Some(format!(
            "https://{}/devices/{}/commands?starting_after={}&limit={}",
            host.0,
            device_id,
            last_id.unwrap(),
            limit
        ))
    } else {
        None
    };

    let previous = if has_more_first_id {
        Some(format!(
            "https://{}/devices/{}/commands?ending_before={}&limit={}",
            host.0,
            device_id,
            first_id.unwrap(),
            limit
        ))
    } else {
        None
    };

    let commands_paginated = types::CommandsPaginated {
        commands,
        next,
        previous,
    };

    Ok(Json(commands_paginated))
}

#[utoipa::path(
    get,
    path = "/devices/{device_id}/release",
    responses(
        (status = 200, description = "Device release information", body = types::DeviceRelease),
        (status = 500, description = "Failed to retrieve device release", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_device_release(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<types::DeviceRelease>, StatusCode> {
    let release = sqlx::query_as!(
        Release,
        "
        SELECT release.*,
        distribution.name AS distribution_name,
        distribution.architecture AS distribution_architecture
        FROM device
        LEFT JOIN release ON device.release_id = release.id
        JOIN distribution ON release.distribution_id = distribution.id
        WHERE device.id = $1
        ",
        device_id
    )
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get device release {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let previous_release = if let Some(ref current_release) = release {
        sqlx::query_as!(
            Release,
            "
        SELECT release.*,
        distribution.name AS distribution_name,
        distribution.architecture AS distribution_architecture
        FROM device_release_upgrades
        JOIN release ON release.id = device_release_upgrades.previous_release_id
        JOIN distribution ON release.distribution_id = distribution.id
        WHERE device_release_upgrades.device_id = $1
        AND device_release_upgrades.upgraded_release_id = $2
        ",
            device_id,
            current_release.id
        )
        .fetch_optional(&state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get device release {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
    } else {
        None
    };

    let target_release = sqlx::query_as!(
        Release,
        "
        SELECT release.*,
        distribution.name AS distribution_name,
        distribution.architecture AS distribution_architecture
        FROM device
        LEFT JOIN release ON device.target_release_id = release.id
        JOIN distribution ON release.distribution_id = distribution.id
        WHERE device.id = $1
        ",
        device_id
    )
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get device release {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let device_release: types::DeviceRelease = types::DeviceRelease {
        previous_release,
        release,
        target_release,
    };

    Ok(Json(device_release))
}

#[utoipa::path(
    post,
    path = "/devices/{device_id}/release",
    request_body = types::UpdateDeviceRelease,
    responses(
        (status = 200, description = "Device target release updated successfully"),
        (status = 404, description = "Release not found"),
        (status = 500, description = "Failed to update device target release", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn update_device_target_release(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
    Json(device_release): Json<types::UpdateDeviceRelease>,
) -> Result<StatusCode, StatusCode> {
    let target_release_id = device_release.target_release_id;
    let releases = sqlx::query!(
        "SELECT COUNT(*) FROM release WHERE id = $1",
        target_release_id
    )
    .fetch_one(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to check if release exists: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if releases.count == Some(0) {
        error!("Release {target_release_id} not found");
        return Err(StatusCode::NOT_FOUND);
    }

    sqlx::query!(
        "UPDATE device SET target_release_id = $1 WHERE id = $2",
        target_release_id,
        device_id
    )
    .execute(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to update target release id for device {device_id}; {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::OK)
}

#[utoipa::path(
    put,
    path = "/devices/release",
    request_body = types::UpdateDevicesRelease,
    responses(
        (status = 200, description = "Devices target release updated successfully"),
        (status = 404, description = "Release not found"),
        (status = 500, description = "Failed to update devices target release", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn update_devices_target_release(
    Extension(state): Extension<State>,
    Json(devices_release): Json<types::UpdateDevicesRelease>,
) -> Result<StatusCode, StatusCode> {
    let target_release_id = devices_release.target_release_id;
    let release = db_get_release_by_id(target_release_id, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get target release: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if release.is_none() {
        error!("Release {target_release_id} not found");
        return Err(StatusCode::NOT_FOUND);
    }

    let target_release = release.unwrap();
    if target_release.yanked || target_release.draft {
        return Err(StatusCode::CONFLICT);
    }

    sqlx::query!(
        "UPDATE device SET target_release_id = $1 WHERE id = ANY($2)",
        target_release_id,
        &devices_release.devices
    )
    .execute(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to update target release id for devices; {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::OK)
}

#[utoipa::path(
    post,
    path = "/devices/{device_id}/commands",
    responses(
        (status = StatusCode::CREATED, description = "Command successfully issue to device"),
        (status = StatusCode::NOT_FOUND, description = "Device not found"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to issue command to device"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn issue_commands_to_device(
    Path(device_id): Path<String>,
    Extension(state): Extension<State>,
    Json(commands): Json<Vec<SafeCommandRequest>>,
) -> Result<StatusCode, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let device = sqlx::query!(
        "
        SELECT id
        FROM device
        WHERE
            CASE
                WHEN $1 ~ '^[0-9]+$' AND length($1) <= 10 THEN
                    id = $1::int4
                ELSE
                    serial_number = $1
            END
        ",
        device_id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to fetch device id {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let device_id = match device {
        Some(device) => device.id,
        None => {
            return Err(StatusCode::NOT_FOUND);
        }
    };

    let bundle_id = sqlx::query!("INSERT INTO command_bundles DEFAULT VALUES RETURNING uuid")
        .fetch_one(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to insert command bundle {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    for command in commands {
        sqlx::query!(
            "INSERT INTO command_queue (device_id, cmd, continue_on_error, canceled, bundle)
            VALUES ($1, $2::jsonb, $3, false, $4)",
            device_id,
            serde_json::to_value(command.command)
                .expect("error: failed to serialize device command"),
            command.continue_on_error,
            bundle_id.uuid
        )
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to insert command for device {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::CREATED)
}

#[utoipa::path(
    get,
    path = "/devices/{device_id}",
    responses(
        (status = StatusCode::OK, description = "Return found device", body = Device),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve device"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_device_info(
    Path(device_id): Path<String>,
    Extension(state): Extension<State>,
) -> Result<Json<Device>, StatusCode> {
    let device_row = sqlx::query!(
        "
        SELECT
        d.id,
        d.serial_number,
        d.note,
        d.last_ping as last_seen,
        d.created_on,
        d.approved,
        d.token IS NOT NULL as has_token,
        d.release_id,
        d.target_release_id,
        d.system_info,
        d.modem_id,
        d.ip_address_id,
        ip.id as \"ip_id?\",
        ip.ip_address as \"ip_address?\",
        ip.name as \"ip_name?\",
        ip.continent as \"ip_continent?\",
        ip.continent_code as \"ip_continent_code?\",
        ip.country_code as \"ip_country_code?\",
        ip.country as \"ip_country?\",
        ip.region as \"ip_region?\",
        ip.city as \"ip_city?\",
        ip.isp as \"ip_isp?\",
        ip.coordinates[0] as \"ip_longitude?\",
        ip.coordinates[1] as \"ip_latitude?\",
        ip.proxy as \"ip_proxy?\",
        ip.hosting as \"ip_hosting?\",
        ip.created_at as \"ip_created_at?\",
        ip.updated_at as \"ip_updated_at?\",
        m.id as \"modem_id_nested?\",
        m.imei as \"modem_imei?\",
        m.network_provider as \"modem_network_provider?\",
        m.updated_at as \"modem_updated_at?\",
        m.created_at as \"modem_created_at?\",
        r.id as \"release_id_nested?\",
        r.distribution_id as \"release_distribution_id?\",
        rd.architecture as \"release_distribution_architecture?\",
        rd.name as \"release_distribution_name?\",
        r.version as \"release_version?\",
        r.draft as \"release_draft?\",
        r.yanked as \"release_yanked?\",
        r.created_at as \"release_created_at?\",
        r.user_id as \"release_user_id?\",
        tr.id as \"target_release_id_nested?\",
        tr.distribution_id as \"target_release_distribution_id?\",
        trd.architecture as \"target_release_distribution_architecture?\",
        trd.name as \"target_release_distribution_name?\",
        tr.version as \"target_release_version?\",
        tr.draft as \"target_release_draft?\",
        tr.yanked as \"target_release_yanked?\",
        tr.created_at as \"target_release_created_at?\",
        tr.user_id as \"target_release_user_id?\"
        FROM device d
        LEFT JOIN ip_address ip ON d.ip_address_id = ip.id
        LEFT JOIN modem m ON d.modem_id = m.id
        LEFT JOIN release r ON d.release_id = r.id
        LEFT JOIN distribution rd ON r.distribution_id = rd.id
        LEFT JOIN release tr ON d.target_release_id = tr.id
        LEFT JOIN distribution trd ON tr.distribution_id = trd.id
        WHERE
            CASE
                WHEN $1 ~ '^[0-9]+$' AND length($1) <= 10 THEN
                    d.id = $1::int4
                ELSE
                    d.serial_number = $1
            END
        ",
        device_id
    )
    .fetch_one(&state.pg_pool)
    .await
    .map_err(|err| {
        error!(
            serial_number = device_id,
            "Failed to fetch device info {err}"
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let ip_address = if device_row.ip_id.is_some() {
        let coordinates = match (device_row.ip_longitude, device_row.ip_latitude) {
            (Some(lon), Some(lat)) => Some((lon, lat)),
            _ => None,
        };

        Some(crate::handlers::ip_address::IpAddressInfo {
            id: device_row.ip_id.unwrap(),
            ip_address: device_row.ip_address.unwrap(),
            name: device_row.ip_name,
            continent: device_row.ip_continent,
            continent_code: device_row.ip_continent_code,
            country_code: device_row.ip_country_code,
            country: device_row.ip_country,
            region: device_row.ip_region,
            city: device_row.ip_city,
            isp: device_row.ip_isp,
            coordinates,
            proxy: device_row.ip_proxy,
            hosting: device_row.ip_hosting,
            device_count: None,
            created_at: device_row.ip_created_at.unwrap(),
            updated_at: device_row.ip_updated_at.unwrap(),
        })
    } else {
        None
    };

    let modem = if device_row.modem_id_nested.is_some() {
        Some(crate::modem::schema::Modem {
            id: device_row.modem_id_nested.unwrap(),
            imei: device_row.modem_imei.unwrap(),
            network_provider: device_row.modem_network_provider.unwrap(),
            updated_at: device_row.modem_updated_at.unwrap(),
            created_at: device_row.modem_created_at.unwrap(),
        })
    } else {
        None
    };

    let release = if device_row.release_id_nested.is_some() {
        Some(crate::handlers::distributions::types::Release {
            id: device_row.release_id_nested.unwrap(),
            distribution_id: device_row.release_distribution_id.unwrap(),
            distribution_architecture: device_row.release_distribution_architecture.unwrap(),
            distribution_name: device_row.release_distribution_name.unwrap(),
            version: device_row.release_version.unwrap(),
            draft: device_row.release_draft.unwrap(),
            yanked: device_row.release_yanked.unwrap(),
            created_at: device_row.release_created_at.unwrap(),
            user_id: device_row.release_user_id,
        })
    } else {
        None
    };

    let target_release = if device_row.target_release_id_nested.is_some() {
        Some(crate::handlers::distributions::types::Release {
            id: device_row.target_release_id_nested.unwrap(),
            distribution_id: device_row.target_release_distribution_id.unwrap(),
            distribution_architecture: device_row.target_release_distribution_architecture.unwrap(),
            distribution_name: device_row.target_release_distribution_name.unwrap(),
            version: device_row.target_release_version.unwrap(),
            draft: device_row.target_release_draft.unwrap(),
            yanked: device_row.target_release_yanked.unwrap(),
            created_at: device_row.target_release_created_at.unwrap(),
            user_id: device_row.target_release_user_id,
        })
    } else {
        None
    };

    let device = Device {
        id: device_row.id,
        serial_number: device_row.serial_number,
        note: device_row.note,
        last_seen: device_row.last_seen,
        created_on: device_row.created_on,
        approved: device_row.approved,
        has_token: device_row.has_token,
        release_id: device_row.release_id,
        target_release_id: device_row.target_release_id,
        system_info: device_row.system_info,
        modem_id: device_row.modem_id,
        ip_address_id: device_row.ip_address_id,
        ip_address,
        modem,
        release,
        target_release,
    };

    Ok(Json(device))
}

#[utoipa::path(
    delete,
    path = "/devices/{device_id}",
    responses(
        (status = StatusCode::NO_CONTENT, description = "Successfully deleted the device"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to delete device"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn delete_device(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<StatusCode, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    sqlx::query!("UPDATE device SET archived = true WHERE id = $1", device_id)
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to archive device {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/devices/{device_id}/approval",
    responses(
        (status = 200, description = "Device approved successfully"),
        (status = 500, description = "Failed to approve device", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn approve_device(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<()>, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let response = sqlx::query!(
        r#"UPDATE device SET approved = true WHERE id = $1 RETURNING serial_number"#,
        device_id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to approve device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let approved_serial_number = response.serial_number;

    // get tag for trolley
    let tag_name = "trolley";
    let query = r#"
            INSERT INTO tag (name)
            VALUES ($1)
            ON CONFLICT (name) DO UPDATE
            SET name = EXCLUDED.name
            RETURNING id;
        "#;

    // Execute the query and get the tag id
    let row = sqlx::query(query)
        .bind(tag_name) // Bind the 'trolley' name to the query
        .fetch_one(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to insert tag trolley for device {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let tag_id: i32 = row.try_get("id").map_err(|err| {
        error!("Failed to get tag id for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    sqlx::query!(
        r#"INSERT INTO tag_device (device_id, tag_id) VALUES ($1, $2)"#,
        device_id,
        tag_id
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to insert tag entry for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    sqlx::query!(
        r#"INSERT INTO ledger (device_id, "class", "text") VALUES ($1, $2, $3)"#,
        device_id,
        "approved",
        format!("Device approved.")
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to insert ledger entry for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let msg = PublicEvent::ApprovedDevice {
        serial_number: approved_serial_number,
    };
    let tx_message = state.public_events;
    let guard = tx_message.lock().await;
    (*guard).send(msg).expect("failed to send");

    Ok(Json(()))
}

#[utoipa::path(
    delete,
    path = "/devices/{device_id}/approval",
    responses(
        (status = 200, description = "Device approval revoked successfully"),
        (status = 500, description = "Failed to revoke device approval", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn revoke_device(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<()>, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    sqlx::query!(
        r#"UPDATE device SET approved = false WHERE id = $1"#,
        device_id
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to revoke device approval {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    sqlx::query!(
        r#"INSERT INTO ledger (device_id, "class", "text") VALUES ($1, $2, $3)"#,
        device_id,
        "revoked",
        format!("Device approval revoked.")
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to insert ledger entry for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(()))
}

#[utoipa::path(
    delete,
    path = "/devices/{device_id}/token",
    responses(
        (status = 200, description = "Device token deleted successfully"),
        (status = 500, description = "Failed to delete device token", body = String),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn delete_token(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<()>, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    sqlx::query!(r#"UPDATE device SET token = NULL WHERE id = $1"#, device_id)
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to delete token for device {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    sqlx::query!(
        r#"INSERT INTO ledger (device_id, "class", "text") VALUES ($1, $2, $3)"#,
        device_id,
        "token",
        format!("Token reset.")
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to insert ledger entry for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(()))
}

#[utoipa::path(
    get,
    path = "/devices/{serial_number}/network",
    responses(
        (status = StatusCode::OK, description = "Network retrieved successfully"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve network"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_network_for_device(
    Path(serial_number): Path<String>,
    Extension(state): Extension<State>,
) -> Result<Json<schema::Network>, StatusCode> {
    let tags = sqlx::query_as!(
        schema::Network,
        r#"
        SELECT
            n.id,
            n.network_type::TEXT,
            n.is_network_hidden,
            n.ssid,
            n.name,
            n.description,
            n.password
        FROM network n
        JOIN device d ON n.id = d.network_id
        WHERE d.serial_number = $1"#,
        serial_number
    )
    .fetch_one(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get network for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(tags))
}

#[utoipa::path(
    put,
    path = "/devices/{serial_number}/network",
    responses(
        (status = StatusCode::OK, description = "Successfully updated network"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to update network"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn update_device_network(
    Path(serial_number): Path<String>,
    Extension(state): Extension<State>,
    Json(network_id): Json<i32>,
) -> Result<StatusCode, StatusCode> {
    sqlx::query!(
        "UPDATE device SET network_id = $1 WHERE serial_number = $2",
        network_id,
        serial_number
    )
    .execute(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to update network id for device {serial_number}; {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::OK)
}

#[utoipa::path(
    put,
    path = "/devices/network/{network}",
    responses(
        (status = StatusCode::OK, description = "Successfully updated networks"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to update networks"),
    ),
    security(
        ("Access Token" = [])
    ),
    tag = DEVICES_TAG
)]
/// Batch updates the `network_id` for a list of `serial_number`s.
pub async fn update_devices_network(
    Path(network_id): Path<i32>,
    Extension(state): Extension<State>,
    Json(serial_numbers): Json<Vec<String>>,
) -> Result<StatusCode, StatusCode> {
    let query = r#"
        UPDATE device
        SET network_id = $1
        WHERE serial_number = ANY($2)
    "#;

    sqlx::query(query)
        .bind(network_id)
        .bind(&serial_numbers)
        .execute(&state.pg_pool)
        .await
        .map_err(|err| {
            error!(
                "Failed to update network id for devices {:?}; {err:?}",
                serial_numbers
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::OK)
}
