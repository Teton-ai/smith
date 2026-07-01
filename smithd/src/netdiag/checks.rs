//! The probes and the `run_sweep` entry point.
//!
//! Each check is self-contained and *defensive*: it owns its timeout and
//! converts any failure-to-run into a [`CheckStatus::Error`] outcome (or a
//! [`CheckStatus::Skip`] when a precondition wasn't met) rather than propagating
//! an error out of the sweep. So one probe blowing up — a missing `iw`, a hung
//! socket — can never abort the rest or crash the daemon. The checks run in
//! triage-ladder order, and later rungs skip (with a reason) when an earlier
//! rung they depend on failed.

use crate::netdiag::report::{
    AllowlistEntry, Category, CheckOutcome, DeviceInfo, DiagnosticReport, ItHandoff, ReportBuilder,
    Trigger,
};
use chrono::{Datelike, Utc};
use serde_json::json;
use std::time::Instant;
use tokio::net::{lookup_host, TcpStream};
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use tracing::warn;
use url::Url;

const CMD_TIMEOUT: Duration = Duration::from_secs(5);
const DNS_TIMEOUT: Duration = Duration::from_secs(5);
const TCP_TIMEOUT: Duration = Duration::from_secs(8);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Standard captive-portal probe: a clean network answers `204 No Content`;
/// anything else means our traffic is being intercepted or redirected.
const CAPTIVE_URL: &str = "http://connectivitycheck.gstatic.com/generate_204";

/// Inputs for a sweep. The caller (daemon / backend command / CLI) assembles
/// this from `magic.toml`; the engine stays free of config loading.
pub struct SweepOptions {
    pub trigger: Trigger,
    /// The backend base URL, e.g. `https://api.smith.teton.ai/smith`.
    pub server_url: String,
    /// Remote-support tunnel endpoint, if configured.
    pub tunnel: Option<(String, u16)>,
    pub device: DeviceInfo,
    /// WiFi interface to inspect (defaults to `wlan0`).
    pub wifi_iface: String,
}

impl SweepOptions {
    pub fn new(trigger: Trigger, server_url: impl Into<String>, device: DeviceInfo) -> Self {
        Self {
            trigger,
            server_url: server_url.into(),
            tunnel: None,
            device,
            wifi_iface: "wlan0".to_string(),
        }
    }
}

/// Run the full diagnostic sweep and return a sealed report. Never panics and
/// never returns an error — a failure to probe is itself recorded in the report.
pub async fn run_sweep(opts: SweepOptions) -> DiagnosticReport {
    let mut b = ReportBuilder::new(opts.trigger, opts.device.clone());

    // Device MAC into the IT handoff: prefer the registered value, fall back to
    // reading it off the interface.
    if let Some(mac) = opts.device.wifi_mac.clone() {
        b.handoff_mut().device_mac = Some(mac);
    } else if let Some(mac) = read_mac(&opts.wifi_iface).await {
        b.handoff_mut().device_mac = Some(mac);
    }

    // Rung 1: WiFi association.
    let wifi = check_wifi(&opts.wifi_iface, b.handoff_mut()).await;
    b.push(wifi);

    // Resolve the backend host/port up front; without it the egress rungs can't run.
    let (host, port) = match parse_host_port(&opts.server_url) {
        Some(hp) => hp,
        None => {
            b.push(CheckOutcome::error(
                "config.server_url",
                Category::Application,
                "Could not parse the configured server URL",
                format!("invalid url: {}", opts.server_url),
            ));
            return b.finish();
        }
    };

    // Rung 2: DNS.
    let (dns_outcome, dns_ok) = check_dns(&host).await;
    b.push(dns_outcome);

    // Rung 3: egress to the API.
    let (api_egress, api_reachable) = check_egress("egress.api", &host, port, dns_ok).await;
    b.push(api_egress);
    b.handoff_mut().required_allowlist.push(AllowlistEntry {
        host: host.clone(),
        port,
        proto: "tcp".to_string(),
        reachable: api_reachable,
    });

    // Rung 4: egress to the support tunnel (if configured).
    if let Some((thost, tport)) = opts.tunnel.clone() {
        let (t_out, t_reach) = check_egress("egress.tunnel", &thost, tport, true).await;
        b.push(t_out);
        b.handoff_mut().required_allowlist.push(AllowlistEntry {
            host: thost,
            port: tport,
            proto: "tcp".to_string(),
            reachable: t_reach,
        });
    }

    // Rungs 5–6 need an HTTP client.
    match reqwest::Client::builder().timeout(HTTP_TIMEOUT).build() {
        Ok(client) => {
            b.push(check_captive(&client).await);
            let tcp_ok = api_reachable == Some(true);
            b.push(check_app(&client, &opts.server_url, tcp_ok).await);
        }
        Err(e) => {
            b.push(CheckOutcome::error(
                "http.client",
                Category::Application,
                "Could not build the HTTP client for the application probes",
                e.to_string(),
            ));
        }
    }

    // Rung 7: clock sanity (a wrong clock breaks TLS in confusing ways).
    b.push(check_clock());

    b.finish()
}

