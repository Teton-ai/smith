use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Smith {
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct OsRelease {
    pub pretty_name: String,
    pub version_id: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DeviceTree {
    pub serial_number: String,
    pub model: Option<String>,
    pub compatible: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProcStat {
    pub btime: u64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Proc {
    pub version: String,
    pub stat: ProcStat,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct NetworkItem {
    pub ips: Vec<String>,
    pub mac_address: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Network {
    pub interfaces: HashMap<String, NetworkItem>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Default, ToSchema)]
pub struct NetworkConfig {
    pub connection_profile_name: String,
    pub connection_profile_uuid: String,
    pub device_type: String,
    pub device_name: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Default, ToSchema)]
pub struct ConnectionStatus {
    pub connection_name: String,
    pub connection_state: String,
    pub device_type: String,
    pub device_name: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SystemInfo {
    pub smith: Smith,
    pub hostname: String,
    pub os_release: OsRelease,
    pub proc: Proc,
    pub network: Network,
    pub device_tree: DeviceTree,
    pub connection_statuses: Vec<ConnectionStatus>,
}
