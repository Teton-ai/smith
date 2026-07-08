//! Network diagnostics: a standalone sweep that probes why a device can't reach
//! the backend, produces a self-contained [`report::DiagnosticReport`], and
//! persists it for upload-on-reconnect and out-of-band retrieval.
//!
//! The module is intentionally decoupled from the daemon's actors so the same
//! engine can be driven from the boot sequence, a backend command, the
//! `smith diagnose` CLI subcommand, and (later) the Bluetooth handler. The
//! report model lives in [`report`]; on-disk persistence lives in [`store`].
//! The probes themselves and the `run_sweep` entry point are added on top of
//! this foundation.

pub mod checks;
pub mod handler;
pub mod report;
pub mod store;

pub use checks::{SweepOptions, run_sweep};
pub use report::{
    Category, CheckOutcome, CheckStatus, DeviceInfo, DiagnosticReport, FaultSide, ItHandoff,
    ReportBuilder, Trigger, Verdict, build_verdict, render_text,
};
