//! Security and reliability baseline smithd enforces on every boot. Not a
//! setting: nothing remote can turn these off. Each rule is idempotent and
//! a failure only logs, so a broken rule never stops the daemon from starting.

mod console;
mod ssh;
mod watchdog;

use crate::utils::files::write_file_atomic;
use anyhow::{Context, Result, bail};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tokio::process::Command;
use tokio::time::{Duration, timeout};
use tracing::{error, info};

/// this runs in the boot path, a hung systemctl must not hold up the daemon
const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn run() {
    // self-heals if an os update or someone flips it back
    if let Err(err) = ssh::disable_ssh_password_auth().await {
        error!("Failed to disable SSH password auth: {err:#}");
    }

    if !crate::utils::system::is_v2() {
        info!("Not v2 hardware, leaving console logins open");
    } else if let Err(err) = console::close_console_logins().await {
        error!("Failed to close console logins: {err:#}");
    }

    if let Err(err) = watchdog::set_reboot_watchdog().await {
        error!("Failed to set the reboot watchdog: {err:#}");
    }
}

/// audit probe, `None` when the check itself couldn't run
pub async fn console_logins_closed() -> Option<bool> {
    console::is_closed()
        .await
        .inspect_err(|err| error!("Failed to check console logins: {err:#}"))
        .ok()
}

/// audit probe, `None` when the check itself couldn't run
pub async fn reboot_watchdog_set() -> Option<bool> {
    watchdog::is_set()
        .await
        .inspect_err(|err| error!("Failed to check the reboot watchdog: {err:#}"))
        .ok()
}

/// writes `contents` unless the file already has them, returns whether it wrote.
/// keeps the existing mode, `new_mode` is only for a file that doesn't exist yet
async fn write_if_different(path: &Path, contents: &str, new_mode: u32) -> Result<bool> {
    if let Ok(existing) = tokio::fs::read_to_string(path).await
        && existing == contents
    {
        return Ok(false);
    }

    let mode = match tokio::fs::metadata(path).await {
        Ok(meta) => meta.permissions().mode() & 0o7777,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => new_mode,
        Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
    };
    write_file_atomic(path, contents, mode).await?;
    info!("Wrote {}", path.display());
    Ok(true)
}

async fn systemctl(args: &[&str]) -> Result<String> {
    let mut command = Command::new("systemctl");
    command.args(args).kill_on_drop(true);
    let output = timeout(SYSTEMCTL_TIMEOUT, command.output())
        .await
        .with_context(|| format!("systemctl {} timed out", args.join(" ")))?
        .context("running systemctl")?;
    if !output.status.success() {
        bail!(
            "systemctl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
