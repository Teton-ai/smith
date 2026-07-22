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
    #[serde(deserialize_with = "deserialize_command")]
    pub command: SafeCommandRx,
    pub status: i32,
}

/// Tolerate any report variant this build doesn't recognize (e.g. a report type that
/// was removed but is still sent by an un-upgraded device): fall back to
/// `SafeCommandRx::Unknown` instead of failing to deserialize the whole request,
/// which would otherwise reject the POST and block the device's status update.
fn deserialize_command<'de, D>(deserializer: D) -> Result<SafeCommandRx, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or(SafeCommandRx::Unknown))
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct NMProfile {
    pub name: String,
    pub ssid: Option<String>,
    pub password: Option<String>,
    pub is_active: bool,
    pub key_mgmt: Option<String>,
    pub hidden: Option<bool>,
    pub pmf: Option<String>,
    pub eap: Option<String>,
    pub phase2_auth: Option<String>,
    pub anonymous_identity: Option<String>,
    pub eap_identity: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IntentNetwork {
    pub profile_name: String,
    pub ssid: String,
    pub priority: i32,
    pub credentials: NetworkCredentials,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NetworkCredentials {
    pub key_mgmt: String,
    #[serde(default)]
    pub psk: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NetworkCondition {
    pub profile_name: String,
    pub state: ConditionState,
    pub reason: Option<ConditionReason>,
    pub message: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ConditionState {
    Applied,
    Failed,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ConditionReason {
    WrongPSK,
    NotInRange,
    NmcliError,
    ActiveProfileKept,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WifiNetwork {
    pub ssid: Option<String>,
    pub bssid: String,
    pub signal: Option<i32>,
    pub rate: Option<i32>,
    pub security: Option<String>,
    pub channel: Option<i32>,
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
    ReportNMProfiles {
        profiles: Vec<NMProfile>,
    },
    WifiScan {
        networks: Vec<WifiNetwork>,
    },
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
    AuditReport {
        disk_encrypted: Option<bool>,
        password_access_disabled: Option<bool>,
    },
    ApplyNetworksResult {
        applied_version: i32,
        conditions: Vec<NetworkCondition>,
    },
    /// Fallback for any report this build doesn't recognize; ignored by the api.
    Unknown,
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
    ReportNMProfiles,
    WifiScan,
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
    RunAudit,
    GetLogs {
        unit: Option<String>,
        since: Option<String>,
        until: Option<String>,
        grep: Option<String>,
    },
    ApplyNetworks {
        version: i32,
        networks: Vec<IntentNetwork>,
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
    Unknown {},
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NetworkInfo {
    pub interface_type: InterfaceType,
    pub interface_name: String,
    pub details: NetworkDetails,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_logs_protocol_round_trip() {
        // Deserialize the JSON shape the API stores in the cmd jsonb column.
        let json = r#"{"GetLogs":{"unit":"smithd","since":"1h ago","until":null,"grep":null}}"#;
        let cmd: SafeCommandTx = serde_json::from_str(json).unwrap();
        match cmd {
            SafeCommandTx::GetLogs {
                unit,
                since,
                until,
                grep,
            } => {
                assert_eq!(unit, Some("smithd".to_string()));
                assert_eq!(since, Some("1h ago".to_string()));
                assert_eq!(until, None);
                assert_eq!(grep, None);
            }
            _ => panic!("expected GetLogs variant"),
        }
    }

    #[test]
    fn get_logs_omitted_fields_default_to_none() {
        // Fields absent from the JSON object must deserialize as None,
        // not fail. Covers clients that omit null fields entirely.
        let json = r#"{"GetLogs":{"unit":"smithd"}}"#;
        let cmd: SafeCommandTx = serde_json::from_str(json).unwrap();
        match cmd {
            SafeCommandTx::GetLogs {
                unit,
                since,
                until,
                grep,
            } => {
                assert_eq!(unit, Some("smithd".to_string()));
                assert_eq!(since, None);
                assert_eq!(until, None);
                assert_eq!(grep, None);
            }
            _ => panic!("expected GetLogs variant"),
        }
    }

    #[test]
    fn get_logs_serialization_roundtrip() {
        // Serialized shape must match what the API stores in the cmd jsonb column.
        let cmd = SafeCommandTx::GetLogs {
            unit: Some("smithd".to_string()),
            since: Some("1h ago".to_string()),
            until: None,
            grep: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let expected = r#"{"GetLogs":{"unit":"smithd","since":"1h ago","until":null,"grep":null}}"#;
        assert_eq!(json, expected);
    }

    // Golden-file wire-format tests. The fixtures are the on-the-wire contract
    // between smithd and the API: a deployed daemon and a deployed API may run
    // different versions, so any change that makes these fail is a protocol
    // change and must be backward compatible (new fields need serde(default),
    // renames/tag changes are breaking). Update a fixture only deliberately.

    #[test]
    fn home_post_matches_golden_fixture() {
        let post = HomePost {
            timestamp: Duration::new(1, 500_000_000),
            responses: vec![
                SafeCommandResponse {
                    id: -1,
                    command: SafeCommandRx::Pong,
                    status: 0,
                },
                SafeCommandResponse {
                    id: 2,
                    command: SafeCommandRx::FreeForm {
                        stdout: "e2e-ok\n".to_string(),
                        stderr: String::new(),
                    },
                    status: 0,
                },
                SafeCommandResponse {
                    id: 3,
                    command: SafeCommandRx::Upgraded,
                    status: 0,
                },
                SafeCommandResponse {
                    id: 4,
                    command: SafeCommandRx::UpdateSystemInfo {
                        system_info: serde_json::json!({"os": "ubuntu"}),
                    },
                    status: 0,
                },
            ],
            release_id: Some(42),
            service_statuses: vec![ServiceStatus {
                id: 1,
                active_state: "active".to_string(),
                n_restarts: 2,
            }],
        };

        let fixture: Value = serde_json::from_str(include_str!("fixtures/home_post.json")).unwrap();
        assert_eq!(
            serde_json::to_value(&post).unwrap(),
            fixture,
            "HomePost serialization no longer matches the wire contract"
        );

        // The reverse direction catches new required fields the old peer omits.
        let parsed: HomePost = serde_json::from_value(fixture).unwrap();
        assert_eq!(parsed.release_id, Some(42));
        assert_eq!(parsed.responses.len(), 4);
        assert_eq!(parsed.service_statuses.len(), 1);
    }

    #[test]
    fn home_post_response_matches_golden_fixture() {
        let response = HomePostResponse {
            timestamp: Duration::new(1721, 0),
            commands: vec![
                SafeCommandRequest {
                    id: 1,
                    command: SafeCommandTx::Ping,
                    continue_on_error: false,
                },
                SafeCommandRequest {
                    id: 2,
                    command: SafeCommandTx::FreeForm {
                        cmd: "echo hi".to_string(),
                    },
                    continue_on_error: false,
                },
                SafeCommandRequest {
                    id: 3,
                    command: SafeCommandTx::Upgrade,
                    continue_on_error: true,
                },
                SafeCommandRequest {
                    id: 4,
                    command: SafeCommandTx::GetLogs {
                        unit: Some("smithd".to_string()),
                        since: Some("1h ago".to_string()),
                        until: None,
                        grep: None,
                    },
                    continue_on_error: false,
                },
            ],
            target_release_id: Some(7),
            services: vec![ServiceCheck {
                id: 1,
                name: "smithd".to_string(),
            }],
        };

        let fixture: Value =
            serde_json::from_str(include_str!("fixtures/home_post_response.json")).unwrap();
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            fixture,
            "HomePostResponse serialization no longer matches the wire contract"
        );

        let parsed: HomePostResponse = serde_json::from_value(fixture).unwrap();
        assert_eq!(parsed.target_release_id, Some(7));
        assert_eq!(parsed.commands.len(), 4);
        assert_eq!(parsed.services.len(), 1);
    }

    #[test]
    fn registration_matches_golden_fixture() {
        let registration = DeviceRegistration {
            serial_number: "smith-device-1".to_string(),
            wifi_mac: "aa:bb:cc:dd:ee:ff".to_string(),
        };
        let response = DeviceRegistrationResponse {
            token: "8b1a44a4-2a10-42da-9e59-6dc2b3f6e1b0".to_string(),
        };

        let fixture: Value =
            serde_json::from_str(include_str!("fixtures/registration.json")).unwrap();
        assert_eq!(
            serde_json::to_value(&registration).unwrap(),
            fixture["registration"],
            "DeviceRegistration serialization no longer matches the wire contract"
        );
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            fixture["response"],
            "DeviceRegistrationResponse serialization no longer matches the wire contract"
        );

        let parsed: DeviceRegistration =
            serde_json::from_value(fixture["registration"].clone()).unwrap();
        assert_eq!(parsed.serial_number, "smith-device-1");
        let parsed: DeviceRegistrationResponse =
            serde_json::from_value(fixture["response"].clone()).unwrap();
        assert_eq!(parsed.token, "8b1a44a4-2a10-42da-9e59-6dc2b3f6e1b0");
    }

    #[test]
    fn home_post_tolerates_peer_without_service_statuses() {
        // An older daemon omits service_statuses entirely; the API must accept it.
        let json = r#"{"timestamp":{"secs":1,"nanos":0},"responses":[],"release_id":null}"#;
        let post: HomePost = serde_json::from_str(json).unwrap();
        assert!(post.service_statuses.is_empty());
    }

    #[test]
    fn home_post_response_tolerates_peer_without_services() {
        // An older API omits services entirely; the daemon must accept it.
        let json = r#"{"timestamp":{"secs":1,"nanos":0},"commands":[],"target_release_id":null}"#;
        let response: HomePostResponse = serde_json::from_str(json).unwrap();
        assert!(response.services.is_empty());
    }
}
