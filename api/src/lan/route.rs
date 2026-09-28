use crate::State;
use crate::middlewares::authorization;
use crate::user::CurrentUser;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Extension, Json};
use models::lan::{Lan, LanDevice, LanListResponse};
use tracing::error;

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
