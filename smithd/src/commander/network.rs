use crate::utils::files::{load_last_applied_networks, save_last_applied_networks};
use crate::utils::schema::{
    ConditionReason, ConditionState, IntentNetwork, InterfaceType, NMProfile, Network,
    NetworkCondition, NetworkDetails, NetworkInfo, SafeCommandResponse, SafeCommandRx, SpeedSample,
    WifiNetwork,
};
use anyhow::{Context, Result};
use chrono::Utc;
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::{
    process::Command,
    time::{sleep, timeout},
};

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

// ACTIVE field in terse mode always returns "no" on NetworkManager 1.22.x
// (Ubuntu 20.04 / L4T 35.3.1). Use DEVICE instead: a non-empty, non-"--"
// device name means the profile is currently active.
fn parse_nm_connection_list_line(line: &str) -> Option<(String, bool, String)> {
    let parts = split_terse_line(line, 4);
    if parts.len() == 4 && parts[1] == "802-11-wireless" {
        let is_active = !parts[2].is_empty() && parts[2] != "--";
        Some((parts[0].clone(), is_active, parts[3].clone()))
    } else {
        None
    }
}

async fn get_nm_wifi_profiles() -> Result<Vec<NMProfile>> {
    let mut list_cmd = Command::new("nmcli");
    // Include UUID so each profile is queried by UUID in step 2, avoiding ambiguous
    // output when multiple connections share the same connection.id (name).
    list_cmd.args(["-t", "-f", "NAME,TYPE,DEVICE,UUID", "connection", "show"]);

    let entries = match execute_nmcli_command(list_cmd).await {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout
                .lines()
                .filter_map(parse_nm_connection_list_line)
                .collect::<Vec<_>>()
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("nmcli connection show failed: {stderr}"));
        }
        Err(e) => return Err(e.context("running nmcli connection show")),
    };

    let mut profiles = Vec::new();
    for (name, is_active, uuid) in entries {
        let mut detail_cmd = Command::new("nmcli");
        detail_cmd.args(["--show-secrets", "-t", "connection", "show", "uuid", &uuid]);

        let (ssid, password) = match execute_nmcli_command(detail_cmd).await {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut ssid = None;
                let mut psk = None;
                for line in stdout.lines() {
                    if let Some(val) = line.strip_prefix("802-11-wireless.ssid:") {
                        if !val.is_empty() {
                            ssid = Some(unescape_terse(val));
                        }
                    } else if let Some(val) = line.strip_prefix("802-11-wireless-security.psk:")
                        && !val.is_empty()
                        && val != "--"
                    {
                        psk = Some(unescape_terse(val));
                    }
                }
                (ssid, psk)
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!("nmcli connection show uuid {uuid} failed: {stderr}");
                (None, None)
            }
            Err(e) => {
                tracing::warn!("Failed to run nmcli connection show uuid {uuid}: {e}");
                (None, None)
            }
        };

        profiles.push(NMProfile {
            name,
            ssid,
            password,
            is_active,
        });
    }

    Ok(profiles)
}

pub(crate) async fn execute_report_nm_profiles(id: i32) -> SafeCommandResponse {
    match get_nm_wifi_profiles().await {
        Ok(profiles) => {
            let partial = profiles.iter().any(|p| p.ssid.is_none());
            SafeCommandResponse {
                id,
                command: SafeCommandRx::ReportNMProfiles { profiles },
                status: if partial { 1 } else { 0 },
            }
        }
        Err(e) => {
            tracing::error!("get_nm_wifi_profiles failed: {e:#}");
            SafeCommandResponse {
                id,
                command: SafeCommandRx::ReportNMProfiles { profiles: vec![] },
                status: -1,
            }
        }
    }
}

const WIFI_SCAN_FIELDS: usize = 6;

