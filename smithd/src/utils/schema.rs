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
    #[serde(default)]
    pub autoconnect_priority: Option<i32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IntentNetwork {
    pub profile_name: String,
    pub ssid: String,
    pub priority: i32,
    // Defaulted: a command queued before the API rolled out these fields must
    // still deserialize. "" is a value profile_security_type never produces,
    // so a missing security_type just disables adopt-matching, not a false match.
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub security_type: String,
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
    FileSessionStarted {
        session_id: String,
    },
    FileSessionStopped {
        session_id: String,
    },
    FileSessionError {
        session_id: String,
        error: String,
    },
    /// Fallback for any report this build doesn't recognize; ignored by the api.
    Unknown,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct SafeCommandRequest {
    pub id: i32,
    #[serde(deserialize_with = "deserialize_tx")]
    pub command: SafeCommandTx,
    pub continue_on_error: bool,
}

/// Tolerate any command variant this build doesn't recognize (e.g. a newer api
/// issuing a command added after this daemon shipped): fall back to
/// `SafeCommandTx::Unknown` instead of failing to deserialize the whole
/// `HomePostResponse`. `Postman::ping_home` parses that response with
/// `unwrap_or_default()`, so a failed parse would silently discard every other
/// command in the batch along with `target_release_id` and `services`, leaving
/// the device unable to converge on its target release.
fn deserialize_tx<'de, D>(deserializer: D) -> Result<SafeCommandTx, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or(SafeCommandTx::Unknown))
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
    /// Dial back to the api and serve filesystem operations for the lifetime of
    /// the session. Carries no path: every operation is negotiated over the
    /// resulting websocket so browsing doesn't pay the poll interval per click.
    OpenFileSession {
        session_id: String,
    },
    CloseFileSession {
        session_id: String,
    },
    /// Fallback for any command this build doesn't recognize. Never issued by
    /// the api: it is produced locally by `deserialize_tx` and reported back
    /// with a failure status so the operator sees why nothing happened.
    Unknown,
}

/// What a directory entry is, as reported by `lstat` — a symlink is reported as
/// `Symlink` rather than resolved, so the UI can show the link and its target.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    File,
    Dir,
    Symlink,
    Other,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DirEntryInfo {
    /// Linux filenames are bytes, not UTF-8. A name that isn't valid UTF-8 is
    /// lossily converted for display and `reachable` is false, because the lossy
    /// form would not round-trip back to the same file.
    pub name: String,
    pub kind: FileKind,
    /// `st_size`. Meaningless for directories; the UI hides it there.
    pub size: u64,
    /// Unix mtime in whole seconds, `None` when the filesystem has none.
    pub mtime: Option<i64>,
    /// Permission bits only (`st_mode & 0o7777`).
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    /// Raw, unresolved link target. `Some` only for `FileKind::Symlink`.
    pub symlink_target: Option<String>,
    /// False when this entry cannot be acted on — currently only for names that
    /// aren't valid UTF-8, which would not round-trip back to the same file.
    pub reachable: bool,
}

/// Control frames sent api -> device over the file session websocket, as JSON
/// text. Distinct from `SafeCommandTx`: these never touch the command queue or
/// Postgres, so they are free of the NUL-stripping and size concerns that apply
/// to command payloads.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum FileOpRequest {
    List {
        op_id: u64,
        path: String,
    },
    /// Resolve, validate and *hold the descriptor open*. Holding it makes the
    /// later transfer free of a time-of-check/time-of-use race and the reported
    /// size exact.
    Open {
        op_id: u64,
        path: String,
    },
    /// Stream the descriptor held for `op_id` to the api's upload endpoint.
    StartUpload {
        op_id: u64,
        upload_token: String,
    },
    /// Release a held descriptor without transferring it.
    Cancel {
        op_id: u64,
    },
}

