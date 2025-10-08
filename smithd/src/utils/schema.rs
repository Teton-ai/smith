use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx;
use sqlx::Type;
use std::collections::HashMap;
use std::time;
use std::time::Duration;

// POST That the device does
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct HomePost {
    pub timestamp: Duration,
    pub responses: Vec<SafeCommandResponse>,
    pub release_id: Option<i32>,
    pub network_metrics: Option<NetworkMetrics>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct NetworkMetrics {
    pub rx_bytes_delta: u64,
    pub tx_bytes_delta: u64,
    pub interval_seconds: u64,
}

impl HomePost {
    pub fn new(
        responses: Vec<SafeCommandResponse>,
        release_id: Option<i32>,
        network_metrics: NetworkMetrics,
    ) -> Self {
        let timestamp = time::Instant::now().elapsed();
        Self {
            timestamp,
            responses,
            release_id,
            network_metrics: Some(network_metrics),
        }
    }
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct CreateSession {
    pub token: String,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Package {
    pub id: Option<i32>,
    pub name: String,
    pub architecture: Option<String>,
    pub version: String,
    pub file: String,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct SafeCommandResponse {
    pub id: i32,
    pub command: SafeCommandRx,
    pub status: i32,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub enum SafeCommandRx {
    #[default]
    Pong,
    Restart {
        message: String,
    },
    FreeForm {
        stdout: String,
        stderr: String,
    },
    OpenTunnel {
        port_server: u16,
    },
    TunnelClosed,
    GetVariables,
    Upgraded,
    UpdateVariables,
    GetNetwork,
    UpdateNetwork,
    UpdateSystemInfo {
        system_info: Value,
    },
    UpdatePackage {
        name: String,
        version: String,
    },
    UpgradePackages,
    WifiConnect {
        stdout: String,
        stderr: String,
    },
    DownloadOTA,
    CheckOTAStatus {
        status: String,
    },
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct SafeCommandRequest {
    pub id: i32,
    pub command: SafeCommandTx,
    pub continue_on_error: bool,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub enum SafeCommandTx {
    #[default]
    Ping,
    Upgrade,
    Restart,
    FreeForm {
        cmd: String,
    },
    OpenTunnel {
        port: Option<u16>,
        user: Option<String>,
        pub_key: Option<String>,
    },
    CloseTunnel,
    UpdateNetwork {
        network: Network,
    },
    UpdateVariables {
        variables: HashMap<String, String>,
    },
    DownloadOTA {
        tools: String,
        payload: String,
        rate: f64,
    },
    CheckOTAStatus,
    StartOTA,
}

// RESPONSE THAT IT GETS
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct HomePostResponse {
    pub timestamp: Duration,
    pub commands: Vec<SafeCommandRequest>,
    pub target_release_id: Option<i32>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct DeviceRegistration {
    pub serial_number: String,
    pub wifi_mac: String,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct DeviceRegistrationResponse {
    pub token: String,
}

#[derive(Type, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[sqlx(type_name = "network_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum NetworkType {
    Wifi,
    Ethernet,
    Dongle,
}

impl From<Option<String>> for NetworkType {
    fn from(value: Option<String>) -> Self {
        match value
            .expect("error: failed to get network type string")
            .as_str()
            .to_lowercase()
            .as_str()
        {
            "wifi" => NetworkType::Wifi,
            "ethernet" => NetworkType::Ethernet,
            "dongle" => NetworkType::Dongle,
            _ => panic!("error: invalid network type string"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Network {
    pub id: i32,
    pub network_type: NetworkType,
    pub is_network_hidden: bool,
    pub ssid: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub password: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NewNetwork {
    pub network_type: NetworkType,
    pub is_network_hidden: bool,
    pub ssid: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub password: Option<String>,
}
