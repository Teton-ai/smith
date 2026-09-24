use pnet::datalink;
use pnet::datalink::NetworkInterface;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::OnceLock;
use tracing::{error, info};

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct Smith {
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct OsRelease {
    pub pretty_name: String,
    pub version_id: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct DeviceTree {
    pub serial_number: String,
    pub model: Option<String>,
    pub compatible: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct ProcStat {
    pub btime: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct Proc {
    pub version: String,
    pub stat: ProcStat,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct NetworkItem {
    pub ips: Vec<String>,
    pub mac_address: String,
    /// Same addresses as `ips` but with their prefix (e.g. `192.168.1.23/24`); `ips` is kept
    /// as-is for older api versions.
    #[serde(default)]
    pub addresses: Vec<String>,
    /// IPv4 default gateway reached through this interface. The api groups devices sharing
    /// a gateway MAC and subnet into the same LAN.
    #[serde(default)]
    pub gateway: Option<Gateway>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct Gateway {
    pub ip: String,
    pub mac_address: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct Network {
    pub interfaces: HashMap<String, NetworkItem>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Default, Clone)]
pub struct NetworkConfig {
    pub connection_profile_name: String,
    pub connection_profile_uuid: String,
    pub device_type: String,
    pub device_name: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Default, Clone)]
pub struct ConnectionStatus {
    pub connection_name: String,
    pub connection_state: String,
    pub device_type: String,
    pub device_name: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
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

    content
        .lines()
        .find(|line| line.starts_with("btime"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

static DUMMY_SERIAL_NUMBER: OnceLock<String> = OnceLock::new();

fn get_dummy_serial_number() -> String {
    #[cfg(debug_assertions)]
    {
        use uuid::Uuid;
        DUMMY_SERIAL_NUMBER
            .get_or_init(||
                // This needs to be at least 11 characters long because of the sql query in `/devices/{device_id}`
                Uuid::new_v4().to_string())
            .to_string()
    }

    #[cfg(not(debug_assertions))]
    {
        panic!("The device should have a serial number")
    }
}

pub fn get_serial_number() -> String {
    get_raw_serial_number()
        .unwrap_or_else(get_dummy_serial_number)
        .trim()
        .trim_matches(char::is_whitespace)
        .trim_matches(char::from(0))
        .to_owned()
}

fn get_docker_container_name() -> Option<String> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    // Minimum length required by the SQL query in `/devices/{device_id}`;
    // also filters out short auto-generated names, accepting stable Compose names like `smith-device-1`.
    const MIN_SERIAL_LEN: usize = 11;

    let container_id = std::fs::read_to_string("/etc/hostname").ok()?;
    let container_id = container_id.trim();

    let mut stream = UnixStream::connect("/var/run/docker.sock").ok()?;
    let timeout = Some(Duration::from_secs(2));
    stream.set_read_timeout(timeout).ok()?;
    stream.set_write_timeout(timeout).ok()?;

    let request = format!(
        "GET /containers/{}/json HTTP/1.0\r\nHost: localhost\r\n\r\n",
        container_id
    );
    stream.write_all(request.as_bytes()).ok()?;

    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;

    let body = response.split("\r\n\r\n").nth(1)?;
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let name = json["Name"].as_str()?.trim_start_matches('/').to_owned();

    if name.len() >= MIN_SERIAL_LEN {
        Some(name)
    } else {
        None
    }
}

pub fn get_raw_serial_number() -> Option<String> {
    // Check if we're running in a docker container (in which case it's a dev env), and use the container's name.
    if std::fs::metadata("/.dockerenv").is_ok()
        && let Some(name) = get_docker_container_name()
    {
        return Some(name);
    }

    // Check if we're on a Jetson and if so, use the serial number
    if let Ok(jetson_serial_number) =
        std::fs::read_to_string("/sys/firmware/devicetree/base/serial-number")
    {
        return Some(jetson_serial_number);
    }

    // Check if we're on the overview screen LENOVOS and if so, use the product serial
    if let Ok(overview_board_vendor) = std::fs::read_to_string("/sys/class/dmi/id/board_vendor")
        && overview_board_vendor.trim() == "LENOVO"
        && let Ok(product_serial) = std::fs::read_to_string("/sys/class/dmi/id/product_serial")
    {
        return Some(product_serial);
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

    // Missing on non-Linux dev machines; the gateway is then simply not reported.
    let routes = tokio::fs::read_to_string("/proc/net/route")
        .await
        .unwrap_or_default();
    let arp = tokio::fs::read_to_string("/proc/net/arp")
        .await
        .unwrap_or_default();
    let mut gateways = parse_default_gateways(&routes);

    for interface in network_interfaces {
        let ips = get_ips_from_interface(&interface);
        let addresses = interface
            .ips
            .iter()
            .map(|ip_network| ip_network.to_string())
            .collect();
        let mac_address = interface
            .mac
            .map_or_else(|| "Unknown".to_string(), |mac| mac.to_string());
        let gateway = gateways.remove(&interface.name).map(|ip| Gateway {
            ip: ip.to_string(),
            mac_address: find_arp_mac(&arp, ip, &interface.name),
        });

        interfaces.insert(
            interface.name.clone(),
            NetworkItem {
                ips,
                mac_address,
                addresses,
                gateway,
            },
        );
    }

    Network { interfaces }
}

/// Default IPv4 gateway per interface from `/proc/net/route`, keeping the lowest metric
/// when an interface has several default routes.
fn parse_default_gateways(routes: &str) -> HashMap<String, Ipv4Addr> {
    const RTF_UP: u32 = 0x1;
    const RTF_GATEWAY: u32 = 0x2;

    let mut gateways: HashMap<String, (u32, Ipv4Addr)> = HashMap::new();
    for line in routes.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [iface, destination, gateway, flags, _, _, metric, mask, ..] = fields.as_slice() else {
            continue;
        };
        let hex = |s: &str| u32::from_str_radix(s, 16).ok();
        let (Some(destination), Some(gateway), Some(flags), Some(mask), Ok(metric)) = (
            hex(destination),
            hex(gateway),
            hex(flags),
            hex(mask),
            metric.parse::<u32>(),
        ) else {
            continue;
        };
        if destination != 0 || mask != 0 || flags & (RTF_UP | RTF_GATEWAY) != RTF_UP | RTF_GATEWAY {
            continue;
        }
        // The kernel prints the address as a native-endian u32 of its network-order bytes.
        let ip = Ipv4Addr::from(gateway.to_ne_bytes());
        match gateways.get(*iface) {
            Some((best, _)) if *best <= metric => {}
            _ => {
                gateways.insert((*iface).to_string(), (metric, ip));
            }
        }
    }
    gateways
        .into_iter()
        .map(|(iface, (_, ip))| (iface, ip))
        .collect()
}

/// MAC of `ip` on `iface` from `/proc/net/arp`, skipping incomplete entries.
fn find_arp_mac(arp: &str, ip: Ipv4Addr, iface: &str) -> Option<String> {
    let ip = ip.to_string();
    arp.lines().skip(1).find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [entry_ip, _, flags, mac, _, device, ..] = fields.as_slice() else {
            return None;
        };
        (*entry_ip == ip && *device == iface && *flags != "0x0" && *mac != "00:00:00:00:00:00")
            .then(|| mac.to_lowercase())
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    const ROUTES: &str = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
wlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0
eth0\t00000000\t0100000A\t0003\t0\t0\t100\t00000000\t0\t0\t0
eth0\t00000000\tNOTHEX\t0003\t0\t0\t50\t00000000\t0\t0\t0
eth0\t0000000A\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0
docker0\t000011AC\t00000000\t0001\t0\t0\t0\t0000FFFF\t0\t0\t0
";

    const ARP: &str = "\
IP address       HW type     Flags       HW address            Mask     Device
192.168.1.1      0x1         0x2         AA:BB:CC:DD:EE:01     *        wlan0
10.0.0.1         0x1         0x0         00:00:00:00:00:00     *        eth0
192.168.1.1      0x1         0x2         aa:bb:cc:dd:ee:99     *        eth0
";

    #[test]
    fn parses_default_gateways() {
        let gateways = parse_default_gateways(ROUTES);
        assert_eq!(gateways.get("wlan0"), Some(&Ipv4Addr::new(192, 168, 1, 1)));
        // Malformed line is skipped, the remaining default route wins.
        assert_eq!(gateways.get("eth0"), Some(&Ipv4Addr::new(10, 0, 0, 1)));
        assert!(!gateways.contains_key("docker0"));
    }

    #[test]
    fn picks_lowest_metric_gateway() {
        let routes = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask
eth0\t00000000\t0100000A\t0003\t0\t0\t100\t00000000
eth0\t00000000\t0200000A\t0003\t0\t0\t50\t00000000
eth0\t00000000\t0300000A\t0003\t0\t0\t200\t00000000
";
        let gateways = parse_default_gateways(routes);
        assert_eq!(gateways.get("eth0"), Some(&Ipv4Addr::new(10, 0, 0, 2)));
    }

    #[test]
    fn finds_gateway_mac_on_matching_interface() {
        let ip = Ipv4Addr::new(192, 168, 1, 1);
        assert_eq!(
            find_arp_mac(ARP, ip, "wlan0"),
            Some("aa:bb:cc:dd:ee:01".to_string())
        );
        assert_eq!(
            find_arp_mac(ARP, ip, "eth0"),
            Some("aa:bb:cc:dd:ee:99".to_string())
        );
        assert_eq!(find_arp_mac(ARP, Ipv4Addr::new(10, 0, 0, 1), "eth0"), None);
    }
}