/// Control frames sent device -> api over the file session websocket.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum FileOpResponse {
    Listing {
        op_id: u64,
        /// Canonicalized absolute path, so the caller's breadcrumb reflects
        /// where it actually landed after resolving symlinks.
        path: String,
        entries: Vec<DirEntryInfo>,
        /// True when the directory held more than the daemon will return.
        truncated: bool,
    },
    Opened {
        op_id: u64,
        name: String,
        size: u64,
    },
    UploadFinished {
        op_id: u64,
        bytes_sent: u64,
    },
    Error {
        op_id: u64,
        code: FileOpError,
        message: String,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOpError {
    NotFound,
    PermissionDenied,
    NotADirectory,
    /// Character devices, block devices, sockets and FIFOs are refused: opening
    /// them can block forever or stream without end.
    NotRegularFile,
    TooLarge,
    TooManyOpenFiles,
    Io,
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
    /// App API `wifi_security_enum` ("open" | "WPA2-Personal" | "WPA2-Enterprise").
    /// Absent for older callers; falls back to the password heuristic.
    #[serde(default)]
    pub security: Option<String>,
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
    fn intent_network_omitted_hidden_and_security_type_default() {
        // A command queued before the API rolled out these fields must still
        // deserialize, not get dropped.
        let json = r#"{"profile_name":"HC-Teton","ssid":"HC-Teton","priority":10,
            "credentials":{"key_mgmt":"wpa-psk","psk":"secret"}}"#;
        let network: IntentNetwork = serde_json::from_str(json).unwrap();
        assert!(!network.hidden);
        assert_eq!(network.security_type, "");
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

    #[test]
    fn home_post_response_tolerates_unknown_command() {
        // A newer API issues a command this build predates. `ping_home` parses
        // with `unwrap_or_default()`, so if the unknown variant poisoned the
        // whole response the daemon would silently lose every sibling command
        // plus target_release_id and services for that tick.
        let json = r#"{
            "timestamp": {"secs": 1, "nanos": 0},
            "commands": [
                {"id": 1, "command": {"NotARealCommand": {"path": "/etc"}}, "continue_on_error": false},
                {"id": 2, "command": "Ping", "continue_on_error": false}
            ],
            "target_release_id": 7,
            "services": [{"id": 1, "name": "smithd"}]
        }"#;

        let response: HomePostResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.target_release_id, Some(7));
        assert_eq!(response.services.len(), 1);
        assert_eq!(response.commands.len(), 2);
        assert!(matches!(
            response.commands[0].command,
            SafeCommandTx::Unknown
        ));
        assert!(matches!(response.commands[1].command, SafeCommandTx::Ping));
    }

    #[test]
    fn file_session_protocol_matches_golden_fixture() {
        // FileOpRequest/FileOpResponse never touch the command queue, but they
        // are just as much a cross-version contract as HomePost: an api and a
        // daemon on different releases must agree on this framing.
        let request = FileOpRequest::List {
            op_id: 1,
            path: "/var/log".to_string(),
        };
        let response = FileOpResponse::Listing {
            op_id: 1,
            path: "/var/log".to_string(),
            entries: vec![
                DirEntryInfo {
                    name: "syslog".to_string(),
                    kind: FileKind::File,
                    size: 4096,
                    mtime: Some(1_700_000_000),
                    mode: 0o644,
                    uid: 0,
                    gid: 4,
                    symlink_target: None,
                    reachable: true,
                },
                DirEntryInfo {
                    name: "journal".to_string(),
                    kind: FileKind::Dir,
                    size: 0,
                    mtime: Some(1_700_000_001),
                    mode: 0o755,
                    uid: 0,
                    gid: 0,
                    symlink_target: None,
                    reachable: true,
                },
            ],
            truncated: false,
        };

        let fixture: Value =
            serde_json::from_str(include_str!("fixtures/file_session.json")).unwrap();
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            fixture["request"],
            "FileOpRequest serialization no longer matches the wire contract"
        );
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            fixture["response"],
            "FileOpResponse serialization no longer matches the wire contract"
        );

        let parsed: FileOpRequest = serde_json::from_value(fixture["request"].clone()).unwrap();
        assert!(matches!(parsed, FileOpRequest::List { op_id: 1, .. }));

        let parsed: FileOpResponse = serde_json::from_value(fixture["response"].clone()).unwrap();
        match parsed {
            FileOpResponse::Listing {
                entries, truncated, ..
            } => {
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].kind, FileKind::File);
                assert_eq!(entries[1].kind, FileKind::Dir);
                assert!(!truncated);
            }
            other => panic!("expected a Listing, got {other:?}"),
        }
    }

    #[test]
    fn file_op_error_round_trips() {
        for code in [
            FileOpError::NotFound,
            FileOpError::PermissionDenied,
            FileOpError::NotADirectory,
            FileOpError::NotRegularFile,
            FileOpError::TooLarge,
            FileOpError::TooManyOpenFiles,
            FileOpError::Io,
        ] {
            let encoded = serde_json::to_string(&FileOpResponse::Error {
                op_id: 3,
                code,
                message: "boom".to_string(),
            })
            .unwrap();
            let decoded: FileOpResponse = serde_json::from_str(&encoded).unwrap();
            match decoded {
                FileOpResponse::Error { code: got, .. } => assert_eq!(got, code),
                other => panic!("expected an Error, got {other:?}"),
            }
        }
    }

    #[test]
    fn unknown_command_variant_round_trips() {
        // A daemon reporting a failed unknown command must not itself produce a
        // payload the api can't parse.
        let request = SafeCommandRequest {
            id: 9,
            command: SafeCommandTx::Unknown,
            continue_on_error: false,
        };
        let encoded = serde_json::to_string(&request).unwrap();
        let decoded: SafeCommandRequest = serde_json::from_str(&encoded).unwrap();
        assert!(matches!(decoded.command, SafeCommandTx::Unknown));
    }
}
