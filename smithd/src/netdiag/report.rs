//! Network diagnostic report: the data model, the verdict engine, and the
//! human-readable renderer.
//!
//! This module is deliberately free of I/O and of any daemon plumbing
//! (commander / postman / dbus). A sweep builds a [`DiagnosticReport`] here;
//! every trigger — startup, a backend command, the CLI subcommand, and later
//! the Bluetooth handler — produces and consumes this same serializable type.
//!
//! The guiding distinction is that a check that *couldn't run* ([`CheckStatus::Error`])
//! is never confused with a check that *ran and found the network broken*
//! ([`CheckStatus::Fail`]). Likewise a [`CheckStatus::Skip`] always records the
//! precondition that caused it, so a non-expert reading the report top-to-bottom
//! always sees *why* a rung was not evaluated.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Bumped whenever the on-disk / on-wire shape changes, so the backend can
/// parse reports produced by older daemons.
pub const SCHEMA_VERSION: u32 = 1;

/// Outcome of a single check. The split between `Fail` and `Error` is the whole
/// point: `Fail` is a finding to hand IT, `Error` is our tooling failing and
/// must never be blamed on their network.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    /// Ran and the condition is healthy.
    Pass,
    /// Ran, suboptimal but not fatal on its own.
    Warn,
    /// Ran and the condition is genuinely bad — a finding for IT.
    Fail,
    /// The check itself could not run (tool missing, timeout, permission,
    /// parse error). Our problem, not the network's.
    Error,
    /// Not run because a precondition was not met (e.g. a parent check failed).
    Skip,
}

impl CheckStatus {
    fn label(self) -> &'static str {
        match self {
            CheckStatus::Pass => "PASS",
            CheckStatus::Warn => "WARN",
            CheckStatus::Fail => "FAIL",
            CheckStatus::Error => "ERROR",
            CheckStatus::Skip => "SKIP",
        }
    }
}

/// The triage ladder rung a check belongs to. Used for grouping and for the
/// coarse fault-attribution in the verdict engine.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Radio / driver / physical layer.
    Radio,
    /// WiFi association and authentication.
    Association,
    /// Network access control (MAC allow-listing, VLAN quarantine, 802.1X).
    Nac,
    /// IP / DHCP / routing.
    Ip,
    /// DNS resolution.
    Dns,
    /// Outbound reachability / firewall / egress.
    Egress,
    /// HTTP(S) proxy requirements.
    Proxy,
    /// TLS handshake / certificate / interception.
    Tls,
    /// System clock / NTP.
    Time,
    /// Application-layer request to the backend.
    Application,
    /// Device-local state (rules out that the fault is ours, not theirs).
    Device,
}

/// What triggered a sweep.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// Once, shortly after the daemon boots.
    Startup,
    /// The `smith diagnose` CLI subcommand.
    OnDemand,
    /// A backend `RunNetworkDiagnostic` command.
    Command,
    /// The police actor detected persistent offline state.
    PoliceOffline,
    /// Requested over the Bluetooth diagnostic channel.
    BleRequest,
}

/// Which side a problem most likely sits on.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FaultSide {
    /// The customer's network / IT's responsibility.
    Network,
    /// The device itself (config, clock, local firewall, CA bundle).
    Device,
    /// Our backend (e.g. token rejected).
    Backend,
    /// Could not be attributed.
    Unknown,
}

/// How sure the verdict is.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

/// A single check's result. `finding` is always populated — including for
/// `Skip`/`Error`, where it states the "why" in plain language; `reason` carries
/// the machine-ish detail (the failed precondition or the underlying error).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CheckOutcome {
    /// Stable identifier, e.g. `"egress.tcp_443"`.
    pub id: String,
    pub category: Category,
    pub status: CheckStatus,
    /// Human one-liner, always present.
    pub finding: String,
    /// For `Skip`/`Error`: the precondition that failed or the error string.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reason: Option<String>,
    /// What to do about it, if actionable.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub remediation: Option<String>,
    /// Bounded structured detail for the dashboard.
    #[serde(skip_serializing_if = "Value::is_null", default)]
    pub data: Value,
    pub duration_ms: u64,
}

