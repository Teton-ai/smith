//! On-disk persistence for diagnostic reports.
//!
//! Layout under [`base_dir`] (default `/var/lib/smith/diagnostics`):
//!
//! ```text
//! last.json            newest report (machine-readable, atomic)
//! last.txt             newest report rendered for humans (atomic)
//! history/             rolling, gzip'd, capped at MAX_HISTORY
//!   <stamp>-<id>.json.gz
//! queue/               reports awaiting upload, capped at MAX_QUEUE
//!   <stamp>-<id>.json
//! ```
//!
//! Every file is written atomically (temp file in the same dir, fsync, rename)
//! under an exclusive directory lock, so the daemon and a concurrent `smith
//! diagnose` CLI run can never observe or produce a half-written `last.json`.
//! The directory and files are hardened to owner-only because a report carries
//! the site's network topology.

use crate::netdiag::report::{DiagnosticReport, render_text};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use flate2::Compression;
use flate2::write::GzEncoder;
use fs2::FileExt;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

const DEFAULT_DIR: &str = "/var/lib/smith/diagnostics";
/// Keep the newest N reports in `history/`.
const MAX_HISTORY: usize = 20;
/// Cap the upload queue so a device that never reconnects can't fill the disk.
const MAX_QUEUE: usize = 50;

/// Root of the diagnostics tree. Overridable via `SMITH_DIAG_DIR` (used by tests
/// and dev runs that aren't writing to `/var/lib`).
pub fn base_dir() -> PathBuf {
    match std::env::var_os("SMITH_DIAG_DIR") {
        Some(v) => PathBuf::from(v),
        None => PathBuf::from(DEFAULT_DIR),
    }
}

/// Path to the latest machine-readable report (consumed later by the BLE handler).
pub fn last_json_path() -> PathBuf {
    base_dir().join("last.json")
}

/// Path to the latest human-readable report (consumed by the CLI / USB dump).
pub fn last_text_path() -> PathBuf {
    base_dir().join("last.txt")
}

/// Persist a report: refresh `last.json` / `last.txt`, append to `history/`, and
/// enqueue a copy for upload-on-reconnect. Runs the blocking filesystem work on
/// a blocking thread so it doesn't stall the async runtime.
pub async fn persist(report: &DiagnosticReport) -> Result<()> {
    let report = report.clone();
    let base = base_dir();
    tokio::task::spawn_blocking(move || persist_blocking(&base, &report))
        .await
        .context("diagnostics persist task panicked")?
}

/// List queued reports awaiting upload, oldest first. The postman drains these
/// after a successful `/home` (wired up in a later step).
pub async fn pending_uploads() -> Result<Vec<PathBuf>> {
    let base = base_dir();
    tokio::task::spawn_blocking(move || list_sorted(&base.join("queue"), ".json"))
        .await
        .context("diagnostics queue scan task panicked")?
}

/// Remove a queued report once it has been uploaded.
pub async fn clear_uploaded(path: PathBuf) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))
    })
    .await
    .context("diagnostics queue cleanup task panicked")?
}

fn persist_blocking(base: &Path, report: &DiagnosticReport) -> Result<()> {
    fs::create_dir_all(base).with_context(|| format!("create {}", base.display()))?;
    harden_dir(base);

    let json = serde_json::to_vec_pretty(report).context("serialize report to json")?;
    let text = render_text(report);
    let stamp = stamp(&report.finished_at);

    // Serialise all writers (daemon + CLI) so nobody clobbers last.json.
    let _lock = LockGuard::acquire(base)?;

    write_atomic(base, "last.json", &json)?;
    write_atomic(base, "last.txt", text.as_bytes())?;
    write_history(base, &stamp, &report.report_id, &json)?;

    info!(report_id = %report.report_id, "Persisted network diagnostic report");
    Ok(())
}

/// Queue a report for upload-on-reconnect. Kept separate from [`persist`] so the
/// local copy (for the CLI / Bluetooth) and the upload backlog are independent —
/// only autonomously-captured reports (e.g. on boot, while offline) are queued.
pub async fn enqueue(report: &DiagnosticReport) -> Result<()> {
    let report = report.clone();
    let base = base_dir();
    tokio::task::spawn_blocking(move || enqueue_blocking(&base, &report))
        .await
        .context("diagnostics enqueue task panicked")?
}

fn enqueue_blocking(base: &Path, report: &DiagnosticReport) -> Result<()> {
    fs::create_dir_all(base).with_context(|| format!("create {}", base.display()))?;
    let json = serde_json::to_vec_pretty(report).context("serialize report to json")?;
    let stamp = stamp(&report.finished_at);
    let _lock = LockGuard::acquire(base)?;
    write_queue_entry(base, &stamp, &report.report_id, &json)
}

fn write_history(base: &Path, stamp: &str, report_id: &str, json: &[u8]) -> Result<()> {
    let dir = base.join("history");
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    harden_dir(&dir);

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(json).context("gzip report")?;
    let gz = encoder.finish().context("finish gzip stream")?;

    write_atomic(&dir, &format!("{stamp}-{report_id}.json.gz"), &gz)?;
    prune(&dir, ".json.gz", MAX_HISTORY);
    Ok(())
}

