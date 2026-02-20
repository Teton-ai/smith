use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx;
use sqlx::Type;
use std::collections::HashMap;
use std::time;
use std::time::Duration;

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct ServiceCheck {
    pub id: i32,
    pub name: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct ServiceStatus {
    pub id: i32,
    pub active_state: String,
    pub n_restarts: u32,
}

// POST That the device does
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct HomePost {
    pub timestamp: Duration,
    pub responses: Vec<SafeCommandResponse>,
    pub release_id: Option<i32>,
    #[serde(default)]
    pub service_statuses: Vec<ServiceStatus>,
}

impl HomePost {
    pub fn new(
        responses: Vec<SafeCommandResponse>,
        release_id: Option<i32>,
        service_statuses: Vec<ServiceStatus>,
    ) -> Self {
        let timestamp = time::Instant::now().elapsed();
        Self {
            timestamp,
            responses,
            release_id,
            service_statuses,
        }
    }
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct CreateSession {
    pub token: String,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Package {
    pub id: i32,
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
    TestNetwork {
        bytes_downloaded: usize,
        duration_ms: u64,
        bytes_uploaded: Option<usize>,
        upload_duration_ms: Option<u64>,
        timed_out: bool,
    },
    ExtendedNetworkTest {
        samples: Vec<SpeedSample>,
        network_info: Option<NetworkInfo>,
        total_duration_ms: u64,
        error: Option<String>,
    },
    LogStreamStarted {
        session_id: String,
    },
    LogStreamStopped {
        session_id: String,
    },
    LogStreamError {
        session_id: String,
        error: String,
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
    TestNetwork,
    ExtendedNetworkTest {
        duration_minutes: u32,
    },
    StreamLogs {
        session_id: String,
        service_name: String,
    },
    StopLogStream {
        session_id: String,
    },
}

// RESPONSE THAT IT GETS
#[derive(Serialize, Deserialize, Default, Debug)]
pub struct HomePostResponse {
    pub timestamp: Duration,
    pub commands: Vec<SafeCommandRequest>,
    pub target_release_id: Option<i32>,
    #[serde(default)]
    pub services: Vec<ServiceCheck>,
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
        match value.as_deref().map(|s| s.to_lowercase()) {
            Some(s) => match s.as_str() {
                "wifi" => NetworkType::Wifi,
                "ethernet" => NetworkType::Ethernet,
                "dongle" => NetworkType::Dongle,
                other => {
                    tracing::warn!(network_type = %other, "Unknown network type, defaulting to Ethernet");
                    NetworkType::Ethernet
                }
            },
            None => {
                tracing::warn!("Missing network type, defaulting to Ethernet");
                NetworkType::Ethernet
            }
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

// Extended network test types

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpeedSample {
    pub started_at: DateTime<Utc>,
    pub download_bytes: usize,
    pub download_mbps: f64,
    pub upload_bytes: Option<usize>,
    pub upload_mbps: Option<f64>,
    pub duration_ms: u64,
    pub timed_out: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum InterfaceType {
    Wifi,
    Ethernet,
    Lte,
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum NetworkDetails {
    Wifi {
        ssid: Option<String>,
        signal_dbm: Option<i32>,
        frequency_mhz: Option<u32>,
        vht_mcs: Option<u8>,
        vht_nss: Option<u8>,
        channel_width_mhz: Option<u8>,
    },
    Ethernet {
        speed_mbps: Option<u32>,
        duplex: Option<String>,
        link_detected: bool,
    },
    Lte {
        operator: Option<String>,
        signal_quality: Option<i32>,
        access_technology: Option<String>,
    },
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NetworkInfo {
    pub interface_type: InterfaceType,
    pub interface_name: String,
    pub details: NetworkDetails,
}
