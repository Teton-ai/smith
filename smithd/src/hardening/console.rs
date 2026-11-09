use crate::utils::files::write_file_atomic;
use anyhow::{Context, Result, bail};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tokio::process::Command;
use tokio::time::{Duration, timeout};
use tracing::{error, info};

const ACCESS_FILE: &str = "/etc/security/access-nologin.conf";
/// pam_access rule: deny every user from every origin
const ACCESS_RULE: &str = "-:ALL:ALL\n";
const PAM_LOGIN: &str = "/etc/pam.d/login";
/// same line forge writes. only /bin/login reads it, sshd and sudo have their own stacks
const PAM_LINE: &str =
    "account required pam_access.so accessfile=/etc/security/access-nologin.conf";

/// getty@ covers tty1-6, autovt@ is what logind starts on alt+fn, getty-static is
/// the tty2-6 fallback when logind is down, ttyTCU0 is the debug uart (forge masked
/// it before the rest), ttyGS0 is the usb gadget's serial port
const MASKED_UNITS: [&str; 5] = [
    "getty@.service",
    "autovt@.service",
    "getty-static.service",
    "serial-getty@ttyTCU0.service",
    "serial-getty@ttyGS0.service",
];
/// nvidia-l4t-init ships nvgetty (debug uart getty) as a real file in /etc/systemd/system,
/// so `systemctl mask` refuses it. forge overwrites it with the mask symlink, so do we
const FORCE_MASKED_UNITS: [&str; 1] = ["nvgetty.service"];
const UNIT_DIR: &str = "/etc/systemd/system";
/// masking only blocks future starts, these catch what's already running
const RUNNING_PATTERNS: [&str; 5] = [
    "getty@*",
    "autovt@*",
    "serial-getty@ttyTCU0.service",
    "serial-getty@ttyGS0.service",
    "nvgetty.service",
];

const LOGIND_DROPIN: &str = "/etc/systemd/logind.conf.d/99-no-vt.conf";
/// takes effect next boot, restarting logind isn't worth the risk
const LOGIND_BODY: &str = "[Login]\nNAutoVTs=0\nReserveVT=0\n";

/// craft needs the gadget's ncm network, not its serial port. read at gadget start (boot)
const USB_GADGET_CONFIG: &str = "/opt/nvidia/l4t-usb-device-mode/nv-l4t-usb-device-mode-config.sh";

/// this runs in the boot path, a hung systemctl must not hold up the daemon
const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) async fn close_console_logins() -> Result<()> {
    // independent steps, one failing shouldn't skip the rest.
    // access file before the pam line so login never points at a missing file
    let steps = [
        ("access rule", write_access_rule().await),
        (
            "pam login",
            patch_file(Path::new(PAM_LOGIN), with_pam_line).await,
        ),
        ("mask gettys", mask_gettys().await),
        ("stop gettys", stop_running_gettys().await),
        ("logind drop-in", write_logind_dropin().await),
        ("usb serial", disable_usb_serial().await),
    ];

    let mut failed = Vec::new();
    for (step, result) in steps {
        if let Err(err) = result {
            error!("Console hardening step '{step}' failed: {err:#}");
            failed.push(step);
        }
    }
    if !failed.is_empty() {
        bail!("failed steps: {}", failed.join(", "));
    }
    Ok(())
}

async fn write_access_rule() -> Result<()> {
    write_if_different(Path::new(ACCESS_FILE), ACCESS_RULE, 0o644).await
}

async fn mask_gettys() -> Result<()> {
    // checking directly avoids a daemon-reload every boot
    let mut forced = Vec::new();
    for unit in FORCE_MASKED_UNITS {
        if !is_masked(unit).await {
            force_mask(unit).await?;
            forced.push(unit);
        }
    }
    let mut missing = Vec::new();
    for unit in MASKED_UNITS {
        if !is_masked(unit).await {
            missing.push(unit);
        }
    }

    if !missing.is_empty() {
        // mask reloads systemd itself, which also picks up the forced ones
        let mut args = vec!["mask"];
        args.extend(&missing);
        systemctl(&args).await?;
    } else if !forced.is_empty() {
        systemctl(&["daemon-reload"]).await?;
    }

    if !forced.is_empty() || !missing.is_empty() {
        info!("Masked {}", [forced, missing].concat().join(", "));
    }
    Ok(())
}

