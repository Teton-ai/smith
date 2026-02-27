use crate::magic::MagicHandle;
use crate::shutdown::ShutdownHandler;
use crate::utils::schema::{
    InterfaceType, Network, NetworkDetails, NetworkInfo, SafeCommandResponse, SafeCommandRx,
    SpeedSample,
};
use anyhow::{Context, Result};
use chrono::Utc;
use reqwest::Client;
use std::time::{Duration, Instant};
use tokio::{process::Command, time::timeout};

pub(super) async fn execute(id: i32, network: Network) -> SafeCommandResponse {
    let network_name = network.name;
    let network_ssid = network.ssid.unwrap_or(network_name.clone());

    // Initially attempt to connect to the network by network name. In case connecting
    // to the WiFi fails, add a new connection and make another attempt after.
    //
    // Before adding a connection, delete any existing connections with the same name.
    // This is done in order to cater use cases such as password changes.

    // Try to connect to existing connection
    let mut cmd = Command::new("nmcli");
    cmd.arg("c").arg("up").arg(&network_name);

    if let Ok(output) = execute_nmcli_command(cmd).await
        && output.status.success()
    {
        let (status_code, response) = process_output(output);
        return SafeCommandResponse {
            id,
            command: response,
            status: status_code,
        };
    }
    // Connection failed, delete old connection and create new one
    // Delete existing connection (ignore errors if it doesn't exist)
    let mut delete_cmd = Command::new("nmcli");
    delete_cmd
        .arg("connection")
        .arg("delete")
        .arg(&network_name);
    let _ = execute_nmcli_command(delete_cmd).await;

    // Add new connection
    let mut add_cmd = Command::new("nmcli");
    add_cmd.args([
        "connection",
        "add",
        "type",
        "wifi",
        "con-name",
        &network_name,
        "ssid",
        &network_ssid,
        "autoconnect",
        "yes",
        "connection.autoconnect-priority",
        "500",
        "save",
        "yes",
    ]);

    if let Some(password) = network.password {
        add_cmd.args(["wifi-sec.key-mgmt", "wpa-psk", "wifi-sec.psk", &password]);
    }

    match execute_nmcli_command(add_cmd).await {
        Ok(output) if output.status.success() => {
            // Connection added successfully, now connect
            let mut connect_cmd = Command::new("nmcli");
            connect_cmd.args(["c", "up"]).arg(&network_name);

            match execute_nmcli_command(connect_cmd).await {
                Ok(output) => {
                    let (status_code, response) = process_output(output);
                    SafeCommandResponse {
                        id,
                        command: response,
                        status: status_code,
                    }
                }
                Err(e) => SafeCommandResponse {
                    id,
                    command: SafeCommandRx::WifiConnect {
                        stdout: "".to_string(),
                        stderr: format!("Error connecting after adding connection: {}", e),
                    },
                    status: -1,
                },
            }
        }
        Ok(output) => {
            let (status_code, response) = process_output(output);
            SafeCommandResponse {
                id,
                command: response,
                status: status_code,
            }
        }
        Err(e) => SafeCommandResponse {
            id,
            command: SafeCommandRx::WifiConnect {
                stdout: "".to_string(),
                stderr: format!("Error adding connection: {}", e),
            },
            status: -1,
        },
    }
}

async fn execute_nmcli_command(mut cmd: Command) -> Result<std::process::Output> {
    let future = cmd.kill_on_drop(true).output();

    match timeout(Duration::from_secs(60), future).await {
        Ok(output) => output.context("Failed to run nmcli command"),
        Err(_) => Err(anyhow::anyhow!("Timeout running nmcli command (60s)")),
    }
}

fn process_output(output: std::process::Output) -> (i32, SafeCommandRx) {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let status_code = output.status.code().unwrap_or(-1);

    (status_code, SafeCommandRx::WifiConnect { stdout, stderr })
}

pub(super) async fn test_network(id: i32) -> SafeCommandResponse {
    let result = perform_network_test().await;
    SafeCommandResponse {
        id,
        command: SafeCommandRx::TestNetwork {
            bytes_downloaded: result.bytes_downloaded,
            duration_ms: result.duration_ms,
            bytes_uploaded: result.bytes_uploaded,
            upload_duration_ms: result.upload_duration_ms,
            timed_out: result.timed_out,
        },
        status: if result.timed_out || result.error {
            -1
        } else {
            0
        },
    }
}

