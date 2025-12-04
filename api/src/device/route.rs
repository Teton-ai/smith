use crate::State;
use crate::device::{
    AuthDevice, CommandsPaginated, Device, DeviceCommandResponse, DeviceHealth, DeviceLedgerItem,
    DeviceLedgerItemPaginated, DeviceNetwork, DeviceRelease, LeanDevice, LeanResponse, NewVariable,
    Note, RawDevice, Tag, UpdateDeviceRelease, UpdateDevicesRelease, Variable,
};
use crate::event::PublicEvent;
use crate::middlewares::authorization;
use crate::modem::Modem;
use crate::release::Release;
use crate::user::CurrentUser;
use axum::extract::Host;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Extension, Json};
use axum_extra::extract::Query;
use serde::Deserialize;
use smith::utils::schema;
use smith::utils::schema::SafeCommandRequest;
use sqlx::types::Json as SqlxJson;
use std::collections::HashMap;
use tracing::{debug, error};

const DEVICE_TAG: &str = "device";
const DEVICES_TAG: &str = "devices";

#[derive(Deserialize, Debug)]
pub struct LeanDeviceFilter {
    reverse: Option<bool>,
    limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/device",
    responses(
        (status = StatusCode::OK, description = "Return Device Information", body = RawDevice),

    ),
    security(
        ("device_token" = [])
    ),
    tag = DEVICE_TAG,
)]
pub async fn get_device(
    Extension(AuthDevice(device)): Extension<AuthDevice>,
) -> axum::response::Result<Json<RawDevice>, StatusCode> {
    Ok(Json(device))
}