/// Rung 1 — is the WiFi interface associated, and to what?
async fn check_wifi(iface: &str, handoff: &mut ItHandoff) -> CheckOutcome {
    let started = Instant::now();
    match run_cmd("iw", &["dev", iface, "link"], CMD_TIMEOUT).await {
        Ok(out) => {
            if out.trim().is_empty() || out.contains("Not connected") {
                return CheckOutcome::fail(
                    "wifi.link",
                    Category::Association,
                    format!("WiFi interface {iface} is not associated to any access point"),
                )
                .with_remediation(
                    "Verify the SSID and credentials, and that the access point is in range",
                )
                .with_duration_ms(ms(started));
            }
            let ssid = parse_field(&out, "SSID:");
            let signal = parse_signal(&out);
            if let Some(s) = &ssid {
                handoff.ssid = Some(s.clone());
            }
            let finding = format!(
                "Associated to {} ({})",
                ssid.clone().unwrap_or_else(|| "unknown SSID".to_string()),
                signal
                    .map(|s| format!("{s} dBm"))
                    .unwrap_or_else(|| "signal unknown".to_string())
            );
            CheckOutcome::pass("wifi.link", Category::Association, finding)
                .with_data(json!({ "ssid": ssid, "signal_dbm": signal, "iface": iface }))
                .with_duration_ms(ms(started))
        }
        Err(e) => CheckOutcome::error(
            "wifi.link",
            Category::Association,
            format!("Could not read WiFi link state for {iface}"),
            e,
        )
        .with_duration_ms(ms(started)),
    }
}

/// Rung 2 — does the backend hostname resolve? Returns whether it did, so the
/// egress rung can skip meaningfully.
async fn check_dns(host: &str) -> (CheckOutcome, bool) {
    let started = Instant::now();
    match timeout(DNS_TIMEOUT, lookup_host(format!("{host}:443"))).await {
        Ok(Ok(addrs)) => {
            let ips: Vec<String> = addrs.map(|a| a.ip().to_string()).collect();
            if ips.is_empty() {
                (
                    CheckOutcome::fail(
                        "dns.resolve",
                        Category::Dns,
                        format!("DNS returned no addresses for {host}"),
                    )
                    .with_remediation(format!("Ask IT why {host} does not resolve on this network"))
                    .with_duration_ms(ms(started)),
                    false,
                )
            } else {
                (
                    CheckOutcome::pass(
                        "dns.resolve",
                        Category::Dns,
                        format!("{host} resolves to {}", ips.join(", ")),
                    )
                    .with_data(json!({ "host": host, "addresses": ips }))
                    .with_duration_ms(ms(started)),
                    true,
                )
            }
        }
        Ok(Err(e)) => (
            CheckOutcome::fail(
                "dns.resolve",
                Category::Dns,
                format!("DNS resolution failed for {host}: {e}"),
            )
            .with_remediation(format!("Ask IT to permit DNS resolution of {host}"))
            .with_duration_ms(ms(started)),
            false,
        ),
        Err(_) => (
            CheckOutcome::fail(
                "dns.resolve",
                Category::Dns,
                format!("DNS resolution for {host} timed out"),
            )
            .with_remediation("Check DNS server reachability with IT")
            .with_duration_ms(ms(started)),
            false,
        ),
    }
}

/// How a TCP connect attempt resolved — the distinction (timeout vs refused vs
/// reset) tells IT what kind of egress policy is in play.
enum Conn {
    Open,
    Refused,
    Reset,
    Timeout,
    Err(String),
}

async fn probe_tcp(host: &str, port: u16) -> Conn {
    match timeout(TCP_TIMEOUT, TcpStream::connect(format!("{host}:{port}"))).await {
        Ok(Ok(_stream)) => Conn::Open,
        Ok(Err(e)) => match e.kind() {
            std::io::ErrorKind::ConnectionRefused => Conn::Refused,
            std::io::ErrorKind::ConnectionReset => Conn::Reset,
            _ => Conn::Err(e.to_string()),
        },
        Err(_) => Conn::Timeout,
    }
}

