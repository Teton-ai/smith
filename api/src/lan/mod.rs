pub mod route;

use anyhow::Result;
use models::system::Network;
use serde_json::Value;
use sqlx::PgPool;
use sqlx::types::ipnetwork::IpNetwork;
use std::net::IpAddr;

/// Replaces the device's interface addresses with the ones in its latest `system_info`.
/// Devices on older smithd versions report no prefixes, so they simply end up with no rows.
pub async fn record_interface_addresses(
    pool: &PgPool,
    device_id: i32,
    system_info: &Value,
) -> Result<()> {
    let Some(network) = system_info.get("network") else {
        return Ok(());
    };
    let network: Network = serde_json::from_value(network.clone())?;

    let mut interfaces = Vec::new();
    let mut addresses = Vec::new();
    let mut mac_addresses = Vec::new();
    let mut gateway_ips = Vec::new();
    let mut gateway_macs = Vec::new();

    for (name, item) in &network.interfaces {
        let gateway_ip = item
            .gateway
            .as_ref()
            .and_then(|gateway| gateway.ip.parse::<IpAddr>().ok());
        let gateway_mac = item
            .gateway
            .as_ref()
            .and_then(|gateway| gateway.mac_address.as_deref())
            .filter(|mac| is_mac(mac));

        for address in &item.addresses {
            let Ok(address) = address.parse::<IpNetwork>() else {
                continue;
            };
            let ip = address.ip();
            if ip.is_loopback() || is_link_local(ip) {
                continue;
            }
            // A gateway only defines the LAN of the address whose subnet contains it.
            let gateway = gateway_ip.filter(|gateway| address.contains(*gateway));

            interfaces.push(name.clone());
            addresses.push(address);
            mac_addresses.push(Some(item.mac_address.clone()).filter(|mac| is_mac(mac)));
            gateway_ips.push(gateway.map(IpNetwork::from));
            gateway_macs.push(gateway.and(gateway_mac).map(str::to_string));
        }
    }

    let mut tx = pool.begin().await?;
    sqlx::query!(
        "DELETE FROM device_interface_address WHERE device_id = $1",
        device_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        r#"INSERT INTO device_interface_address
               (device_id, interface, address, mac_address, gateway_ip, gateway_mac)
           SELECT $1, UNNEST($2::text[]), UNNEST($3::inet[]), UNNEST($4::text[])::macaddr,
                  UNNEST($5::inet[]), UNNEST($6::text[])::macaddr
           ON CONFLICT DO NOTHING"#,
        device_id,
        &interfaces,
        &addresses,
        &mac_addresses as &[Option<String>],
        &gateway_ips as &[Option<IpNetwork>],
        &gateway_macs as &[Option<String>],
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(())
}

fn is_link_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_link_local(),
        IpAddr::V6(ip) => ip.is_unicast_link_local(),
    }
}

/// Guards the `::macaddr` casts so one odd value can't fail the whole insert.
fn is_mac(value: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_hexdigit()))
        && value != "00:00:00:00:00:00"
}