/// a mask is a symlink to /dev/null in the unit dir
async fn is_masked(unit: &str) -> bool {
    matches!(
        tokio::fs::read_link(Path::new(UNIT_DIR).join(unit)).await,
        Ok(target) if target == Path::new("/dev/null")
    )
}

/// same as forge's `ln -sf /dev/null`, but through a rename so the unit file is
/// never half replaced
async fn force_mask(unit: &str) -> Result<()> {
    let path = Path::new(UNIT_DIR).join(unit);
    let tmp = Path::new(UNIT_DIR).join(format!(".{unit}.smith-tmp"));
    match tokio::fs::remove_file(&tmp).await {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err).with_context(|| format!("removing {}", tmp.display())),
    }
    tokio::fs::symlink("/dev/null", &tmp)
        .await
        .with_context(|| format!("creating {}", tmp.display()))?;
    tokio::fs::rename(&tmp, &path)
        .await
        .with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

async fn stop_running_gettys() -> Result<()> {
    let mut args = vec!["list-units", "--state=active", "--plain", "--no-legend"];
    args.extend(RUNNING_PATTERNS);
    let listing = systemctl(&args).await?;
    let running: Vec<&str> = listing
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    if running.is_empty() {
        return Ok(());
    }

    let mut args = vec!["stop"];
    args.extend(&running);
    systemctl(&args).await?;
    info!("Stopped {}", running.join(", "));
    Ok(())
}

async fn write_logind_dropin() -> Result<()> {
    let path = Path::new(LOGIND_DROPIN);
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir)
            .await
            .with_context(|| format!("creating {}", dir.display()))?;
    }
    write_if_different(path, LOGIND_BODY, 0o644).await
}

async fn disable_usb_serial() -> Result<()> {
    let path = Path::new(USB_GADGET_CONFIG);
    if !tokio::fs::try_exists(path)
        .await
        .with_context(|| format!("checking {}", path.display()))?
    {
        info!("No USB gadget config, no serial port to disable");
        return Ok(());
    }

    patch_file(path, with_acm_off).await?;

    // same check as forge's grep -qx, so a sed that didn't take is a failure
    let script = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    if !acm_off(&script) {
        bail!("enable_acm=0 not set in {}", path.display());
    }
    Ok(())
}

/// writes `contents` unless the file already has them. keeps the existing mode,
/// `new_mode` is only for a file that doesn't exist yet
async fn write_if_different(path: &Path, contents: &str, new_mode: u32) -> Result<()> {
    if let Ok(existing) = tokio::fs::read_to_string(path).await
        && existing == contents
    {
        return Ok(());
    }

    let mode = match tokio::fs::metadata(path).await {
        Ok(meta) => meta.permissions().mode() & 0o7777,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => new_mode,
        Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
    };
    write_file_atomic(path, contents, mode).await?;
    info!("Wrote {}", path.display());
    Ok(())
}