impl CheckOutcome {
    fn new(
        id: impl Into<String>,
        category: Category,
        status: CheckStatus,
        finding: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            category,
            status,
            finding: finding.into(),
            reason: None,
            remediation: None,
            data: Value::Null,
            duration_ms: 0,
        }
    }

    pub fn pass(id: impl Into<String>, category: Category, finding: impl Into<String>) -> Self {
        Self::new(id, category, CheckStatus::Pass, finding)
    }

    pub fn warn(id: impl Into<String>, category: Category, finding: impl Into<String>) -> Self {
        Self::new(id, category, CheckStatus::Warn, finding)
    }

    pub fn fail(id: impl Into<String>, category: Category, finding: impl Into<String>) -> Self {
        Self::new(id, category, CheckStatus::Fail, finding)
    }

    /// The check could not run. `reason` is the underlying error.
    pub fn error(
        id: impl Into<String>,
        category: Category,
        finding: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::new(id, category, CheckStatus::Error, finding).with_reason(reason)
    }

    /// The check was not run because a precondition was not met.
    pub fn skip(
        id: impl Into<String>,
        category: Category,
        finding: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::new(id, category, CheckStatus::Skip, finding).with_reason(reason)
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = Some(remediation.into());
        self
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }

    pub fn with_duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = duration_ms;
        self
    }
}

/// Identifying facts about the device, copied in so a report is self-contained.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DeviceInfo {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub serial: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub wifi_mac: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub release_id: Option<i32>,
}

/// One host:port the device must be able to reach, and whether it could.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AllowlistEntry {
    pub host: String,
    pub port: u16,
    pub proto: String,
    /// `None` when we couldn't even attempt the probe.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reachable: Option<bool>,
}

/// The facts IT most often needs, pulled out of `checks` so they aren't buried.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ItHandoff {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub device_mac: Option<String>,
    /// When `Some(true)`, MAC randomisation is on — which silently breaks any
    /// MAC allow-listing IT may rely on.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mac_randomization: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub egress_public_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub band: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gateway: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_allowlist: Vec<AllowlistEntry>,
}

/// The headline conclusion, derived from the checks by [`build_verdict`].
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Verdict {
    /// One-line plain-English cause.
    pub headline: String,
    pub fault_side: FaultSide,
    pub confidence: Confidence,
    /// Id of the first failing check, if any.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub first_failed: Option<String>,
    /// Ready-to-paste actions for IT.
    #[serde(default)]
    pub it_actions: Vec<String>,
}

/// A complete diagnostic sweep — the single source of truth, persisted as JSON
/// and rendered to text from this same struct.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DiagnosticReport {
    pub schema_version: u32,
    pub report_id: String,
    pub trigger: Trigger,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub device: DeviceInfo,
    pub verdict: Verdict,
    pub it_handoff: ItHandoff,
    pub checks: Vec<CheckOutcome>,
    /// Names of fields deliberately withheld (e.g. the WiFi PSK), since this
    /// report can leave the building.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redactions: Vec<String>,
}

/// Accumulates check outcomes during a sweep, then seals them into a
/// [`DiagnosticReport`] with timestamps and a computed verdict. This is the API
/// the (future) `run_sweep` builds against.
pub struct ReportBuilder {
    report_id: String,
    trigger: Trigger,
    started_at: DateTime<Utc>,
    device: DeviceInfo,
    checks: Vec<CheckOutcome>,
    handoff: ItHandoff,
    redactions: Vec<String>,
}

impl ReportBuilder {
    pub fn new(trigger: Trigger, device: DeviceInfo) -> Self {
        Self {
            report_id: Uuid::new_v4().to_string(),
            trigger,
            started_at: Utc::now(),
            device,
            checks: Vec::new(),
            handoff: ItHandoff::default(),
            redactions: Vec::new(),
        }
    }