const NETWORK_TEST_TIMEOUT: Duration = Duration::from_secs(30);

struct NetworkTestResult {
    bytes_downloaded: usize,
    duration_ms: u64,
    bytes_uploaded: Option<usize>,
    upload_duration_ms: Option<u64>,
    timed_out: bool,
    error: bool,
}

async fn perform_network_test() -> NetworkTestResult {
    match perform_network_test_inner().await {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Network test failed: {}", e);
            NetworkTestResult {
                bytes_downloaded: 0,
                duration_ms: 0,
                bytes_uploaded: None,
                upload_duration_ms: None,
                timed_out: false,
                error: true,
            }
        }
    }
}

async fn perform_network_test_inner() -> Result<NetworkTestResult> {
    let shutdown = ShutdownHandler::new();
    let configuration = MagicHandle::new(shutdown.signals());
    configuration.load(None).await;

    let server_api_url = configuration.get_server().await;
    let download_url = format!("{}/network/test-file", server_api_url);
    let upload_url = format!("{}/network/test-upload", server_api_url);

    // No global timeout on client - we handle timeouts per-phase
    let client = Client::builder()
        .build()
        .context("Failed to create HTTP client")?;

    // Download test with timeout
    let download_start = Instant::now();
    let mut downloaded: usize = 0;
    let mut download_timed_out = false;

    let response = match timeout(NETWORK_TEST_TIMEOUT, client.get(&download_url).send()).await {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => return Err(e.into()),
        Err(_) => {
            return Ok(NetworkTestResult {
                bytes_downloaded: 0,
                duration_ms: NETWORK_TEST_TIMEOUT.as_millis() as u64,
                bytes_uploaded: None,
                upload_duration_ms: None,
                timed_out: true,
                error: false,
            });
        }
    };

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Server returned status: {}",
            response.status()
        ));
    }

    let mut stream = response.bytes_stream();
    use futures_util::StreamExt;

    loop {
        let remaining = NETWORK_TEST_TIMEOUT.saturating_sub(download_start.elapsed());
        if remaining.is_zero() {
            download_timed_out = true;
            break;
        }

        match timeout(remaining, stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                downloaded += chunk.len();
            }
            Ok(Some(Err(e))) => return Err(e.into()),
            Ok(None) => break, // Stream finished
            Err(_) => {
                download_timed_out = true;
                break;
            }
        }
    }

    let download_duration_ms = download_start.elapsed().as_millis() as u64;

    // If download timed out, return partial results (skip upload)
    if download_timed_out {
        return Ok(NetworkTestResult {
            bytes_downloaded: downloaded,
            duration_ms: download_duration_ms,
            bytes_uploaded: None,
            upload_duration_ms: None,
            timed_out: true,
            error: false,
        });
    }

    // Upload test with timeout
    let upload_data = vec![0u8; downloaded];
    let upload_start = Instant::now();

    let upload_result = timeout(
        NETWORK_TEST_TIMEOUT,
        client.post(&upload_url).body(upload_data).send(),
    )
    .await;

    match upload_result {
        Ok(Ok(resp)) if resp.status().is_success() => {
            let upload_duration_ms = upload_start.elapsed().as_millis() as u64;
            Ok(NetworkTestResult {
                bytes_downloaded: downloaded,
                duration_ms: download_duration_ms,
                bytes_uploaded: Some(downloaded),
                upload_duration_ms: Some(upload_duration_ms),
                timed_out: false,
                error: false,
            })
        }
        Ok(Ok(resp)) => Err(anyhow::anyhow!(
            "Upload server returned status: {}",
            resp.status()
        )),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => {
            // Upload timed out - return download results with upload timeout
            Ok(NetworkTestResult {
                bytes_downloaded: downloaded,
                duration_ms: download_duration_ms,
                bytes_uploaded: Some(0),
                upload_duration_ms: Some(NETWORK_TEST_TIMEOUT.as_millis() as u64),
                timed_out: true,
                error: false,
            })
        }
    }
}

// Extended network test implementation

pub(super) async fn extended_network_test(id: i32, duration_minutes: u32) -> SafeCommandResponse {
    let result = perform_extended_network_test(duration_minutes).await;
    let status = if result.error.is_some() { -1 } else { 0 };
    SafeCommandResponse {
        id,
        command: SafeCommandRx::ExtendedNetworkTest {
            samples: result.samples,
            network_info: result.network_info,
            total_duration_ms: result.total_duration_ms,
            error: result.error,
        },
        status,
    }
}