/// rewrites an existing file through `patch`, which returns `None` when there's
/// nothing to change. keeps the mode (the gadget config is an executable script)
async fn patch_file(path: &Path, patch: fn(&str) -> Option<String>) -> Result<()> {
    let current = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    let Some(patched) = patch(&current) else {
        return Ok(());
    };

    let mode = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?
        .permissions()
        .mode()
        & 0o7777;
    write_file_atomic(path, &patched, mode).await?;
    info!("Patched {}", path.display());
    Ok(())
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

/// matches forge's `grep -qx enable_acm=0`
fn acm_off(script: &str) -> bool {
    script.lines().any(|line| line == "enable_acm=0")
}

/// same edit as forge's `sed 's/^enable_acm=1/enable_acm=0/'`, `None` if no line needs it
fn with_acm_off(script: &str) -> Option<String> {
    if !script.lines().any(|line| line.starts_with("enable_acm=1")) {
        return None;
    }
    let mut out = String::with_capacity(script.len());
    for line in script.lines() {
        match line.strip_prefix("enable_acm=1") {
            Some(rest) => {
                out.push_str("enable_acm=0");
                out.push_str(rest);
            }
            None => out.push_str(line),
        }
        out.push('\n');
    }
    if !script.ends_with('\n') {
        out.pop();
    }
    Some(out)
}

/// true when an active (uncommented) line matches ours, ignoring whitespace.
/// a commented-out copy doesn't count, unlike forge's grep
fn has_pam_line(pam: &str) -> bool {
    let wanted: Vec<&str> = PAM_LINE.split_whitespace().collect();
    pam.lines().any(|line| {
        let line = line.trim_start();
        !line.starts_with('#') && line.split_whitespace().eq(wanted.iter().copied())
    })
}

/// `pam` with our line appended, or `None` if it's already there
fn with_pam_line(pam: &str) -> Option<String> {
    if has_pam_line(pam) {
        return None;
    }
    let mut out = String::with_capacity(pam.len() + PAM_LINE.len() + 2);
    out.push_str(pam);
    if !pam.is_empty() && !pam.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(PAM_LINE);
    out.push('\n');
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // trimmed version of the stock l4t /etc/pam.d/login
    const STOCK: &str = "\
auth       optional   pam_faildelay.so  delay=3000000
# account  required       pam_access.so
@include common-auth
@include common-account
session    required   pam_limits.so
";

    #[test]
    fn appends_to_stock() {
        let Some(out) = with_pam_line(STOCK) else {
            panic!("expected the line to be added");
        };
        assert!(out.starts_with(STOCK));
        assert!(out.ends_with(&format!("{PAM_LINE}\n")));
        assert!(has_pam_line(&out));
    }

    #[test]
    fn leaves_it_alone_when_present() {
        let pam = format!("{STOCK}{PAM_LINE}\n");
        assert_eq!(with_pam_line(&pam), None);
    }

    #[test]
    fn whitespace_differences_still_count() {
        let pam = format!(
            "{STOCK}  account\trequired   pam_access.so accessfile=/etc/security/access-nologin.conf  \n"
        );
        assert!(has_pam_line(&pam));
    }

    #[test]
    fn commented_out_does_not_count() {
        let pam = format!("{STOCK}# {PAM_LINE}\n");
        assert!(!has_pam_line(&pam));
        assert!(with_pam_line(&pam).is_some());
    }

    #[test]
    fn stock_pam_access_comment_does_not_count() {
        assert!(!has_pam_line(STOCK));
    }

    #[test]
    fn other_accessfile_does_not_count() {
        let pam =
            format!("{STOCK}account required pam_access.so accessfile=/etc/security/access.conf\n");
        assert!(!has_pam_line(&pam));
    }

    #[test]
    fn adds_missing_trailing_newline() {
        let Some(out) = with_pam_line("@include common-account") else {
            panic!("expected the line to be added");
        };
        assert_eq!(out, format!("@include common-account\n{PAM_LINE}\n"));
    }

    #[test]
    fn writes_into_empty_file() {
        assert_eq!(with_pam_line(""), Some(format!("{PAM_LINE}\n")));
    }

    const GADGET: &str = "\
#!/bin/bash
enable_rndis=1
enable_acm=1
enable_ncm=1
";

    #[test]
    fn turns_acm_off_and_keeps_the_rest() {
        assert_eq!(
            with_acm_off(GADGET).as_deref(),
            Some("#!/bin/bash\nenable_rndis=1\nenable_acm=0\nenable_ncm=1\n")
        );
    }

    #[test]
    fn acm_already_off_is_left_alone() {
        let script = GADGET.replace("enable_acm=1", "enable_acm=0");
        assert_eq!(with_acm_off(&script), None);
        assert!(acm_off(&script));
    }

    #[test]
    fn acm_on_is_not_off() {
        assert!(!acm_off(GADGET));
    }

    #[test]
    fn indented_acm_is_left_alone_like_sed() {
        let script = "  enable_acm=1\n";
        assert_eq!(with_acm_off(script), None);
        assert!(!acm_off(script));
    }

    #[test]
    fn acm_keeps_missing_trailing_newline() {
        assert_eq!(
            with_acm_off("enable_acm=1").as_deref(),
            Some("enable_acm=0")
        );
    }
}
