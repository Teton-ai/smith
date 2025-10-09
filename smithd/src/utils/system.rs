use pnet::datalink;
use pnet::datalink::NetworkInterface;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use tracing::{error, info};

#[derive(Debug, Serialize, Deserialize)]
pub struct Smith {
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OsRelease {
    pub pretty_name: String,
    pub version_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceTree {
    pub serial_number: String,
    pub model: Option<String>,
    pub compatible: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcStat {
    pub btime: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Proc {
    pub version: String,
    pub stat: ProcStat,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkItem {
    pub ips: Vec<String>,
    pub mac_address: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Network {
    pub interfaces: HashMap<String, NetworkItem>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct NetworkConfig {
    pub connection_profile_name: String,
    pub connection_profile_uuid: String,
    pub device_type: String,
    pub device_name: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct ConnectionStatus {
    pub connection_name: String,
    pub connection_state: String,
    pub device_type: String,
    pub device_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SystemInfo {
    pub smith: Smith,
    pub hostname: String,
    pub os_release: OsRelease,
    pub proc: Proc,
    pub network: Network,
    pub device_tree: DeviceTree,
    pub connection_statuses: Vec<ConnectionStatus>,
}

impl SystemInfo {
    pub async fn new() -> SystemInfo {
        let os_release = tokio::fs::read_to_string("/etc/os-release")
            .await
            .unwrap_or_default();

        SystemInfo {
            smith: Smith {
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            hostname: tokio::fs::read_to_string("/etc/hostname")
                .await
                .unwrap_or_else(|_| "Unknown".to_string())
                .trim()
                .to_string(),
            os_release: OsRelease {
                pretty_name: os_release
                    .lines()
                    .find(|&line| line.starts_with("PRETTY_NAME="))
                    .map(|s| {
                        s.trim_start_matches("PRETTY_NAME=")
                            .trim_matches('"')
                            .to_string()
                    })
                    .unwrap_or_else(|| "Unknown".to_string()),
                version_id: os_release
                    .lines()
                    .find(|&line| line.starts_with("VERSION_ID="))
                    .map(|s| {
                        s.trim_start_matches("VERSION_ID=")
                            .trim_matches('"')
                            .to_string()
                    })
                    .unwrap_or_else(|| "Unknown".to_string()),
            },
            proc: Proc {
                version: tokio::fs::read_to_string("/proc/version")
                    .await
                    .unwrap_or_else(|_| "Unknown".to_string())
                    .split_whitespace()
                    .nth(2)
                    .unwrap_or("Unknown")
                    .to_string(),
                stat: ProcStat {
                    btime: get_last_boot_time().await,
                },
            },
            network: get_network_info().await,
            device_tree: DeviceTree {
                serial_number: tokio::fs::read_to_string("/proc/device-tree/serial-number")
                    .await
                    .unwrap_or_else(|_| "Unknown".to_string())
                    .trim_matches('\0')
                    .to_string(),
                model: tokio::fs::read_to_string("/proc/device-tree/model")
                    .await
                    .ok()
                    .map(|s| s.trim_matches('\0').to_string()),
                compatible: tokio::fs::read_to_string("/proc/device-tree/compatible")
                    .await
                    .ok()
                    .map(|s| {
                        s.split('\0')
                            .filter(|s| !s.is_empty())
                            .map(|s| s.trim().to_string())
                            .collect()
                    }),
            },
            connection_statuses: get_connection_statuses(),
        }
    }
    pub fn print(&self) {
        match serde_json::to_string_pretty(&self) {
            Ok(json) => info!("{}", json),
            Err(_) => error!("Failed to parse system info"),
        };
    }
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
    }
}

async fn get_last_boot_time() -> u64 {
    let content = tokio::fs::read_to_string("/proc/stat")
        .await
        .unwrap_or_default();

    let boot_time = content
        .lines()
        .find(|line| line.starts_with("btime"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    boot_time
}

pub fn get_serial_number() -> String {
    get_raw_serial_number()
        .unwrap_or_else(|| "1234567890123".to_owned())
        .trim()
        .trim_matches(char::is_whitespace)
        .trim_matches(char::from(0))
        .to_owned()
}

pub fn get_raw_serial_number() -> Option<String> {
    // Check if we're on a Jetson and if so, use the serial number
    if let Ok(jetson_serial_number) =
        std::fs::read_to_string("/sys/firmware/devicetree/base/serial-number")
    {
        return Some(jetson_serial_number);
    }

    // Check if we're on the overview screen LENOVOS and if so, use the product serial
    if let Ok(overview_board_vendor) = std::fs::read_to_string("/sys/class/dmi/id/board_vendor") {
        if overview_board_vendor.trim() == "LENOVO" {
            if let Ok(product_serial) = std::fs::read_to_string("/sys/class/dmi/id/product_serial")
            {
                return Some(product_serial);
            }
        }
    }

    // We must be on the GPU server, use the board serial
    if let Ok(server_board_serial) =
        std::fs::read_to_string("/sys/devices/virtual/dmi/id/board_serial")
    {
        return Some(server_board_serial);
    }

    // Default case: log and return None
    tracing::error!("Failed to read from all serial number files, using default value.");
    None
}

async fn get_network_info() -> Network {
    let mut interfaces = HashMap::new();
    let network_interfaces = datalink::interfaces();

    for interface in network_interfaces {
        let ips = get_ips_from_interface(&interface);
        let mac_address = interface
            .mac
            .map_or_else(|| "Unknown".to_string(), |mac| mac.to_string());

        interfaces.insert(interface.name.clone(), NetworkItem { ips, mac_address });
    }

    Network { interfaces }
}

fn get_ips_from_interface(interface: &NetworkInterface) -> Vec<String> {
    interface
        .ips
        .iter()
        .map(|ip_network| ip_network.ip().to_string())
        .collect()
}

/// Returns the list of connection statuses as provided by `nmcli`.
fn get_connection_statuses() -> Vec<ConnectionStatus> {
    let output = std::process::Command::new("nmcli")
        .args([
            "-t",
            "-f",
            "CONNECTION,STATE,TYPE,DEVICE",
            "device",
            "status",
        ])
        .output();

    if let Ok(output) = output {
        let network_statuses =
            std::str::from_utf8(&output.stdout).expect("error: failed to read CLI output");
        parse_connection_statuses(network_statuses)
    } else {
        vec![]
    }
}

/// Parses the list of connection statuses as provided by `nmcli -t -f CONNECTION,STATE,TYPE,DEVICE device status`.
///
/// Different fields within a line of the output are separated by `:`.
/// Example: Wired connection 1:connected:ethernet:eth1
fn parse_connection_statuses(statuses: &str) -> Vec<ConnectionStatus> {
    statuses
        .lines()
        .map(|line| {
            let fields: Vec<&str> = line.split(':').collect();

            // Early return empty network config in case parsing fails.
            if fields.len() != 4 {
                return ConnectionStatus::default();
            }

            ConnectionStatus {
                connection_name: fields[0].to_owned(),
                connection_state: fields[1].to_owned(),
                device_type: fields[2].to_owned(),
                device_name: fields[3].to_owned(),
            }
        })
        .collect()
}
