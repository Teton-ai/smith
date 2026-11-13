//! reboot watchdog, so a hung shutdown still resets the device

use super::{systemctl, write_if_different};
use anyhow::{Context, Result, bail};
use std::path::Path;
use tracing::info;

const DROPIN: &str = "/etc/systemd/system.conf.d/reboot-watchdog.conf";
/// identical to forge's file so forge builds aren't rewritten
const BODY: &str = "\
# hardware watchdog during the last phase of a reboot, so a hung shutdown
# still resets the device.
#
# the tegra186 wdt driver caps timeouts at 255s; systemd's 10min default is
# rejected by the driver, so stay under the cap.

[Manager]
RebootWatchdogSec=4min
";
const EXPECTED: &str = "4min";

pub(super) async fn set_reboot_watchdog() -> Result<()> {
    let path = Path::new(DROPIN);
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir)
            .await
            .with_context(|| format!("creating {}", dir.display()))?;
    }

    if write_if_different(path, BODY, 0o644).await? {
        systemctl(&["daemon-reload"]).await?;
    }

    // trust systemd's live value over the file
    let live = live_value().await?;
    if live != EXPECTED {
        bail!("systemd reports RebootWatchdogUSec={live}, wanted {EXPECTED}");
    }
    info!("Reboot watchdog set to {live}");
    Ok(())
}

pub(super) async fn is_set() -> Result<bool> {
    let live = live_value().await?;
    if live != EXPECTED {
        info!("Reboot watchdog is {live}, wanted {EXPECTED}");
    }
    Ok(live == EXPECTED)
}

async fn live_value() -> Result<String> {
    let live = systemctl(&["show", "-p", "RebootWatchdogUSec", "--value"]).await?;
    Ok(live.trim().to_owned())
}