/// Rungs 3–4 — can we open a TCP connection to a required host:port? Returns the
/// reachability for the allowlist (`None` when we couldn't test).
async fn check_egress(id: &str, host: &str, port: u16, dns_ok: bool) -> (CheckOutcome, Option<bool>) {
    let started = Instant::now();
    if !dns_ok {
        return (
            CheckOutcome::skip(
                id,
                Category::Egress,
                format!("Did not test TCP {host}:{port}"),
                "DNS did not resolve (dns.resolve failed)",
            )
            .with_duration_ms(ms(started)),
            None,
        );
    }
    let target = format!("{host}:{port}");
    let allow = format!("Ask IT to allow outbound TCP {port} to {host}");
    match probe_tcp(host, port).await {
        Conn::Open => (
            CheckOutcome::pass(
                id,
                Category::Egress,
                format!("TCP connect to {target} succeeded"),
            )
            .with_duration_ms(ms(started)),
            Some(true),
        ),
        Conn::Timeout => (
            CheckOutcome::fail(
                id,
                Category::Egress,
                format!(
                    "TCP connect to {target} timed out — packets silently dropped (firewall likely blocking egress)"
                ),
            )
            .with_remediation(allow)
            .with_duration_ms(ms(started)),
            Some(false),
        ),
        Conn::Refused => (
            CheckOutcome::fail(
                id,
                Category::Egress,
                format!("TCP connect to {target} was actively refused"),
            )
            .with_remediation(allow)
            .with_duration_ms(ms(started)),
            Some(false),
        ),
        Conn::Reset => (
            CheckOutcome::fail(
                id,
                Category::Egress,
                format!("TCP connect to {target} was reset (likely deep-packet-inspection / filtering)"),
            )
            .with_remediation(allow)
            .with_duration_ms(ms(started)),
            Some(false),
        ),
        Conn::Err(e) => (
            CheckOutcome::error(
                id,
                Category::Egress,
                format!("Could not test TCP {target}"),
                e,
            )
            .with_duration_ms(ms(started)),
            None,
        ),
    }
}

/// Rung 5 — captive portal / interception detection.
async fn check_captive(client: &reqwest::Client) -> CheckOutcome {
    let started = Instant::now();
    match client.get(CAPTIVE_URL).send().await {
        Ok(resp) => {
            let code = resp.status().as_u16();
            if code == 204 {
                CheckOutcome::pass(
                    "captive.portal",
                    Category::Proxy,
                    "Clean internet egress (connectivity check returned 204)",
                )
                .with_duration_ms(ms(started))
            } else {
                CheckOutcome::fail(
                    "captive.portal",
                    Category::Proxy,
                    format!(
                        "Connectivity check returned HTTP {code} instead of 204 — a captive portal or proxy is intercepting traffic"
                    ),
                )
                .with_remediation(
                    "This network needs a portal sign-in or an exempted/registered connection for the device",
                )
                .with_data(json!({ "status": code }))
                .with_duration_ms(ms(started))
            }
        }
        // A hard egress failure is already covered by the egress rung; here we
        // only warn so we don't double-count it as a separate failure.
        Err(e) => CheckOutcome::warn(
            "captive.portal",
            Category::Proxy,
            "Could not reach the connectivity-check endpoint",
        )
        .with_reason(e.to_string())
        .with_duration_ms(ms(started)),
    }
}

/// Rung 6 — actual application request: separates "network is fine but our token
/// is rejected" (401) from TLS/proxy problems and from a healthy backend.
async fn check_app(client: &reqwest::Client, server: &str, tcp_ok: bool) -> CheckOutcome {
    let started = Instant::now();
    if !tcp_ok {
        return CheckOutcome::skip(
            "app.home",
            Category::Application,
            "Did not test the backend HTTP endpoint",
            "TCP egress to the API was not open (egress.api failed)",
        )
        .with_duration_ms(ms(started));
    }
    match client.get(server).send().await {
        Ok(resp) => {
            let code = resp.status().as_u16();
            if code == 401 {
                CheckOutcome::fail(
                    "app.home",
                    Category::Application,
                    "Backend reachable but it rejected the device token (HTTP 401)",
                )
                .with_remediation("Network path is fine — re-provision / re-register this device")
                .with_data(json!({ "status": code }))
                .with_duration_ms(ms(started))
            } else {
                CheckOutcome::pass(
                    "app.home",
                    Category::Application,
                    format!("Backend reachable (HTTP {code})"),
                )
                .with_data(json!({ "status": code }))
                .with_duration_ms(ms(started))
            }
        }
        Err(e) => {
            if e.is_timeout() {
                CheckOutcome::fail(
                    "app.home",
                    Category::Tls,
                    "Backend connection stalled after TCP connected (TLS/HTTP never completed)",
                )
                .with_remediation(
                    "Possible TLS interception, mandatory proxy, or an MTU blackhole — check with IT",
                )
                .with_duration_ms(ms(started))
            } else if e.is_connect() {
                CheckOutcome::fail(
                    "app.home",
                    Category::Tls,
                    format!("Could not establish TLS/HTTP to the backend: {e}"),
                )
                .with_remediation(
                    "Possible TLS interception or corporate proxy — verify the certificate chain with IT",
                )
                .with_duration_ms(ms(started))
            } else {
                CheckOutcome::error(
                    "app.home",
                    Category::Application,
                    "Backend request failed",
                    e.to_string(),
                )
                .with_duration_ms(ms(started))
            }
        }
    }
}