    pub fn push(&mut self, outcome: CheckOutcome) -> &mut Self {
        self.checks.push(outcome);
        self
    }

    /// Mutable access to the IT-handoff facts so checks can fill them as they run.
    pub fn handoff_mut(&mut self) -> &mut ItHandoff {
        &mut self.handoff
    }

    pub fn redact(&mut self, what: impl Into<String>) -> &mut Self {
        self.redactions.push(what.into());
        self
    }

    pub fn finish(self) -> DiagnosticReport {
        let finished_at = Utc::now();
        // A backwards clock would make this negative; clamp rather than panic.
        let duration_ms = (finished_at - self.started_at).num_milliseconds().max(0) as u64;
        let verdict = build_verdict(&self.checks, &self.handoff);
        DiagnosticReport {
            schema_version: SCHEMA_VERSION,
            report_id: self.report_id,
            trigger: self.trigger,
            started_at: self.started_at,
            finished_at,
            duration_ms,
            device: self.device,
            verdict,
            it_handoff: self.handoff,
            checks: self.checks,
            redactions: self.redactions,
        }
    }
}

/// Coarse fault attribution from the rung a failure sits on. The check's own
/// `finding`/`remediation` carry the nuance; this only seeds `fault_side`.
fn fault_side_for(category: Category) -> FaultSide {
    match category {
        // Clock skew and local state are the device's own fault.
        Category::Device | Category::Time => FaultSide::Device,
        // An app-layer rejection (e.g. 401) is our side, not the customer's net.
        Category::Application => FaultSide::Backend,
        _ => FaultSide::Network,
    }
}

fn push_unique(actions: &mut Vec<String>, action: String) {
    if !actions.contains(&action) {
        actions.push(action);
    }
}

/// Derive the verdict by walking the checks in ladder order. The first `Fail`
/// becomes the headline; if nothing failed we are careful to distinguish
/// "healthy" from "could not actually confirm" (any `Error` present).
pub fn build_verdict(checks: &[CheckOutcome], handoff: &ItHandoff) -> Verdict {
    if let Some(first) = checks.iter().find(|c| c.status == CheckStatus::Fail) {
        let mut it_actions = Vec::new();
        for c in checks.iter().filter(|c| c.status == CheckStatus::Fail) {
            if let Some(r) = &c.remediation {
                push_unique(&mut it_actions, r.clone());
            }
        }
        for e in handoff
            .required_allowlist
            .iter()
            .filter(|e| e.reachable == Some(false))
        {
            push_unique(
                &mut it_actions,
                format!(
                    "Allow egress {}/{} to {} (port {})",
                    e.proto, e.port, e.host, e.port
                ),
            );
        }
        return Verdict {
            headline: first.finding.clone(),
            fault_side: fault_side_for(first.category),
            confidence: Confidence::High,
            first_failed: Some(first.id.clone()),
            it_actions,
        };
    }

    let warns: Vec<&CheckOutcome> = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Warn)
        .collect();
    let errors = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Error)
        .count();

    if let Some(first_warn) = warns.first() {
        let headline = if warns.len() == 1 {
            first_warn.finding.clone()
        } else {
            format!(
                "{} (and {} other warning(s))",
                first_warn.finding,
                warns.len() - 1
            )
        };
        Verdict {
            headline,
            fault_side: FaultSide::Unknown,
            confidence: Confidence::Medium,
            first_failed: None,
            it_actions: Vec::new(),
        }
    } else if errors > 0 {
        // Nothing failed, but we couldn't run everything — refuse to claim healthy.
        Verdict {
            headline: format!(
                "Diagnostics incomplete: {errors} check(s) could not run, so connectivity could not be confirmed."
            ),
            fault_side: FaultSide::Unknown,
            confidence: Confidence::Low,
            first_failed: None,
            it_actions: Vec::new(),
        }
    } else {
        Verdict {
            headline: "All checks passed — device has full connectivity to the backend."
                .to_string(),
            fault_side: FaultSide::Unknown,
            confidence: Confidence::High,
            first_failed: None,
            it_actions: Vec::new(),
        }
    }
}