struct ExtendedNetworkTestResult {
    samples: Vec<SpeedSample>,
    network_info: Option<NetworkInfo>,
    total_duration_ms: u64,
    error: Option<String>,
}

async fn perform_extended_network_test(duration_minutes: u32) -> ExtendedNetworkTestResult {
    let start = Instant::now();
    let deadline = Duration::from_secs(duration_minutes as u64 * 60);
    let mut samples = Vec::new();

    // Loop existing speedtest until duration reached
    while start.elapsed() < deadline {
        let sample_start = Utc::now();
        let result = perform_network_test().await;

        let download_mbps = calculate_mbps(result.bytes_downloaded, result.duration_ms);
        let upload_mbps = result
            .bytes_uploaded
            .zip(result.upload_duration_ms)
            .map(|(bytes, ms)| calculate_mbps(bytes, ms));

        samples.push(SpeedSample {
            started_at: sample_start,
            download_bytes: result.bytes_downloaded,
            download_mbps,
            upload_bytes: result.bytes_uploaded,
            upload_mbps,
            duration_ms: result.duration_ms + result.upload_duration_ms.unwrap_or(0),
            timed_out: result.timed_out,
        });
    }

    // Collect network info once at end
    let network_info = collect_network_info().await.ok();

    ExtendedNetworkTestResult {
        samples,
        network_info,
        total_duration_ms: start.elapsed().as_millis() as u64,
        error: None,
    }
}

fn calculate_mbps(bytes: usize, duration_ms: u64) -> f64 {
    if duration_ms == 0 {
        return 0.0;
    }
    let bits = bytes as f64 * 8.0;
    let seconds = duration_ms as f64 / 1000.0;
    bits / seconds / 1_000_000.0
}

// Network info collection

async fn collect_network_info() -> Result<NetworkInfo> {
    // Try WiFi first (most common on edge devices)
    if let Ok(wifi_info) = collect_wifi_info().await {
        return Ok(wifi_info);
    }

    // Try Ethernet
    if let Ok(eth_info) = collect_ethernet_info().await {
        return Ok(eth_info);
    }

    // Try LTE/modem
    if let Ok(lte_info) = collect_lte_info().await {
        return Ok(lte_info);
    }

    Err(anyhow::anyhow!("No network interface detected"))
}

async fn collect_wifi_info() -> Result<NetworkInfo> {
    // Get wireless interface name using iw dev
    let output = Command::new("iw")
        .arg("dev")
        .kill_on_drop(true)
        .output()
        .await
        .context("Failed to run iw dev")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!("iw dev failed"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let interface_name = parse_iw_interface(&stdout)?;

    // Get detailed link info
    let link_output = Command::new("iw")
        .args(["dev", &interface_name, "link"])
        .kill_on_drop(true)
        .output()
        .await
        .context("Failed to run iw link")?;

    if !link_output.status.success() {
        return Err(anyhow::anyhow!("iw link failed"));
    }

    let link_stdout = String::from_utf8_lossy(&link_output.stdout);

    // Check if connected
    if link_stdout.contains("Not connected") {
        return Err(anyhow::anyhow!("WiFi not connected"));
    }

    let (ssid, signal_dbm, frequency_mhz, vht_mcs, vht_nss, channel_width_mhz) =
        parse_iw_link(&link_stdout);

    Ok(NetworkInfo {
        interface_type: InterfaceType::Wifi,
        interface_name,
        details: NetworkDetails::Wifi {
            ssid,
            signal_dbm,
            frequency_mhz,
            vht_mcs,
            vht_nss,
            channel_width_mhz,
        },
    })
}

fn parse_iw_interface(output: &str) -> Result<String> {
    // Parse output like:
    // phy#0
    //     Interface wlan0
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Interface ") {
            return Ok(trimmed.strip_prefix("Interface ").unwrap_or("").to_string());
        }
    }
    Err(anyhow::anyhow!("No wireless interface found"))
}