/// Rung 7 — a plausible clock. A wildly wrong clock makes TLS fail with
/// confusing "certificate not yet valid" errors.
fn check_clock() -> CheckOutcome {
    let now = Utc::now();
    if now.year() < 2024 {
        CheckOutcome::fail(
            "time.clock",
            Category::Time,
            format!(
                "System clock looks wrong (year {}); TLS certificate validation will fail",
                now.year()
            ),
        )
        .with_remediation("Fix the device RTC or allow outbound NTP (UDP 123)")
    } else {
        CheckOutcome::pass(
            "time.clock",
            Category::Time,
            format!("System clock is plausible ({})", now.to_rfc3339()),
        )
    }
}

/// Run a command with a hard timeout, returning stdout or a human error string.
/// Mirrors the auditor's probe idiom.
async fn run_cmd(bin: &str, args: &[&str], dur: Duration) -> Result<String, String> {
    let output = timeout(dur, Command::new(bin).args(args).output())
        .await
        .map_err(|_| format!("{bin} timed out after {dur:?}"))?
        .map_err(|e| format!("failed to run {bin}: {e}"))?;
    if !output.status.success() {
        return Err(format!("{bin} exited with status {:?}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Best-effort MAC read straight off the interface (the same `/sys` path the
/// registration flow already uses).
async fn read_mac(iface: &str) -> Option<String> {
    let path = format!("/sys/class/net/{iface}/address");
    match tokio::fs::read_to_string(&path).await {
        Ok(s) => {
            let mac = s.trim().to_string();
            if mac.is_empty() {
                None
            } else {
                Some(mac)
            }
        }
        Err(e) => {
            warn!("Could not read MAC from {path}: {e}");
            None
        }
    }
}

/// Find `key` at the start of a (trimmed) line and return the trimmed remainder,
/// e.g. the `MyNet` in `\tSSID: MyNet`.
fn parse_field(out: &str, key: &str) -> Option<String> {
    out.lines().find_map(|line| {
        line.trim()
            .strip_prefix(key)
            .map(|rest| rest.trim().to_string())
    })
}

/// Parse the signal line of `iw link`, e.g. `signal: -58 dBm` -> `-58`.
fn parse_signal(out: &str) -> Option<i32> {
    let line = out.lines().find(|l| l.trim().starts_with("signal:"))?;
    line.split_whitespace().nth(1)?.parse::<i32>().ok()
}

fn parse_host_port(server: &str) -> Option<(String, u16)> {
    let url = Url::parse(server).ok()?;
    let host = url.host_str()?.to_string();
    let port = url.port_or_known_default()?;
    Some((host, port))
}

fn ms(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_https_host_and_default_port() {
        assert_eq!(
            parse_host_port("https://api.smith.teton.ai/smith"),
            Some(("api.smith.teton.ai".to_string(), 443))
        );
        assert_eq!(
            parse_host_port("http://api:8080/smith"),
            Some(("api".to_string(), 8080))
        );
        assert_eq!(parse_host_port("not a url"), None);
    }

    #[test]
    fn parses_iw_link_fields() {
        let sample = "Connected to 12:34:56:78:9a:bc (on wlan0)\n\tSSID: Hospital-IoT\n\tfreq: 5180\n\tsignal: -58 dBm\n\ttx bitrate: 130.0 MBit/s\n";
        assert_eq!(parse_field(sample, "SSID:"), Some("Hospital-IoT".to_string()));
        assert_eq!(parse_signal(sample), Some(-58));
    }

    #[test]
    fn missing_fields_parse_to_none() {
        assert_eq!(parse_field("nothing here", "SSID:"), None);
        assert_eq!(parse_signal("no signal line"), None);
    }
}