/// Render a report as the plain-text artifact handed to on-site staff / IT
/// (this is what `last.txt`, the console output, and a USB dump contain).
///
/// Built with `format!`/`push_str` rather than `write!` so there is no
/// formatter `Result` to either unwrap or silently discard.
pub fn render_text(report: &DiagnosticReport) -> String {
    let mut s = String::new();

    s.push_str("SMITH NETWORK DIAGNOSTIC\n");
    s.push_str("========================\n");
    s.push_str(&format!("Report:   {}\n", report.report_id));
    s.push_str(&format!(
        "When:     {} (took {:.1}s, trigger: {:?})\n",
        report.finished_at.to_rfc3339(),
        report.duration_ms as f64 / 1000.0,
        report.trigger
    ));
    let dev = &report.device;
    s.push_str(&format!(
        "Device:   serial={} mac={} release={}\n",
        dev.serial.as_deref().unwrap_or("?"),
        dev.wifi_mac.as_deref().unwrap_or("?"),
        dev.release_id
            .map(|r| r.to_string())
            .unwrap_or_else(|| "?".to_string()),
    ));

    let v = &report.verdict;
    s.push_str("\nVERDICT\n-------\n");
    s.push_str(&format!(
        "[{:?}] {}\nConfidence: {:?}\n",
        v.fault_side, v.headline, v.confidence
    ));

    if !v.it_actions.is_empty() {
        s.push_str("\nWHAT TO ASK IT\n--------------\n");
        for action in &v.it_actions {
            s.push_str(&format!("  - {action}\n"));
        }
    }

    let h = &report.it_handoff;
    if !h.required_allowlist.is_empty() {
        s.push_str("\nRequired egress allowlist:\n");
        for e in &h.required_allowlist {
            let state = match e.reachable {
                Some(true) => "reachable",
                Some(false) => "UNREACHABLE",
                None => "untested",
            };
            s.push_str(&format!("  {}:{}/{}  {}\n", e.host, e.port, e.proto, state));
        }
    }

    s.push_str("\nDevice facts for IT:\n");
    if let Some(mac) = &h.device_mac {
        let rand = match h.mac_randomization {
            Some(true) => " (randomization: ON — breaks MAC allow-listing)",
            Some(false) => " (randomization: off)",
            None => "",
        };
        s.push_str(&format!("  MAC:        {mac}{rand}\n"));
    }
    if let Some(ip) = &h.egress_public_ip {
        s.push_str(&format!("  Egress IP:  {ip}\n"));
    }
    if h.ssid.is_some() || h.bssid.is_some() {
        s.push_str(&format!(
            "  SSID/BSSID: {} / {}{}\n",
            h.ssid.as_deref().unwrap_or("?"),
            h.bssid.as_deref().unwrap_or("?"),
            h.band
                .as_deref()
                .map(|b| format!(" ({b})"))
                .unwrap_or_default(),
        ));
    }
    if h.ip.is_some() || h.gateway.is_some() {
        s.push_str(&format!(
            "  IP/Gateway: {} / {}\n",
            h.ip.as_deref().unwrap_or("?"),
            h.gateway.as_deref().unwrap_or("?"),
        ));
    }

    let mut pass = 0;
    let mut warn = 0;
    let mut fail = 0;
    let mut error = 0;
    let mut skip = 0;
    for c in &report.checks {
        match c.status {
            CheckStatus::Pass => pass += 1,
            CheckStatus::Warn => warn += 1,
            CheckStatus::Fail => fail += 1,
            CheckStatus::Error => error += 1,
            CheckStatus::Skip => skip += 1,
        }
    }
    s.push_str(&format!(
        "\nCHECKS ({} total: {} pass, {} warn, {} fail, {} error, {} skip)\n",
        report.checks.len(),
        pass,
        warn,
        fail,
        error,
        skip
    ));
    s.push_str("--------\n");
    for c in &report.checks {
        s.push_str(&format!(
            "[{:<5}] {:<22} {}\n",
            c.status.label(),
            c.id,
            c.finding
        ));
        if let Some(reason) = &c.reason {
            s.push_str(&format!("        reason: {reason}\n"));
        }
        if let Some(rem) = &c.remediation {
            s.push_str(&format!("        \u{2192} {rem}\n"));
        }
    }

    if !report.redactions.is_empty() {
        s.push_str(&format!(
            "\nRedacted from this report: {}\n",
            report.redactions.join(", ")
        ));
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report_with(checks: Vec<CheckOutcome>) -> DiagnosticReport {
        let mut b = ReportBuilder::new(Trigger::OnDemand, DeviceInfo::default());
        for c in checks {
            b.push(c);
        }
        b.finish()
    }

    #[test]
    fn first_fail_becomes_the_headline() {
        let report = report_with(vec![
            CheckOutcome::pass("wifi.assoc", Category::Association, "Associated, -58 dBm"),
            CheckOutcome::fail("egress.tcp_443", Category::Egress, "TCP 443 timed out")
                .with_remediation("Allow egress 443"),
            CheckOutcome::fail("egress.tcp_53", Category::Egress, "TCP 53 timed out"),
        ]);
        assert_eq!(report.verdict.headline, "TCP 443 timed out");
        assert_eq!(
            report.verdict.first_failed.as_deref(),
            Some("egress.tcp_443")
        );
        assert_eq!(report.verdict.fault_side, FaultSide::Network);
        assert_eq!(report.verdict.confidence, Confidence::High);
        assert!(
            report
                .verdict
                .it_actions
                .contains(&"Allow egress 443".to_string())
        );
    }

    #[test]
    fn app_layer_failure_points_at_backend() {
        let report = report_with(vec![CheckOutcome::fail(
            "app.home",
            Category::Application,
            "Backend rejected device token (401)",
        )]);
        assert_eq!(report.verdict.fault_side, FaultSide::Backend);
    }

    #[test]
    fn errors_without_fails_do_not_claim_healthy() {
        let report = report_with(vec![
            CheckOutcome::pass("wifi.assoc", Category::Association, "ok"),
            CheckOutcome::error(
                "egress.tcp_443",
                Category::Egress,
                "could not test",
                "iw missing",
            ),
        ]);
        assert_eq!(report.verdict.confidence, Confidence::Low);
        assert!(report.verdict.headline.contains("incomplete"));
    }

    #[test]
    fn all_pass_is_healthy_and_high_confidence() {
        let report = report_with(vec![CheckOutcome::pass(
            "app.home",
            Category::Application,
            "200 OK",
        )]);
        assert_eq!(report.verdict.confidence, Confidence::High);
        assert!(report.verdict.headline.contains("full connectivity"));
    }

    #[test]
    fn skip_carries_its_reason_and_survives_serde() {
        let report = report_with(vec![CheckOutcome::skip(
            "ip.gateway_ping",
            Category::Ip,
            "Gateway reachability not evaluated",
            "no DHCP lease (ip.dhcp failed)",
        )]);
        let json = serde_json::to_string(&report).expect("serialize");
        let back: DiagnosticReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            back.checks[0].reason.as_deref(),
            Some("no DHCP lease (ip.dhcp failed)")
        );
    }

    #[test]
    fn render_text_includes_verdict_and_does_not_panic() {
        let report = report_with(vec![
            CheckOutcome::fail("egress.tcp_443", Category::Egress, "TCP 443 timed out")
                .with_remediation("Allow egress 443"),
        ]);
        let text = render_text(&report);
        assert!(text.contains("VERDICT"));
        assert!(text.contains("TCP 443 timed out"));
        assert!(text.contains("FAIL"));
    }
}