fn parse_iw_link(
    output: &str,
) -> (
    Option<String>,
    Option<i32>,
    Option<u32>,
    Option<u8>,
    Option<u8>,
    Option<u8>,
) {
    let mut ssid = None;
    let mut signal_dbm = None;
    let mut frequency_mhz = None;
    let mut vht_mcs = None;
    let mut vht_nss = None;
    let mut channel_width_mhz = None;

    for line in output.lines() {
        let trimmed = line.trim();

        // SSID: VitaCare Living - Proctor
        if trimmed.starts_with("SSID:") {
            ssid = Some(
                trimmed
                    .strip_prefix("SSID:")
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            );
        }

        // signal: -67 dBm
        if trimmed.starts_with("signal:") {
            if let Some(sig_str) = trimmed.strip_prefix("signal:") {
                let sig_str = sig_str.trim().replace(" dBm", "");
                signal_dbm = sig_str.parse().ok();
            }
        }

        // freq: 5240
        if trimmed.starts_with("freq:") {
            if let Some(freq_str) = trimmed.strip_prefix("freq:") {
                frequency_mhz = freq_str.trim().parse().ok();
            }
        }

        // rx bitrate: 162.0 MBit/s VHT-MCS 4 40MHz VHT-NSS 2
        // tx bitrate: 40.5 MBit/s VHT-MCS 2 40MHz VHT-NSS 1
        if trimmed.contains("VHT-MCS") {
            // Parse VHT-MCS value
            if let Some(mcs_start) = trimmed.find("VHT-MCS ") {
                let after_mcs = &trimmed[mcs_start + 8..];
                if let Some(space_idx) = after_mcs.find(' ') {
                    vht_mcs = after_mcs[..space_idx].parse().ok();
                } else {
                    vht_mcs = after_mcs.parse().ok();
                }
            }

            // Parse VHT-NSS value
            if let Some(nss_start) = trimmed.find("VHT-NSS ") {
                let after_nss = &trimmed[nss_start + 8..];
                if let Some(space_idx) = after_nss.find(' ') {
                    vht_nss = after_nss[..space_idx].parse().ok();
                } else {
                    vht_nss = after_nss.parse().ok();
                }
            }

            // Parse channel width (e.g., "40MHz")
            if let Some(mhz_idx) = trimmed.find("MHz") {
                // Look backwards for the number
                let before_mhz = &trimmed[..mhz_idx];
                let parts: Vec<&str> = before_mhz.split_whitespace().collect();
                if let Some(last) = parts.last() {
                    channel_width_mhz = last.parse().ok();
                }
            }
        }
    }

    (
        ssid,
        signal_dbm,
        frequency_mhz,
        vht_mcs,
        vht_nss,
        channel_width_mhz,
    )
}

async fn collect_ethernet_info() -> Result<NetworkInfo> {
    // Find ethernet interface via /sys/class/net
    let mut interface_name = None;

    if let Ok(mut entries) = tokio::fs::read_dir("/sys/class/net").await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            // Look for eth* or en* interfaces (common naming conventions)
            if name.starts_with("eth") || name.starts_with("en") {
                // Check if it has carrier (is connected)
                let carrier_path = format!("/sys/class/net/{}/carrier", name);
                if let Ok(carrier) = tokio::fs::read_to_string(&carrier_path).await {
                    if carrier.trim() == "1" {
                        interface_name = Some(name);
                        break;
                    }
                }
            }
        }
    }

    let interface_name =
        interface_name.ok_or_else(|| anyhow::anyhow!("No ethernet interface found"))?;

    // Try ethtool first for detailed info
    let (speed_mbps, duplex, link_detected) = if let Ok(output) = Command::new("ethtool")
        .arg(&interface_name)
        .kill_on_drop(true)
        .output()
        .await
    {
        if output.status.success() {
            parse_ethtool_output(&String::from_utf8_lossy(&output.stdout))
        } else {
            // Fallback to sysfs
            get_ethernet_info_from_sysfs(&interface_name).await
        }
    } else {
        // Fallback to sysfs
        get_ethernet_info_from_sysfs(&interface_name).await
    };

    Ok(NetworkInfo {
        interface_type: InterfaceType::Ethernet,
        interface_name,
        details: NetworkDetails::Ethernet {
            speed_mbps,
            duplex,
            link_detected,
        },
    })
}