// TODO: this is getting crazy huge, maybe it would be nice to have an handler
// per filter type instead of only 1 to handle all, maybe that could also have
// some performance benefits to let axum handle the matching of the arms
#[utoipa::path(
    get,
    path = "/lean/{filter_kind}/{filter_value}",
    responses(
        (status = 200, description = "Filtered devices", body = LeanResponse),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Failed to retrieve devices", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
#[deprecated(
    since = "0.2.65",
    note = "We are moving to `/devices` endpoint and make it support conditional params/filters"
)]
pub async fn get_devices_new(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Path((filter_kind, filter_value)): Path<(String, String)>,
    Query(query_params): Query<LeanDeviceFilter>,
) -> axum::response::Result<Json<LeanResponse>, StatusCode> {
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
      sqlx::query_as!(LeanDevice, "SELECT id, serial_number, last_ping as last_seen, approved, release_id = target_release_id as up_to_date, ip_address_id FROM device WHERE serial_number LIKE '%' || $1 || '%' AND archived = false LIMIT $2", filter_value, limit)
        .fetch_all(&state.pg_pool)
        .await
        .map_err(|err| {
          error!("Failed to get devices {err}");
          StatusCode::INTERNAL_SERVER_ERROR
        })
    },
    ("sn", false) => {
      sqlx::query_as!(LeanDevice, "SELECT id, serial_number, last_ping as last_seen, approved, release_id = target_release_id as up_to_date, ip_address_id FROM device WHERE serial_number LIKE '%' || $1 || '%' AND archived = false LIMIT $2", filter_value, limit)
        .fetch_all(&state.pg_pool)
        .await
        .map_err(|err| {
          error!("Failed to get devices {err}");
          StatusCode::INTERNAL_SERVER_ERROR
        })
    },
    ("approved", true) => {
      let value = filter_value.parse().unwrap_or(false);
      sqlx::query_as!(LeanDevice, "SELECT id, serial_number, last_ping as last_seen, approved, release_id = target_release_id as up_to_date, ip_address_id FROM device WHERE approved != $1 AND archived = false LIMIT $2", value, limit)
        .fetch_all(&state.pg_pool)
        .await
        .map_err(|err| {
          error!("Failed to get devices {err}");
          StatusCode::INTERNAL_SERVER_ERROR
        })
    },
    ("approved", false) => {
      let value = filter_value.parse().unwrap_or(false);
      sqlx::query_as!(LeanDevice, "SELECT id, serial_number, last_ping as last_seen, approved, release_id = target_release_id as up_to_date, ip_address_id FROM device WHERE approved = $1 AND archived = false LIMIT $2", value, limit)
        .fetch_all(&state.pg_pool)
        .await
        .map_err(|err| {
          error!("Failed to get devices {err}");
          StatusCode::INTERNAL_SERVER_ERROR
        })
    }
    ("tag", true) => {
      sqlx::query_as!(LeanDevice, r#"SELECT
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
      sqlx::query_as!(LeanDevice, r#"SELECT
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
      sqlx::query_as!(LeanDevice, r#"
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
      sqlx::query_as!(LeanDevice, r#"
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
      sqlx::query_as!(LeanDevice, r#"
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
      sqlx::query_as!(LeanDevice, r#"
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

      sqlx::query_as::<_, LeanDevice>(query)
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

      sqlx::query_as::<_, LeanDevice>(query)
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

    Ok(Json(LeanResponse {
        limit,
        reverse,
        devices,
    }))
}

/// Query filter for device listing.
#[derive(Deserialize, Debug)]
pub struct DeviceFilter {
    pub serial_number: Option<String>,
    /// Filter by approved status. If None, only approved devices are included by default.
    pub approved: Option<bool>,
    /// Filter by archived status. If None, archived devices are excluded by default.
    pub archived: Option<bool>,
    #[deprecated(
        since = "0.2.64",
        note = "Since labels have been released, tags concept be in version 0.74"
    )]
    pub tag: Option<String>,
    /// Filter by labels. Format: key=value. Multiple labels can be provided.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Filter by online status. If true, only devices online in the last 5 minutes.
    pub online: Option<bool>,
}

#[utoipa::path(
    get,
    path = "/devices",
    responses(
        (status = StatusCode::OK, description = "List of devices retrieved successfully", body = Vec<Device>),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to retrieve devices"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_devices(
    Extension(state): Extension<State>,
    filter: Query<DeviceFilter>,
) -> axum::response::Result<Json<Vec<Device>>, StatusCode> {
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
            tr.user_id as "target_release_user_id?",
            dn.network_score as "network_score?",
            dn.download_speed_mbps as "network_download_speed_mbps?",
            dn.upload_speed_mbps as "network_upload_speed_mbps?",
            dn.source as "network_source?",
            dn.updated_at as "network_updated_at?",
            COALESCE(JSONB_OBJECT_AGG(l.name, dl.value) FILTER (WHERE l.name IS NOT NULL), '{}') as "labels!: SqlxJson<HashMap<String, String>>"
        FROM device d
        LEFT JOIN tag_device td ON d.id = td.device_id AND $4::text IS NOT NULL
        LEFT JOIN tag t ON td.tag_id = t.id AND t.name = $4
        LEFT JOIN ip_address ip ON d.ip_address_id = ip.id
        LEFT JOIN modem m ON d.modem_id = m.id
        LEFT JOIN release r ON d.release_id = r.id
        LEFT JOIN distribution rd ON r.distribution_id = rd.id
        LEFT JOIN release tr ON d.target_release_id = tr.id
        LEFT JOIN distribution trd ON tr.distribution_id = trd.id
        LEFT JOIN device_network dn ON d.id = dn.device_id
        LEFT JOIN device_label dl ON dl.device_id = d.id
        LEFT JOIN label l ON l.id = dl.label_id
        WHERE ($1::text IS NULL OR d.serial_number = $1)
          AND ($2::boolean IS NULL OR d.approved = $2)
          AND (COALESCE($3, false) = true OR d.archived = false)
          AND ($4::text IS NULL OR t.name IS NOT NULL)
          AND (CARDINALITY($5::text[]) = 0 OR l.name || '=' || dl.value = ANY($5))
          AND ($6::boolean IS NULL OR
               ($6 = true AND d.last_ping >= now() - INTERVAL '5 minutes') OR
               ($6 = false AND d.last_ping < now() - INTERVAL '5 minutes'))
        GROUP BY d.id, ip.id, m.id, r.id, rd.id, tr.id, trd.id, dn.device_id
        ORDER BY d.serial_number
        "#,
        filter.serial_number,
        filter.approved,
        filter.archived,
        filter.tag,
        filter.labels.as_slice(),
        filter.online
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

                Some(crate::ip_address::IpAddressInfo {
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
                Some(Modem {
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
                Some(Release {
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
                Some(Release {
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

            let network = if row.network_score.is_some() {
                Some(DeviceNetwork {
                    network_score: row.network_score,
                    download_speed_mbps: row.network_download_speed_mbps,
                    upload_speed_mbps: row.network_upload_speed_mbps,
                    source: row.network_source,
                    updated_at: row.network_updated_at,
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
                network,
                labels: row.labels,
            }
        })
        .collect();

    Ok(Json(devices))
}

#[utoipa::path(
    get,
    path = "/devices/tags",
    responses(
        (status = 200, description = "List of all device tags", body = Vec<Tag>),
        (status = 500, description = "Failed to retrieve tags", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
#[deprecated(
    since = "0.2.64",
    note = "Since labels have been released, tags concept be in version 0.74"
)]
pub async fn get_tags(Extension(state): Extension<State>) -> Result<Json<Vec<Tag>>, StatusCode> {
    let tags = sqlx::query_as!(
        Tag,
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
        (status = 200, description = "List of all device variables", body = Vec<Variable>),
        (status = 500, description = "Failed to retrieve variables", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_variables(
    Extension(state): Extension<State>,
) -> Result<Json<Vec<Variable>>, StatusCode> {
    let variables = sqlx::query_as!(
        Variable,
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
        (status = 200, description = "List of tags for device", body = Vec<Tag>),
        (status = 500, description = "Failed to retrieve tags", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
#[deprecated(
    since = "0.2.64",
    note = "Since labels have been released, tags concept be in version 0.74"
)]
pub async fn get_tag_for_device(
    Path(device_id): Path<String>,
    Extension(state): Extension<State>,
) -> Result<Json<Vec<Tag>>, StatusCode> {
    debug!("Getting tags for device {}", device_id);
    let tags = sqlx::query_as!(
        Tag,
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
        ("auth_token" = [])
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
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
#[deprecated(
    since = "0.2.64",
    note = "Since labels have been released, tags concept be in version 0.74"
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
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
#[deprecated(
    since = "0.2.64",
    note = "Since labels have been released, tags concept be in version 0.74"
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
        ("auth_token" = [])
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

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    put,
    path = "/devices/{device_id}/variables/{variable_id}",
    request_body = NewVariable,
    responses(
        (status = 200, description = "Variable updated successfully"),
        (status = 500, description = "Failed to update variable", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn update_variable_for_device(
    Path((device_id, variable_id)): Path<(i32, i32)>,
    Extension(state): Extension<State>,
    Json(variable): Json<NewVariable>,
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

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/devices/{device_id}/variables",
    responses(
        (status = 200, description = "List of variables for device", body = Vec<Variable>),
        (status = 500, description = "Failed to retrieve variables", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_variables_for_device(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<Vec<Variable>>, StatusCode> {
    let variables = sqlx::query_as!(
        Variable,
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
    request_body = NewVariable,
    responses(
        (status = 201, description = "Variable added successfully"),
        (status = 500, description = "Failed to add variable", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn add_variable_to_device(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
    Json(variable): Json<NewVariable>,
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

    Ok(StatusCode::CREATED)
}

#[utoipa::path(
    put,
    path = "/devices/{device_id}/note",
    request_body = Note,
    responses(
        (status = 200, description = "Note updated successfully"),
        (status = 304, description = "Note not modified"),
        (status = 500, description = "Failed to update note", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn update_note_for_device(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
    Json(note): Json<Note>,
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
        (status = 200, description = "Device ledger entries", body = DeviceLedgerItemPaginated),
        (status = 400, description = "Invalid pagination parameters"),
        (status = 500, description = "Failed to retrieve device ledger", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_ledger_for_device(
    host: Host,
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
    pagination: Query<PaginationId>,
) -> Result<Json<DeviceLedgerItemPaginated>, StatusCode> {
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
            DeviceLedgerItem,
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
            DeviceLedgerItem,
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
            DeviceLedgerItem,
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

    let ledger_paginated = DeviceLedgerItemPaginated {
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
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
#[allow(clippy::collapsible_else_if)]
pub async fn get_all_commands_for_device(
    host: Host,
    Path(device_id): Path<String>,
    Extension(state): Extension<State>,
    pagination: Query<PaginationId>,
) -> Result<Json<CommandsPaginated>, StatusCode> {
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

    let limit = pagination.limit.unwrap_or(5).clamp(0, 500);

    let mut commands = if let Some(starting_after) = pagination.starting_after {
        sqlx::query_as!(
            DeviceCommandResponse,
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
            DeviceCommandResponse,
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
            DeviceCommandResponse,
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

    let commands_paginated = CommandsPaginated {
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
        (status = 200, description = "Device release information", body = DeviceRelease),
        (status = 500, description = "Failed to retrieve device release", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_device_release(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<DeviceRelease>, StatusCode> {
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

    let device_release: DeviceRelease = DeviceRelease {
        previous_release,
        release,
        target_release,
    };

    Ok(Json(device_release))
}

#[utoipa::path(
    post,
    path = "/devices/{device_id}/release",
    request_body = UpdateDeviceRelease,
    responses(
        (status = 200, description = "Device target release updated successfully"),
        (status = 404, description = "Release not found"),
        (status = 500, description = "Failed to update device target release", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn update_device_target_release(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
    Json(device_release): Json<UpdateDeviceRelease>,
) -> axum::response::Result<StatusCode, StatusCode> {
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
    request_body = UpdateDevicesRelease,
    responses(
        (status = 200, description = "Devices target release updated successfully"),
        (status = 404, description = "Release not found"),
        (status = 500, description = "Failed to update devices target release", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn update_devices_target_release(
    Extension(state): Extension<State>,
    Json(devices_release): Json<UpdateDevicesRelease>,
) -> axum::response::Result<StatusCode, StatusCode> {
    let target_release_id = devices_release.target_release_id;
    let release = Release::get_release_by_id(target_release_id, &state.pg_pool)
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
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn issue_commands_to_device(
    Path(device_id): Path<String>,
    Extension(state): Extension<State>,
    Json(commands): Json<Vec<SafeCommandRequest>>,
) -> axum::response::Result<StatusCode, StatusCode> {
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
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_device_info(
    Path(device_id): Path<String>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Device>, StatusCode> {
    let device_row = sqlx::query!(
        r#"
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
        tr.user_id as "target_release_user_id?",
        dn.network_score as "network_score?",
        dn.download_speed_mbps as "network_download_speed_mbps?",
        dn.upload_speed_mbps as "network_upload_speed_mbps?",
        dn.source as "network_source?",
        dn.updated_at as "network_updated_at?",
        COALESCE(JSONB_OBJECT_AGG(l.name, dl.value) FILTER (WHERE l.name IS NOT NULL), '{}') as "labels!: SqlxJson<HashMap<String, String>>"
        FROM device d
        LEFT JOIN ip_address ip ON d.ip_address_id = ip.id
        LEFT JOIN modem m ON d.modem_id = m.id
        LEFT JOIN release r ON d.release_id = r.id
        LEFT JOIN distribution rd ON r.distribution_id = rd.id
        LEFT JOIN release tr ON d.target_release_id = tr.id
        LEFT JOIN distribution trd ON tr.distribution_id = trd.id
        LEFT JOIN device_network dn ON d.id = dn.device_id
        LEFT JOIN device_label dl ON dl.device_id = d.id
        LEFT JOIN label l ON l.id = dl.label_id
        WHERE
            CASE
                WHEN $1 ~ '^[0-9]+$' AND length($1) <= 10 THEN
                    d.id = $1::int4
                ELSE
                    d.serial_number = $1
            END
        GROUP BY d.id, ip.id, m.id, r.id, rd.id, tr.id, trd.id, dn.device_id
        "#,
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

        Some(crate::ip_address::IpAddressInfo {
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
        Some(Modem {
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
        Some(Release {
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
        Some(Release {
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

    let network = if device_row.network_score.is_some() {
        Some(DeviceNetwork {
            network_score: device_row.network_score,
            download_speed_mbps: device_row.network_download_speed_mbps,
            upload_speed_mbps: device_row.network_upload_speed_mbps,
            source: device_row.network_source,
            updated_at: device_row.network_updated_at,
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
        network,
        labels: device_row.labels,
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
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn delete_device(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<StatusCode, StatusCode> {
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

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub struct UpdateDeviceRequest {
    pub labels: Option<HashMap<String, String>>,
}

#[utoipa::path(
    patch,
    path = "/devices/{device_id}",
    request_body = UpdateDeviceRequest,
    responses(
        (status = StatusCode::OK, description = "Device updated successfully"),
        (status = StatusCode::NOT_FOUND, description = "Device not found"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to update device"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn update_device(
    Path(device_id): Path<String>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Json(payload): Json<UpdateDeviceRequest>,
) -> axum::response::Result<StatusCode, StatusCode> {
    let allowed = authorization::check(current_user, "devices", "write");

    if !allowed {
        return Err(StatusCode::FORBIDDEN);
    }

    if let Some(labels) = payload.labels {
        let mut tx = state.pg_pool.begin().await.map_err(|err| {
            error!("Failed to start transaction {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        let keys = labels.keys().map(|key| key.to_string()).collect::<Vec<_>>();
        let values = labels.into_values().collect::<Vec<_>>();
        // Ensure the labels exists
        sqlx::query!(
            r#"
            INSERT INTO label (name)
            SELECT * FROM UNNEST($1::text[])
            ON CONFLICT (name) DO NOTHING
            "#,
            keys.as_slice()
        )
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to upsert labels {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        // Remove previous labels on device
        sqlx::query!(
            r#"
            DELETE FROM device_label
            USING device d
            WHERE 
                d.id = device_label.device_id AND
                CASE
                    WHEN $1 ~ '^[0-9]+$' AND length($1) <= 10 THEN
                        d.id = $1::int4
                    ELSE
                        d.serial_number = $1
                END
            "#,
            device_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to remove previous device_labels on device {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        let result = sqlx::query!(
            r#"
            WITH label_input AS (
                SELECT *
                FROM UNNEST($2::text[], $3::text[]) AS t(key, value)
            ),
            the_device AS (
                SELECT d.id
                FROM device d
                WHERE 
                    CASE
                        WHEN $1 ~ '^[0-9]+$' AND length($1) <= 10 THEN
                            id = $1::int4
                        ELSE
                            serial_number = $1
                    END
            )
            INSERT INTO device_label (device_id, label_id, value)
            SELECT
                d.id,
                l.id label_id,
                i.value
            FROM label_input i
            INNER JOIN the_device d ON d.id IS NOT NULL
            INNER JOIN label l ON l.name = i.key
            ON CONFLICT (device_id, label_id)
                DO UPDATE SET value = EXCLUDED.value
            "#,
            device_id,
            keys.as_slice(),
            values.as_slice(),
        )
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to create or update device_labels {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        tx.commit().await.map_err(|err| {
            error!("Failed to commit transaction {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        if result.rows_affected() == 0 {
            return Err(StatusCode::NOT_FOUND);
        }
    }

    Ok(StatusCode::OK)
}

#[utoipa::path(
    delete,
    path = "/labels/{key}",
    responses(
        (status = StatusCode::NO_CONTENT, description = "Label deleted from all devices"),
        (status = StatusCode::FORBIDDEN, description = "Forbidden"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to delete label"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn delete_label(
    Path(key): Path<String>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> axum::response::Result<StatusCode, StatusCode> {
    let allowed = authorization::check(current_user, "devices", "write");

    if !allowed {
        return Err(StatusCode::FORBIDDEN);
    }

    sqlx::query!(
        r#"
        DELETE FROM label
        WHERE name = $1
        "#,
        key
    )
    .execute(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to delete label '{key}': {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/devices/{device_id}/approval",
    responses(
        (status = 200, description = "Device approved successfully", body = bool),
        (status = 500, description = "Failed to approve device", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn approve_device(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<bool>, StatusCode> {
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

    Ok(Json(true))
}

#[utoipa::path(
    delete,
    path = "/devices/{device_id}/approval",
    responses(
        (status = 200, description = "Device approval revoked successfully"),
        (status = 500, description = "Failed to revoke device approval", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn revoke_device(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<()>, StatusCode> {
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
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn delete_token(
    Path(device_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<()>, StatusCode> {
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
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn get_network_for_device(
    Path(serial_number): Path<String>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<schema::Network>, StatusCode> {
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
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
pub async fn update_device_network(
    Path(serial_number): Path<String>,
    Extension(state): Extension<State>,
    Json(network_id): Json<i32>,
) -> axum::response::Result<StatusCode, StatusCode> {
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
        ("auth_token" = [])
    ),
    tag = DEVICES_TAG
)]
/// Batch updates the `network_id` for a list of `serial_number`s.
pub async fn update_devices_network(
    Path(network_id): Path<i32>,
    Extension(state): Extension<State>,
    Json(serial_numbers): Json<Vec<String>>,
) -> axum::response::Result<StatusCode, StatusCode> {
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
