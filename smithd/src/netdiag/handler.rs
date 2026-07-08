//! Daemon-facing glue for diagnostics.
//!
//! The sweep engine ([`super::checks`]) and report model ([`super::report`]) are
//! deliberately free of daemon plumbing. This module is the thin layer that
//! couples them to the running daemon: it builds [`SweepOptions`] from the live
//! configuration, persists results, queues offline-captured reports for upload,
//! and wraps interactive results as command responses. Keeping the coupling here
//! means the engine stays reusable from the CLI and (later) the Bluetooth
//! handler without dragging in actors.

use crate::magic::MagicHandle;
use crate::netdiag::{DeviceInfo, DiagnosticReport, SweepOptions, Trigger, run_sweep, store};
use crate::utils::schema::{SafeCommandResponse, SafeCommandRx};
use tracing::{error, info};

/// Default bore control port — the egress IT must allow for the support tunnel.
const BORE_CONTROL_PORT: u16 = 7835;

/// Assemble sweep inputs from the live daemon configuration.
pub async fn options_from_magic(magic: &MagicHandle, trigger: Trigger) -> SweepOptions {
    let server = magic.get_server().await;
    let release_id = magic.get_release_id().await.ok();
    let device = DeviceInfo {
        serial: None,
        // Left for the sweep to read off the interface.
        wifi_mac: None,
        release_id,
    };

    let mut opts = SweepOptions::new(trigger, server, device);
    let tunnel = magic.get_tunnel_details().await;
    if !tunnel.server.is_empty() {
        opts.tunnel = Some((tunnel.server, BORE_CONTROL_PORT));
    }
    opts
}

/// Run a sweep and persist it locally (`last.json` + history, for the CLI and
/// out-of-band retrieval). Does not queue for upload — the interactive callers
/// deliver the report inline via `/home`. A persistence failure is logged, never
/// fatal.
pub async fn sweep_and_persist(opts: SweepOptions) -> DiagnosticReport {
    let report = run_sweep(opts).await;
    if let Err(err) = store::persist(&report).await {
        error!("Failed to persist diagnostic report: {err:#}");
    }
    report
}

/// Wrap a finished report as a command response carrying its serialized form.
/// Used by the interactive command path, where the device is online by
/// definition so the report rides back inline on the next `/home` (like
/// `AuditReport`).
pub fn to_response(id: i32, report: &DiagnosticReport) -> SafeCommandResponse {
    let value = serde_json::to_value(report).unwrap_or_else(|err| {
        error!("Failed to serialize diagnostic report: {err}");
        serde_json::Value::Null
    });
    SafeCommandResponse {
        id,
        command: SafeCommandRx::NetworkDiagnosticReport { report: value },
        status: 0,
    }
}

/// Daemon-start trigger: sweep in the background, persist locally, and enqueue
/// for upload. The postman drains the queue into a later `/home` and deletes
/// each report once that POST is acknowledged — so a report captured while the
/// device is offline survives the outage (and a restart) and is delivered
/// exactly once. Spawned so it never blocks boot, and run regardless of
/// registration (the point is to diagnose when the device *can't* reach us).
pub fn run_on_startup(magic: MagicHandle) {
    tokio::spawn(async move {
        info!("Running network diagnostics on startup");
        let opts = options_from_magic(&magic, Trigger::Startup).await;
        let report = run_sweep(opts).await;
        if let Err(err) = store::persist(&report).await {
            error!("Failed to persist diagnostic report: {err:#}");
        }
        if let Err(err) = store::enqueue(&report).await {
            error!("Failed to enqueue diagnostic report for upload: {err:#}");
        }
    });
}