fn parse_ethtool_output(output: &str) -> (Option<u32>, Option<String>, bool) {
    let mut speed_mbps = None;
    let mut duplex = None;
    let mut link_detected = false;

    for line in output.lines() {
        let trimmed = line.trim();

        // Speed: 1000Mb/s
        if trimmed.starts_with("Speed:") {
            if let Some(speed_str) = trimmed.strip_prefix("Speed:") {
                let speed_str = speed_str.trim().replace("Mb/s", "");
                speed_mbps = speed_str.parse().ok();
            }
        }

        // Duplex: Full
        if trimmed.starts_with("Duplex:") {
            duplex = Some(
                trimmed
                    .strip_prefix("Duplex:")
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            );
        }

        // Link detected: yes
        if trimmed.starts_with("Link detected:") {
            link_detected = trimmed.contains("yes");
        }
    }

    (speed_mbps, duplex, link_detected)
}

async fn get_ethernet_info_from_sysfs(interface_name: &str) -> (Option<u32>, Option<String>, bool) {
    let speed_path = format!("/sys/class/net/{}/speed", interface_name);
    let carrier_path = format!("/sys/class/net/{}/carrier", interface_name);
    let duplex_path = format!("/sys/class/net/{}/duplex", interface_name);

    let speed_mbps = tokio::fs::read_to_string(&speed_path)
        .await
        .ok()
        .and_then(|s| s.trim().parse().ok());

    let duplex = tokio::fs::read_to_string(&duplex_path)
        .await
        .ok()
        .map(|s| s.trim().to_string());

    let link_detected = tokio::fs::read_to_string(&carrier_path)
        .await
        .ok()
        .map(|s| s.trim() == "1")
        .unwrap_or(false);

    (speed_mbps, duplex, link_detected)
}

async fn collect_lte_info() -> Result<NetworkInfo> {
    // List modems using mmcli
    let output = Command::new("mmcli")
        .arg("-L")
        .kill_on_drop(true)
        .output()
        .await
        .context("Failed to run mmcli -L")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!("mmcli -L failed"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse modem index from output like:
    // /org/freedesktop/ModemManager1/Modem/0 [Quectel] EC25
    let modem_index = parse_modem_index(&stdout)?;

    // Get detailed modem info
    let detail_output = Command::new("mmcli")
        .args(["-m", &modem_index.to_string()])
        .kill_on_drop(true)
        .output()
        .await
        .context("Failed to run mmcli -m")?;

    if !detail_output.status.success() {
        return Err(anyhow::anyhow!("mmcli -m failed"));
    }

    let detail_stdout = String::from_utf8_lossy(&detail_output.stdout);
    let (operator, signal_quality, access_technology) = parse_mmcli_output(&detail_stdout);

    Ok(NetworkInfo {
        interface_type: InterfaceType::Lte,
        interface_name: format!("modem{}", modem_index),
        details: NetworkDetails::Lte {
            operator,
            signal_quality,
            access_technology,
        },
    })
}

fn parse_modem_index(output: &str) -> Result<u32> {
    // Parse output like: /org/freedesktop/ModemManager1/Modem/0 [Quectel] EC25
    for line in output.lines() {
        if line.contains("/Modem/") {
            if let Some(modem_part) = line.split("/Modem/").nth(1) {
                if let Some(space_idx) = modem_part.find(' ') {
                    if let Ok(idx) = modem_part[..space_idx].parse() {
                        return Ok(idx);
                    }
                } else if let Ok(idx) = modem_part.trim().parse() {
                    return Ok(idx);
                }
            }
        }
    }
    Err(anyhow::anyhow!("No modem found"))
}

fn parse_mmcli_output(output: &str) -> (Option<String>, Option<i32>, Option<String>) {
    let mut operator = None;
    let mut signal_quality = None;
    let mut access_technology = None;

    for line in output.lines() {
        let trimmed = line.trim();

        // operator name: Verizon
        if trimmed.contains("operator name:") {
            operator = trimmed.split(':').nth(1).map(|s| s.trim().to_string());
        }

        // signal quality: 75% (recent)
        if trimmed.contains("signal quality:") {
            if let Some(quality_part) = trimmed.split(':').nth(1) {
                let cleaned = quality_part.trim().replace('%', "");
                let quality_str = cleaned.split_whitespace().next().unwrap_or("");
                signal_quality = quality_str.parse().ok();
            }
        }

        // access tech: lte
        if trimmed.contains("access tech:") {
            access_technology = trimmed.split(':').nth(1).map(|s| s.trim().to_string());
        }
    }

    (operator, signal_quality, access_technology)
}