pub(crate) async fn execute_wifi_scan(id: i32) -> SafeCommandResponse {
    let mut cmd = Command::new("nmcli");
    cmd.args([
        "-t",
        "-f",
        "SSID,BSSID,SIGNAL,RATE,SECURITY,CHAN",
        "device",
        "wifi",
        "list",
        "--rescan",
        "yes",
    ]);

    match execute_nmcli_command(cmd).await {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let networks = stdout.lines().filter_map(parse_wifi_scan_line).collect();
            SafeCommandResponse {
                id,
                command: SafeCommandRx::WifiScan { networks },
                status: 0,
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!("nmcli device wifi list failed: {stderr}");
            SafeCommandResponse {
                id,
                command: SafeCommandRx::WifiScan { networks: vec![] },
                status: -1,
            }
        }
        Err(e) => {
            tracing::error!("Failed to run nmcli device wifi list: {e}");
            SafeCommandResponse {
                id,
                command: SafeCommandRx::WifiScan { networks: vec![] },
                status: -1,
            }
        }
    }
}

fn parse_wifi_scan_line(line: &str) -> Option<WifiNetwork> {
    if line.is_empty() {
        return None;
    }
    let fields = split_terse_line(line, WIFI_SCAN_FIELDS);
    if fields.len() < WIFI_SCAN_FIELDS {
        tracing::warn!("Skipping malformed wifi scan line: {line}");
        return None;
    }

    let ssid_raw = unescape_terse(&fields[0]);
    let ssid = if ssid_raw.is_empty() {
        None
    } else {
        Some(ssid_raw)
    };
    let bssid = unescape_terse(&fields[1]);
    if ssid.is_none() && bssid.is_empty() {
        return None;
    }

    let signal = match parse_optional_i32(&fields[2]) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Skipping wifi scan line, bad signal {:?}: {e}", fields[2]);
            return None;
        }
    };
    // Rate comes as "130 Mbit/s"; keep only the numeric part. 802.11b rates
    // are fractional ("5.5 Mbit/s"), so parse as float and round to whole Mbps.
    let rate_token = fields[3].split(' ').next().unwrap_or("");
    let rate = match parse_optional_rate(rate_token) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Skipping wifi scan line, bad rate {:?}: {e}", fields[3]);
            return None;
        }
    };
    let security = if fields[4].is_empty() || fields[4] == "--" {
        None
    } else {
        Some(unescape_terse(&fields[4]))
    };
    let channel = match parse_optional_i32(&fields[5]) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Skipping wifi scan line, bad channel {:?}: {e}", fields[5]);
            return None;
        }
    };

    Some(WifiNetwork {
        ssid,
        bssid,
        signal,
        rate,
        security,
        channel,
    })
}

fn parse_optional_i32(s: &str) -> Result<Option<i32>, std::num::ParseIntError> {
    if s.is_empty() {
        Ok(None)
    } else {
        s.parse::<i32>().map(Some)
    }
}

fn parse_optional_rate(s: &str) -> Result<Option<i32>, std::num::ParseFloatError> {
    if s.is_empty() {
        Ok(None)
    } else {
        Ok(Some(s.parse::<f64>()?.round() as i32))
    }
}

fn unescape_terse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.peek() {
                Some(&':') => {
                    chars.next();
                    out.push(':');
                }
                Some(&'\\') => {
                    chars.next();
                    out.push('\\');
                }
                _ => out.push(ch),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn split_terse_line(line: &str, max_fields: usize) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek() == Some(&':') {
            chars.next();
            current.push(':');
        } else if ch == ':' {
            fields.push(current.clone());
            current.clear();
            if fields.len() == max_fields - 1 {
                while let Some(ch) = chars.next() {
                    if ch == '\\' && chars.peek() == Some(&':') {
                        chars.next();
                        current.push(':');
                    } else {
                        current.push(ch);
                    }
                }
                break;
            }
        } else {
            current.push(ch);
        }
    }
    fields.push(current);
    fields
}

fn process_output(output: std::process::Output) -> (i32, SafeCommandRx) {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let status_code = output.status.code().unwrap_or(-1);

    (status_code, SafeCommandRx::WifiConnect { stdout, stderr })
}