fn write_queue_entry(base: &Path, stamp: &str, report_id: &str, json: &[u8]) -> Result<()> {
    let dir = base.join("queue");
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    harden_dir(&dir);

    write_atomic(&dir, &format!("{stamp}-{report_id}.json"), json)?;
    prune(&dir, ".json", MAX_QUEUE);
    Ok(())
}

/// Write `bytes` to `dir/name` atomically: a temp file in the same directory is
/// filled, fsync'd, then renamed over the target.
fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let mut tmp = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("create temp file in {}", dir.display()))?;
    tmp.write_all(bytes).context("write temp file")?;
    tmp.as_file().sync_all().context("fsync temp file")?;

    let target = dir.join(name);
    tmp.persist(&target)
        .map_err(|e| e.error)
        .with_context(|| format!("persist {}", target.display()))?;
    harden_file(&target);
    Ok(())
}

/// Files in `dir` ending with `suffix`, sorted lexically (which is chronological
/// thanks to the timestamp prefix), oldest first.
fn list_sorted(dir: &Path, suffix: &str) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("read dir {}", dir.display()))?
        .filter_map(|e| match e {
            Ok(e) => Some(e.path()),
            Err(err) => {
                warn!("Skipping unreadable diagnostics entry: {err}");
                None
            }
        })
        .filter(|p| p.to_string_lossy().ends_with(suffix))
        .collect();
    entries.sort();
    Ok(entries)
}

/// Drop the oldest entries beyond `keep`. Best-effort: a failure to remove one
/// file is logged, not fatal.
fn prune(dir: &Path, suffix: &str, keep: usize) {
    let entries = match list_sorted(dir, suffix) {
        Ok(e) => e,
        Err(err) => {
            warn!("Could not scan {} for pruning: {err}", dir.display());
            return;
        }
    };
    if entries.len() <= keep {
        return;
    }
    let drop = entries.len() - keep;
    for path in entries.into_iter().take(drop) {
        if let Err(err) = fs::remove_file(&path) {
            warn!("Failed to prune {}: {err}", path.display());
        }
    }
}

const DIR_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;

fn harden_dir(path: &Path) {
    if let Err(err) = fs::set_permissions(path, fs::Permissions::from_mode(DIR_MODE)) {
        warn!("Failed to set permissions on {}: {err}", path.display());
    }
}

fn harden_file(path: &Path) {
    if let Err(err) = fs::set_permissions(path, fs::Permissions::from_mode(FILE_MODE)) {
        warn!("Failed to set permissions on {}: {err}", path.display());
    }
}

fn stamp(dt: &DateTime<Utc>) -> String {
    dt.format("%Y%m%dT%H%M%SZ").to_string()
}

/// Holds an exclusive advisory lock on `<base>/.lock` for the duration of a write.
struct LockGuard(fs::File);

impl LockGuard {
    fn acquire(base: &Path) -> Result<Self> {
        let path = base.join(".lock");
        let file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("open lock file {}", path.display()))?;
        file.lock_exclusive()
            .context("acquire diagnostics directory lock")?;
        Ok(Self(file))
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        if let Err(err) = self.0.unlock() {
            warn!("Failed to release diagnostics lock: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netdiag::report::{Category, CheckOutcome, DeviceInfo, ReportBuilder, Trigger};

    fn sample(id_finding: &str) -> DiagnosticReport {
        let mut b = ReportBuilder::new(Trigger::OnDemand, DeviceInfo::default());
        b.push(CheckOutcome::fail(
            "egress.tcp_443",
            Category::Egress,
            id_finding,
        ));
        b.finish()
    }

    #[test]
    fn persist_writes_last_and_history_but_not_queue() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path();

        let report = sample("TCP 443 timed out");
        persist_blocking(base, &report).expect("persist");

        let last = fs::read_to_string(base.join("last.json")).expect("last.json");
        assert!(last.contains(&report.report_id));
        assert!(
            fs::read_to_string(base.join("last.txt"))
                .expect("last.txt")
                .contains("TCP 443 timed out")
        );

        let history = list_sorted(&base.join("history"), ".json.gz").expect("history");
        assert_eq!(history.len(), 1);
        // The queue is a separate concern; persist must not touch it.
        assert!(list_sorted(&base.join("queue"), ".json")
            .expect("queue")
            .is_empty());
    }

    #[test]
    fn enqueue_writes_only_the_queue() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path();

        let report = sample("TCP 443 timed out");
        enqueue_blocking(base, &report).expect("enqueue");

        let queue = list_sorted(&base.join("queue"), ".json").expect("queue");
        assert_eq!(queue.len(), 1);
        assert!(!base.join("last.json").exists());
    }

    #[test]
    fn last_json_is_overwritten_not_appended() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path();

        persist_blocking(base, &sample("first")).expect("persist 1");
        let second = sample("second");
        persist_blocking(base, &second).expect("persist 2");

        let last: DiagnosticReport =
            serde_json::from_str(&fs::read_to_string(base.join("last.json")).expect("read"))
                .expect("parse");
        assert_eq!(last.report_id, second.report_id);
        // Two distinct reports => two history entries, one live `last.json`.
        assert_eq!(
            list_sorted(&base.join("history"), ".json.gz")
                .expect("history")
                .len(),
            2
        );
    }
}
