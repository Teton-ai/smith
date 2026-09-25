use serde::{Deserialize, Serialize};
use sqlx::types::chrono::{DateTime, Utc};
use utoipa::ToSchema;

/// Devices whose default gateway has the same MAC and whose address is in the same subnet.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Lan {
    /// Stable identifier derived from `gateway_mac` and `network`.
    pub key: String,
    /// Subnet in CIDR notation, e.g. `192.168.1.0/24`.
    pub network: String,
    pub gateway_ip: String,
    pub gateway_mac: String,
    pub devices: Vec<LanDevice>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LanDevice {
    pub id: i32,
    pub serial_number: String,
    pub interface: String,
    /// Private address of the device on this LAN.
    pub address: String,
    pub public_ip: Option<String>,
    pub public_ip_name: Option<String>,
    pub last_seen: Option<DateTime<Utc>>,
    pub online: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LanListResponse {
    pub lans: Vec<Lan>,
}