pub(super) async fn test_network(id: i32, server: &str) -> SafeCommandResponse {
    let result = perform_network_test(server).await;
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

async fn perform_network_test(server: &str) -> NetworkTestResult {
    match perform_network_test_inner(server).await {
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

async fn perform_network_test_inner(server_api_url: &str) -> Result<NetworkTestResult> {
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

pub(super) async fn extended_network_test(
    id: i32,
    duration_minutes: u32,
    server: &str,
) -> SafeCommandResponse {
    let result = perform_extended_network_test(duration_minutes, server).await;
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

async fn perform_extended_network_test(
    duration_minutes: u32,
    server: &str,
) -> ExtendedNetworkTestResult {
    let start = Instant::now();
    let deadline = Duration::from_secs(duration_minutes as u64 * 60);
    let mut samples = Vec::new();

    // Loop existing speedtest until duration reached
    while start.elapsed() < deadline {
        let iter_start = Instant::now();
        let sample_start = Utc::now();
        let result = perform_network_test(server).await;

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

        // Back off at least 1 second between iterations to avoid tight loops on fast failures
        let elapsed = iter_start.elapsed();
        if elapsed < Duration::from_secs(1) {
            sleep(Duration::from_secs(1) - elapsed).await;
        }
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

    let info = parse_iw_link(&link_stdout);

    Ok(NetworkInfo {
        interface_type: InterfaceType::Wifi,
        interface_name,
        details: NetworkDetails::Wifi {
            ssid: info.ssid,
            signal_dbm: info.signal_dbm,
            frequency_mhz: info.frequency_mhz,
            vht_mcs: info.vht_mcs,
            vht_nss: info.vht_nss,
            channel_width_mhz: info.channel_width_mhz,
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

struct IwLinkInfo {
    ssid: Option<String>,
    signal_dbm: Option<i32>,
    frequency_mhz: Option<u32>,
    vht_mcs: Option<u8>,
    vht_nss: Option<u8>,
    channel_width_mhz: Option<u8>,
}

fn parse_iw_link(output: &str) -> IwLinkInfo {
    let mut ssid = None;
    let mut signal_dbm = None;
    let mut frequency_mhz = None;
    let mut vht_mcs = None;
    let mut vht_nss = None;
    let mut channel_width_mhz = None;

    for line in output.lines() {
        let trimmed = line.trim();

        // SSID: VitaCare Living - Proctor
        if let Some(val) = trimmed.strip_prefix("SSID:") {
            ssid = Some(val.trim().to_string());
        }

        // signal: -67 dBm
        if let Some(sig_str) = trimmed.strip_prefix("signal:") {
            let sig_str = sig_str.trim().replace(" dBm", "");
            signal_dbm = sig_str.parse().ok();
        }

        // freq: 5240
        if let Some(freq_str) = trimmed.strip_prefix("freq:") {
            frequency_mhz = freq_str.trim().parse().ok();
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

    IwLinkInfo {
        ssid,
        signal_dbm,
        frequency_mhz,
        vht_mcs,
        vht_nss,
        channel_width_mhz,
    }
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
                if let Ok(carrier) = tokio::fs::read_to_string(&carrier_path).await
                    && carrier.trim() == "1"
                {
                    interface_name = Some(name);
                    break;
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
        if let Some(speed_str) = trimmed.strip_prefix("Speed:") {
            let speed_str = speed_str.trim().replace("Mb/s", "");
            speed_mbps = speed_str.parse().ok();
        }

        // Duplex: Full
        if let Some(val) = trimmed.strip_prefix("Duplex:") {
            duplex = Some(val.trim().to_string());
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
        if line.contains("/Modem/")
            && let Some(modem_part) = line.split("/Modem/").nth(1)
        {
            if let Some(space_idx) = modem_part.find(' ') {
                if let Ok(idx) = modem_part[..space_idx].parse() {
                    return Ok(idx);
                }
            } else if let Ok(idx) = modem_part.trim().parse() {
                return Ok(idx);
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
        if trimmed.contains("signal quality:")
            && let Some(quality_part) = trimmed.split(':').nth(1)
        {
            let cleaned = quality_part.trim().replace('%', "");
            let quality_str = cleaned.split_whitespace().next().unwrap_or("");
            signal_quality = quality_str.parse().ok();
        }

        // access tech: lte
        if trimmed.contains("access tech:") {
            access_technology = trimmed.split(':').nth(1).map(|s| s.trim().to_string());
        }
    }

    (operator, signal_quality, access_technology)
}

async fn nmcli_delete_profile(name: &str) -> Result<()> {
    let mut cmd = Command::new("nmcli");
    cmd.args(["connection", "delete", name]);
    let output = execute_nmcli_command(cmd).await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("nmcli delete {name} failed: {stderr}"));
    }
    Ok(())
}

async fn nmcli_update_priority(name: &str, priority: i32) -> Result<()> {
    let mut cmd = Command::new("nmcli");
    cmd.args([
        "connection",
        "modify",
        name,
        "connection.autoconnect",
        "yes",
        "connection.autoconnect-priority",
        &priority.to_string(),
    ]);
    let output = execute_nmcli_command(cmd).await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "nmcli set-priority {name} failed: {stderr}"
        ));
    }
    Ok(())
}

async fn nmcli_modify_profile(
    name: &str,
    ssid: &str,
    psk: Option<&str>,
    priority: i32,
) -> Result<()> {
    let mut cmd = Command::new("nmcli");
    cmd.args([
        "connection",
        "modify",
        name,
        "802-11-wireless.ssid",
        ssid,
        "connection.autoconnect",
        "yes",
        "connection.autoconnect-priority",
        &priority.to_string(),
    ]);
    if let Some(psk) = psk {
        cmd.args(["wifi-sec.key-mgmt", "wpa-psk", "wifi-sec.psk", psk]);
    } else {
        cmd.args(["wifi-sec.key-mgmt", "none", "wifi-sec.psk", ""]);
    }
    let output = execute_nmcli_command(cmd).await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("nmcli modify {name} failed: {stderr}"));
    }
    Ok(())
}

async fn nmcli_create_profile(
    profile_name: &str,
    ssid: &str,
    psk: Option<&str>,
    priority: i32,
) -> Result<()> {
    let mut cmd = Command::new("nmcli");
    cmd.args([
        "connection",
        "add",
        "type",
        "wifi",
        "con-name",
        profile_name,
        "ssid",
        ssid,
        "autoconnect",
        "yes",
        "connection.autoconnect-priority",
        &priority.to_string(),
        "save",
        "yes",
    ]);
    if let Some(psk) = psk {
        cmd.args(["wifi-sec.key-mgmt", "wpa-psk", "wifi-sec.psk", psk]);
    }
    let output = execute_nmcli_command(cmd).await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("nmcli add {profile_name} failed: {stderr}"));
    }
    Ok(())
}

fn classify_connect_error(stderr: &str) -> ConditionReason {
    let lower = stderr.to_lowercase();
    if lower.contains("secrets") || lower.contains("psk") || lower.contains("authentication") {
        ConditionReason::WrongPSK
    } else if lower.contains("no network with ssid") || lower.contains("not found") {
        ConditionReason::NotInRange
    } else {
        ConditionReason::NmcliError
    }
}

async fn connectivity_guard(
    profile_name: &str,
    ssid: &str,
    psk: &str,
    priority: i32,
) -> Result<(), ConditionReason> {
    let tmp_name = format!("tmp-{profile_name}");

    // Create temporary profile with new credentials.
    let mut add_cmd = Command::new("nmcli");
    add_cmd.args([
        "connection",
        "add",
        "type",
        "wifi",
        "con-name",
        &tmp_name,
        "ssid",
        ssid,
        "autoconnect",
        "no",
        "connection.autoconnect-priority",
        &priority.to_string(),
        "save",
        "yes",
        "wifi-sec.key-mgmt",
        "wpa-psk",
        "wifi-sec.psk",
        psk,
    ]);
    match execute_nmcli_command(add_cmd).await {
        Ok(out) if !out.status.success() => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            tracing::error!(
                "connectivity_guard: failed to create temp profile {tmp_name}: {stderr}"
            );
            return Err(ConditionReason::NmcliError);
        }
        Err(e) => {
            tracing::error!("connectivity_guard: failed to create temp profile {tmp_name}: {e:#}");
            return Err(ConditionReason::NmcliError);
        }
        Ok(_) => {}
    }

    // Try connecting with the new credentials.
    let mut up_cmd = Command::new("nmcli");
    up_cmd.args(["c", "up", &tmp_name]);
    let connect_stderr = match execute_nmcli_command(up_cmd).await {
        Ok(output) if output.status.success() => None,
        Ok(output) => Some(String::from_utf8_lossy(&output.stderr).to_string()),
        Err(e) => {
            tracing::error!("connectivity_guard: nmcli c up {tmp_name} error: {e:#}");
            Some(String::new())
        }
    };

    if connect_stderr.is_none() {
        // New credentials work. Delete old profile first, then rename temp to the
        // canonical profile_name and re-enable autoconnect.
        if let Err(e) = nmcli_delete_profile(profile_name).await {
            tracing::error!(
                "connectivity_guard: failed to delete old profile {profile_name}: {e:#}"
            );
        }
        let mut rename_cmd = Command::new("nmcli");
        rename_cmd.args([
            "connection",
            "modify",
            &tmp_name,
            "connection.id",
            profile_name,
            "connection.autoconnect",
            "yes",
        ]);
        let rename_ok = match execute_nmcli_command(rename_cmd).await {
            Ok(out) if !out.status.success() => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                tracing::error!(
                    "connectivity_guard: failed to rename {tmp_name} to {profile_name}: {stderr}"
                );
                false
            }
            Err(e) => {
                tracing::error!(
                    "connectivity_guard: failed to rename {tmp_name} to {profile_name}: {e:#}"
                );
                false
            }
            Ok(_) => true,
        };

        if !rename_ok {
            // Old profile is already deleted; re-enable autoconnect on the temp profile so
            // the device can reconnect after reboot, then surface the failure.
            if let Err(e) = nmcli_update_priority(&tmp_name, priority).await {
                tracing::error!(
                    "connectivity_guard: failed to re-enable autoconnect on {tmp_name}: {e:#}"
                );
            }
            return Err(ConditionReason::NmcliError);
        }

        Ok(())
    } else {
        // New credentials failed — restore original connection, remove temp profile.
        let mut up_old = Command::new("nmcli");
        up_old.args(["c", "up", profile_name]);
        if let Err(e) = execute_nmcli_command(up_old).await {
            tracing::warn!("connectivity_guard: failed to reconnect {profile_name}: {e:#}");
        }

        let reason = classify_connect_error(&connect_stderr.unwrap_or_default());

        if let Err(e) = nmcli_delete_profile(&tmp_name).await {
            tracing::error!("connectivity_guard: failed to delete temp profile {tmp_name}: {e:#}");
        }

        Err(reason)
    }
}

pub(crate) async fn execute_apply_networks(
    id: i32,
    version: i32,
    networks: Vec<IntentNetwork>,
) -> SafeCommandResponse {
    let all_profiles = match get_nm_wifi_profiles().await {
        Ok(profiles) => profiles,
        Err(e) => {
            tracing::error!("execute_apply_networks: failed to list NM profiles: {e:#}");
            return SafeCommandResponse {
                id,
                command: SafeCommandRx::ApplyNetworksResult {
                    applied_version: version,
                    conditions: vec![],
                },
                status: -1,
            };
        }
    };
    // Keyed by NM connection name (profile_name). Last entry wins for duplicate names,
    // which can occur with pre-existing user profiles; the apply loop handles this gracefully.
    let current_profiles: HashMap<String, NMProfile> = all_profiles
        .into_iter()
        .map(|p| (p.name.clone(), p))
        .collect();

    let last_applied = load_last_applied_networks()
        .await
        .into_iter()
        .collect::<HashSet<String>>();

    let n = networks.len();
    let intent_profile_names: HashSet<String> =
        networks.iter().map(|nw| nw.profile_name.clone()).collect();
    let mut conditions: Vec<NetworkCondition> = Vec::new();
    let mut applied_profile_names: Vec<String> = Vec::new();

    for (index, network) in networks.iter().enumerate() {
        let priority = ((n - index) * 10) as i32;
        let ssid = &network.ssid;
        let profile_name = &network.profile_name;
        let is_open = network.credentials.key_mgmt == "none";
        let psk = network.credentials.psk.as_deref();

        if !is_open && network.credentials.key_mgmt != "wpa-psk" {
            tracing::error!(
                "execute_apply_networks: unsupported key_mgmt '{}' for {profile_name}",
                network.credentials.key_mgmt
            );
            conditions.push(NetworkCondition {
                profile_name: profile_name.clone(),
                state: ConditionState::Failed,
                reason: Some(ConditionReason::NmcliError),
                message: Some(format!(
                    "unsupported key_mgmt: {}",
                    network.credentials.key_mgmt
                )),
            });
            continue;
        }

        let result = match current_profiles.get(profile_name.as_str()) {
            Some(profile) if profile.is_active => {
                if is_open {
                    nmcli_update_priority(profile_name, priority)
                        .await
                        .map_err(|e| {
                            tracing::error!(
                                "execute_apply_networks: set priority {profile_name} failed: {e:#}"
                            );
                            ConditionReason::NmcliError
                        })
                } else {
                    let Some(psk) = psk else {
                        tracing::error!(
                            "execute_apply_networks: wpa-psk network {profile_name} has no psk"
                        );
                        conditions.push(NetworkCondition {
                            profile_name: profile_name.clone(),
                            state: ConditionState::Failed,
                            reason: Some(ConditionReason::NmcliError),
                            message: Some("wpa-psk network has no psk".to_string()),
                        });
                        continue;
                    };
                    let ssid_matches = profile.ssid.as_deref() == Some(ssid.as_str());
                    let psk_matches = profile.password.as_deref() == Some(psk);
                    if ssid_matches && psk_matches {
                        // Nothing changed: only update priority to avoid an unnecessary reconnect.
                        // Note: profile.password is None when the PSK is stored in a system keyring
                        // rather than the NM config file. In that case this check always misses and
                        // the guard runs with the same PSK, which succeeds harmlessly.
                        nmcli_update_priority(profile_name, priority)
                            .await
                            .map_err(|e| {
                                tracing::error!(
                                    "execute_apply_networks: set priority {profile_name} failed: {e:#}"
                                );
                                ConditionReason::NmcliError
                            })
                    } else {
                        // SSID or PSK changed on an active profile: use connectivity guard to
                        // validate new credentials before committing (avoids stranding the device).
                        connectivity_guard(profile_name, ssid, psk, priority).await
                    }
                }
            }
            Some(_) => {
                // Inactive profile: overwrite SSID, credentials, and priority in place.
                nmcli_modify_profile(profile_name, ssid, psk, priority)
                    .await
                    .map_err(|e| {
                        tracing::error!(
                            "execute_apply_networks: modify {profile_name} failed: {e:#}"
                        );
                        ConditionReason::NmcliError
                    })
            }
            None => nmcli_create_profile(profile_name, ssid, psk, priority)
                .await
                .map_err(|e| {
                    tracing::error!("execute_apply_networks: create {profile_name} failed: {e:#}");
                    ConditionReason::NmcliError
                }),
        };

        match result {
            Ok(()) => {
                conditions.push(NetworkCondition {
                    profile_name: profile_name.clone(),
                    state: ConditionState::Applied,
                    reason: None,
                    message: None,
                });
                applied_profile_names.push(profile_name.clone());
            }
            Err(reason) => {
                conditions.push(NetworkCondition {
                    profile_name: profile_name.clone(),
                    state: ConditionState::Failed,
                    reason: Some(reason),
                    message: None,
                });
            }
        }
    }

    // Delete all NM profiles Smith previously managed that are no longer in intent.
    let mut successfully_deleted: HashSet<String> = HashSet::new();
    for profile_name in last_applied.difference(&intent_profile_names) {
        if current_profiles.contains_key(profile_name.as_str())
            && nmcli_delete_profile(profile_name).await.inspect_err(|e| {
                tracing::error!(
                    "execute_apply_networks: failed to delete removed profile {profile_name}: {e:#}"
                );
            }).is_err()
        {
            continue;
        }
        successfully_deleted.insert(profile_name.clone());
    }

    // Persist: union of previous and newly applied, minus what was cleanly deleted.
    // This prevents a failed run from erasing tracking of still-existing profiles.
    let mut new_last_applied = last_applied;
    for s in applied_profile_names {
        new_last_applied.insert(s);
    }
    for s in &successfully_deleted {
        new_last_applied.remove(s);
    }
    let new_last_applied: Vec<String> = new_last_applied.into_iter().collect();
    if let Err(e) = save_last_applied_networks(&new_last_applied).await {
        tracing::error!("execute_apply_networks: failed to persist last-applied list: {e:#}");
    }

    SafeCommandResponse {
        id,
        command: SafeCommandRx::ApplyNetworksResult {
            applied_version: version,
            conditions,
        },
        status: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_detection_profile_with_device_is_active() {
        // Active profile: DEVICE column contains a real interface name.
        // This is the case NM 1.22.x reported incorrectly via the ACTIVE field.
        let (name, is_active, uuid) =
            parse_nm_connection_list_line("HomeWifi:802-11-wireless:wlan0:abc-123").unwrap();
        assert_eq!(name, "HomeWifi");
        assert!(is_active);
        assert_eq!(uuid, "abc-123");
    }

    #[test]
    fn active_detection_profile_with_dash_is_inactive() {
        let (name, is_active, _) =
            parse_nm_connection_list_line("OfficeWifi:802-11-wireless:--:def-456").unwrap();
        assert_eq!(name, "OfficeWifi");
        assert!(!is_active);
    }

    #[test]
    fn active_detection_profile_with_empty_device_is_inactive() {
        let (name, is_active, _) =
            parse_nm_connection_list_line("GuestWifi:802-11-wireless::ghi-789").unwrap();
        assert_eq!(name, "GuestWifi");
        assert!(!is_active);
    }

    #[test]
    fn active_detection_skips_non_wifi_connections() {
        assert!(parse_nm_connection_list_line("Wired:802-3-ethernet:eth0:jkl-000").is_none());
        assert!(parse_nm_connection_list_line("lo:loopback:lo:mno-111").is_none());
    }

    #[test]
    fn active_detection_handles_escaped_colon_in_name() {
        let (name, is_active, _) =
            parse_nm_connection_list_line("My\\:Network:802-11-wireless:wlan0:pqr-222").unwrap();
        assert_eq!(name, "My:Network");
        assert!(is_active);
    }

    #[test]
    fn active_detection_full_output_mixed_profiles() {
        // Realistic nmcli -t -f NAME,TYPE,DEVICE,UUID connection show output:
        // the active profile has a device name; inactive ones have "--".
        let output = "\
HomeWifi:802-11-wireless:wlan0:uuid-1\n\
OfficeWifi:802-11-wireless:--:uuid-2\n\
Wired connection 1:802-3-ethernet:eth0:uuid-3\n\
BackupAP:802-11-wireless:--:uuid-4";

        let entries: Vec<(String, bool, String)> = output
            .lines()
            .filter_map(parse_nm_connection_list_line)
            .collect();

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].0, "HomeWifi");
        assert!(entries[0].1, "HomeWifi should be active (device=wlan0)");
        assert_eq!(entries[1].0, "OfficeWifi");
        assert!(!entries[1].1, "OfficeWifi should be inactive (device=--)");
        assert_eq!(entries[2].0, "BackupAP");
        assert!(!entries[2].1, "BackupAP should be inactive (device=--)");
    }

    const NM_CONNECTION_FIELDS: usize = 5;

    fn parse_nm_profile_line(line: &str) -> Option<NMProfile> {
        let fields = split_terse_line(line, NM_CONNECTION_FIELDS);
        if fields.len() < NM_CONNECTION_FIELDS || fields[1] != "802-11-wireless" {
            return None;
        }
        let ssid = if fields[2].is_empty() {
            None
        } else {
            Some(fields[2].clone())
        };
        let password = if fields[3].is_empty() || fields[3] == "--" {
            None
        } else {
            Some(fields[3].clone())
        };
        Some(NMProfile {
            name: fields[0].clone(),
            ssid,
            password,
            is_active: fields[4] == "activated",
        })
    }

    #[test]
    fn parse_nm_profiles_filters_wifi_and_extracts_fields() {
        let output = "\
CorpWifi:802-11-wireless:CorpWifi:secretpass:activated\n\
BackupAP:802-11-wireless:BackupAP::--\n\
Wired:802-3-ethernet:::--\n\
OpenNet:802-11-wireless:OpenNet:--:--";

        let profiles: Vec<NMProfile> = output.lines().filter_map(parse_nm_profile_line).collect();

        assert_eq!(profiles.len(), 3);

        assert_eq!(profiles[0].name, "CorpWifi");
        assert_eq!(profiles[0].ssid, Some("CorpWifi".to_string()));
        assert_eq!(profiles[0].password, Some("secretpass".to_string()));
        assert!(profiles[0].is_active);

        assert_eq!(profiles[1].name, "BackupAP");
        assert_eq!(profiles[1].ssid, Some("BackupAP".to_string()));
        assert_eq!(profiles[1].password, None);
        assert!(!profiles[1].is_active);

        assert_eq!(profiles[2].name, "OpenNet");
        assert_eq!(profiles[2].password, None);
        assert!(!profiles[2].is_active);
    }

    #[test]
    fn parse_nm_profiles_handles_colon_in_ssid_and_password() {
        let profile =
            parse_nm_profile_line("My\\:Network:802-11-wireless:My\\:Network:my\\:pass:activated")
                .unwrap();
        assert_eq!(profile.name, "My:Network");
        assert_eq!(profile.ssid, Some("My:Network".to_string()));
        assert_eq!(profile.password, Some("my:pass".to_string()));
        assert!(profile.is_active);
    }

    #[test]
    fn parse_nm_profiles_skips_non_wifi() {
        assert!(parse_nm_profile_line("eth0:802-3-ethernet:::--").is_none());
    }

    #[test]
    fn parse_wifi_scan_normal_ap() {
        let net =
            parse_wifi_scan_line("CorpWifi:AA\\:46\\:8D\\:29\\:A7\\:16:82:130 Mbit/s:WPA2:11")
                .unwrap();
        assert_eq!(net.ssid, Some("CorpWifi".to_string()));
        assert_eq!(net.bssid, "AA:46:8D:29:A7:16");
        assert_eq!(net.signal, Some(82));
        assert_eq!(net.rate, Some(130));
        assert_eq!(net.security, Some("WPA2".to_string()));
        assert_eq!(net.channel, Some(11));
    }

    #[test]
    fn parse_wifi_scan_hidden_ap_keeps_bssid() {
        let net =
            parse_wifi_scan_line(":AA\\:BB\\:CC\\:DD\\:EE\\:FF:60:270 Mbit/s:WPA2:36").unwrap();
        assert_eq!(net.ssid, None);
        assert_eq!(net.bssid, "AA:BB:CC:DD:EE:FF");
        assert_eq!(net.channel, Some(36));
    }

    #[test]
    fn parse_wifi_scan_open_network_has_no_security() {
        let net =
            parse_wifi_scan_line("CafeNet:11\\:22\\:33\\:44\\:55\\:66:45:54 Mbit/s:--:6").unwrap();
        assert_eq!(net.ssid, Some("CafeNet".to_string()));
        assert_eq!(net.security, None);
    }

    #[test]
    fn parse_wifi_scan_ssid_with_escaped_colon() {
        let net =
            parse_wifi_scan_line("My\\:Net:AA\\:BB\\:CC\\:DD\\:EE\\:FF:70:130 Mbit/s:WPA1 WPA2:1")
                .unwrap();
        assert_eq!(net.ssid, Some("My:Net".to_string()));
        assert_eq!(net.security, Some("WPA1 WPA2".to_string()));
    }

    #[test]
    fn parse_wifi_scan_fractional_rate_rounds() {
        let net =
            parse_wifi_scan_line("OldNet:AA\\:BB\\:CC\\:DD\\:EE\\:FF:30:5.5 Mbit/s:WEP:3").unwrap();
        assert_eq!(net.rate, Some(6));
    }

    #[test]
    fn parse_wifi_scan_skips_bad_rows() {
        // Both SSID and BSSID empty.
        assert!(parse_wifi_scan_line("::60:270 Mbit/s:WPA2:36").is_none());
        // Unparseable signal.
        assert!(
            parse_wifi_scan_line("Net:AA\\:BB\\:CC\\:DD\\:EE\\:FF:high:54 Mbit/s:WPA2:6").is_none()
        );
        // Too few fields.
        assert!(parse_wifi_scan_line("Net:AA\\:BB\\:CC\\:DD\\:EE\\:FF:60").is_none());
        assert!(parse_wifi_scan_line("").is_none());
    }
}
