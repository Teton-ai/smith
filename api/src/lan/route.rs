use crate::State;
use crate::middlewares::authorization;
use crate::user::CurrentUser;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::{Extension, Json};
use models::lan::{Lan, LanDevice, LanListResponse, LanSummary, LanSummaryListResponse};
use serde::Deserialize;
use sqlx::types::ipnetwork::IpNetwork;
use tracing::error;
use utoipa::IntoParams;

const LAN_TAG: &str = "lan";

#[utoipa::path(
    get,
    path = "/devices/{serial_number}/lans",
    description = "Local networks the device is on, with the other devices on each. LANs are \
                   inferred from the devices' reported default gateways: devices whose gateway \
                   has the same MAC address and whose address is in the same subnet share a LAN. \
                   Requires smithd to report interface prefixes and gateways; devices on older \
                   versions are not listed.",
    params(
        ("serial_number" = String, Path),
    ),
    responses(
        (status = 200, description = "LANs with their devices", body = LanListResponse),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Failed to retrieve LANs", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = LAN_TAG
)]
pub async fn get_lans_for_device(
    Path(serial_number): Path<String>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<Json<LanListResponse>, StatusCode> {
    if !authorization::check(current_user, "devices", "read") {
        return Err(StatusCode::FORBIDDEN);
    }

    // Scoped to one device's LANs so the lookup stays on the LAN index instead of
    // scanning every device's addresses.
    let rows = sqlx::query!(
        r#"
        WITH own AS (
            SELECT DISTINCT dia.gateway_mac, network(dia.address) AS network
            FROM device_interface_address dia
            JOIN device d ON d.id = dia.device_id
            WHERE d.serial_number = $1
              AND dia.gateway_mac IS NOT NULL
        )
        SELECT
            dia.gateway_mac::text AS "gateway_mac!",
            network(dia.address)::text AS "network!",
            host(dia.gateway_ip) AS "gateway_ip!",
            dia.interface,
            host(dia.address) AS "address!",
            d.id,
            d.serial_number,
            d.last_ping,
            d.last_ping > NOW() - INTERVAL '3 minutes' AS "online!",
            host(ip.ip_address) AS public_ip,
            ip.name AS public_ip_name
        FROM own
        JOIN device_interface_address dia
          ON dia.gateway_mac = own.gateway_mac
         AND network(dia.address) = own.network
        JOIN device d ON d.id = dia.device_id
        LEFT JOIN ip_address ip ON ip.id = d.ip_address_id
        WHERE NOT d.archived
        ORDER BY 1, 2, d.serial_number, dia.interface
        "#,
        serial_number
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get LANs: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Rows are ordered by (gateway_mac, network), so each LAN is a contiguous run.
    let mut lans: Vec<Lan> = Vec::new();
    for row in rows {
        let key = format!("{}|{}", row.gateway_mac, row.network);
        let device = LanDevice {
            id: row.id,
            serial_number: row.serial_number,
            interface: row.interface,
            address: row.address,
            public_ip: row.public_ip,
            public_ip_name: row.public_ip_name,
            last_seen: row.last_ping,
            online: row.online,
        };
        match lans.last_mut() {
            Some(lan) if lan.key == key => lan.devices.push(device),
            _ => lans.push(Lan {
                key,
                network: row.network,
                gateway_ip: row.gateway_ip,
                gateway_mac: row.gateway_mac,
                devices: vec![device],
            }),
        }
    }

    Ok(Json(LanListResponse { lans }))
}

#[utoipa::path(
    get,
    path = "/lans",
    description = "Every local network in the fleet with how many devices are on it, largest \
                   first. LANs are inferred the same way as for `/devices/{serial_number}/lans`; \
                   fetch a LAN's devices with `/lans/devices`.",
    responses(
        (status = 200, description = "LANs without their devices", body = LanSummaryListResponse),
        (status = 403, description = "Forbidden"),
        (status = 500, description = "Failed to retrieve LANs", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = LAN_TAG
)]
pub async fn get_lans(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<Json<LanSummaryListResponse>, StatusCode> {
    if !authorization::check(current_user, "devices", "read") {
        return Err(StatusCode::FORBIDDEN);
    }

    let rows = sqlx::query!(
        r#"
        WITH lan AS (
            SELECT
                dia.gateway_mac,
                network(dia.address) AS network,
                MIN(host(dia.gateway_ip)) AS gateway_ip,
                COUNT(DISTINCT d.id) AS device_count,
                COUNT(DISTINCT d.id) FILTER (WHERE d.last_ping > NOW() - INTERVAL '3 minutes')
                    AS online_count,
                MIN(d.ip_address_id) AS ip_address_id
            FROM device_interface_address dia
            JOIN device d ON d.id = dia.device_id
            WHERE dia.gateway_mac IS NOT NULL
              AND NOT d.archived
            GROUP BY dia.gateway_mac, network(dia.address)
        )
        SELECT
            lan.gateway_mac::text AS "gateway_mac!",
            lan.network::text AS "network!",
            lan.gateway_ip AS "gateway_ip!",
            lan.device_count AS "device_count!",
            lan.online_count AS "online_count!",
            host(ip.ip_address) AS public_ip,
            ip.name AS public_ip_name
        FROM lan
        LEFT JOIN ip_address ip ON ip.id = lan.ip_address_id
        ORDER BY lan.device_count DESC, 2, 1
        "#
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get LANs: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let lans = rows
        .into_iter()
        .map(|row| LanSummary {
            key: format!("{}|{}", row.gateway_mac, row.network),
            network: row.network,
            gateway_ip: row.gateway_ip,
            gateway_mac: row.gateway_mac,
            device_count: row.device_count,
            online_count: row.online_count,
            public_ip: row.public_ip,
            public_ip_name: row.public_ip_name,
        })
        .collect();

    Ok(Json(LanSummaryListResponse { lans }))
}

#[derive(Deserialize, Debug, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LanQuery {
    /// MAC address of the LAN's gateway.
    gateway_mac: String,
    /// Subnet in CIDR notation, e.g. `192.168.1.0/24`.
    network: String,
}

#[utoipa::path(
    get,
    path = "/lans/devices",
    description = "One LAN, identified by its gateway MAC and subnet, with all its devices.",
    params(LanQuery),
    responses(
        (status = 200, description = "The LAN with its devices", body = Lan),
        (status = 400, description = "Invalid gateway MAC or network"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "No devices on this LAN"),
        (status = 500, description = "Failed to retrieve the LAN", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = LAN_TAG
)]
pub async fn get_lan(
    Query(query): Query<LanQuery>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
) -> Result<Json<Lan>, StatusCode> {
    if !authorization::check(current_user, "devices", "read") {
        return Err(StatusCode::FORBIDDEN);
    }

    // Validated here so a bad value is a 400 rather than a failed `::cidr`/`::macaddr` cast.
    // `cidr` also rejects host bits, so the address must be the network's own.
    let is_network = query
        .network
        .parse::<IpNetwork>()
        .is_ok_and(|net| net.network() == net.ip());
    if !super::is_mac(&query.gateway_mac) || !is_network {
        return Err(StatusCode::BAD_REQUEST);
    }

    let rows = sqlx::query!(
        r#"
        SELECT
            dia.gateway_mac::text AS "gateway_mac!",
            network(dia.address)::text AS "network!",
            host(dia.gateway_ip) AS "gateway_ip!",
            dia.interface,
            host(dia.address) AS "address!",
            d.id,
            d.serial_number,
            d.last_ping,
            d.last_ping > NOW() - INTERVAL '3 minutes' AS "online!",
            host(ip.ip_address) AS public_ip,
            ip.name AS public_ip_name
        FROM device_interface_address dia
        JOIN device d ON d.id = dia.device_id
        LEFT JOIN ip_address ip ON ip.id = d.ip_address_id
        WHERE dia.gateway_mac = $1::text::macaddr
          AND network(dia.address) = $2::text::cidr
          AND NOT d.archived
        ORDER BY d.serial_number, dia.interface
        "#,
        query.gateway_mac,
        query.network,
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get LAN: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut lan: Option<Lan> = None;
    for row in rows {
        let device = LanDevice {
            id: row.id,
            serial_number: row.serial_number,
            interface: row.interface,
            address: row.address,
            public_ip: row.public_ip,
            public_ip_name: row.public_ip_name,
            last_seen: row.last_ping,
            online: row.online,
        };
        match lan.as_mut() {
            Some(lan) => lan.devices.push(device),
            None => {
                lan = Some(Lan {
                    key: format!("{}|{}", row.gateway_mac, row.network),
                    network: row.network,
                    gateway_ip: row.gateway_ip,
                    gateway_mac: row.gateway_mac,
                    devices: vec![device],
                })
            }
        }
    }

    lan.map(Json).ok_or(StatusCode::NOT_FOUND)
}
