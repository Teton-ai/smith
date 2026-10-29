//! Carries out an [`ApplyPlan`](super::plan::ApplyPlan): writes and removes
//! the files smithd owns and runs the system commands, each step on its own
//! so the actor decides their order.
// Unused until the settings actor applies settings.
#![allow(dead_code)]

use super::driver::{DriverPaths, VENDOR_MODULE};
use super::ntp::{CHRONY_CONF, CHRONY_SOURCES_DIR, timesyncd_running};
use super::plan::{FileChange, NtpDaemon, OwnedFiles};
use super::read::{run, run_for};
use crate::utils::files::write_file_atomic;
use crate::utils::schema::RoamTuning;
use anyhow::{Context, Result, anyhow, bail};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::warn;
use zbus::Connection;

const DRIVER_CONF: &str = "/etc/modprobe.d/zz-smith-rtl8822ce.conf";
const NM_CONF: &str = "/etc/NetworkManager/conf.d/zz-smith.conf";
const CHRONY_SOURCES: &str = "/etc/chrony/sources.d/smith.sources";
const TIMESYNCD_CONF: &str = "/etc/systemd/timesyncd.conf.d/zz-smith.conf";
const HOSTS: &str = "/etc/hosts";
/// Debian's address for the machine's own name.
const HOSTS_SELF_ADDRESS: &str = "127.0.1.1";

/// `rmmod` waits for the interface to go down and `modprobe` loads firmware.
const DRIVER_TIMEOUT: Duration = Duration::from_secs(30);
const MODPROBE_TRIES: u32 = 3;
const MODPROBE_RETRY_DELAY: Duration = Duration::from_secs(2);
/// The interface's proc dir comes back a few seconds after `modprobe`; the
/// roam values can only be written once it is there.
const VENDOR_BACK_TIMEOUT: Duration = Duration::from_secs(30);
const VENDOR_POLL: Duration = Duration::from_millis(500);
/// How long `nmcli` waits for the activation, and the cap on the process.
const NM_ACTIVATION_WAIT_S: &str = "30";
const NM_ACTIVATION_TIMEOUT: Duration = Duration::from_secs(40);

/// The files smithd owns. Overridable so tests can point at a temp dir.
pub struct OwnedPaths {
    pub driver: PathBuf,
    pub nm: PathBuf,
    pub chrony: PathBuf,
    pub timesyncd: PathBuf,
}

impl Default for OwnedPaths {
    fn default() -> Self {
        Self {
            driver: PathBuf::from(DRIVER_CONF),
            nm: PathBuf::from(NM_CONF),
            chrony: PathBuf::from(CHRONY_SOURCES),
            timesyncd: PathBuf::from(TIMESYNCD_CONF),
        }
    }
}

impl OwnedPaths {
    pub fn ntp(&self, daemon: NtpDaemon) -> &Path {
        match daemon {
            NtpDaemon::Chrony => &self.chrony,
            NtpDaemon::Timesyncd => &self.timesyncd,
        }
    }

    pub async fn read(&self, daemon: Option<NtpDaemon>) -> Result<OwnedFiles> {
        Ok(OwnedFiles {
            driver: read_optional(&self.driver).await?,
            nm: read_optional(&self.nm).await?,
            ntp: match daemon {
                Some(daemon) => read_optional(self.ntp(daemon)).await?,
                None => None,
            },
        })
    }
}

async fn read_optional(path: &Path) -> Result<Option<String>> {
    match tokio::fs::read_to_string(path).await {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("reading {}", path.display())),
    }
}

pub async fn change_file(path: &Path, change: &FileChange) -> Result<()> {
    match change {
        FileChange::Keep => Ok(()),
        FileChange::Write(text) => {
            if let Some(dir) = path.parent() {
                tokio::fs::create_dir_all(dir)
                    .await
                    .with_context(|| format!("creating {}", dir.display()))?;
            }
            write_file_atomic(path, text, 0o644).await
        }
        FileChange::Remove => match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err).with_context(|| format!("removing {}", path.display())),
        },
    }
}

/// `rmmod` then `modprobe`: `modprobe -r` would also unload the modules the
/// driver depends on.
fn driver_reload_steps() -> [(&'static str, [&'static str; 1]); 2] {
    [("rmmod", [VENDOR_MODULE]), ("modprobe", [VENDOR_MODULE])]
}

/// Reloads the vendor driver so it reads its modprobe.d options, then waits
/// for the interface to come back. `modprobe` is retried: the device has no
/// Wi-Fi until it succeeds.
pub async fn reload_driver(paths: &DriverPaths, interface: &str) -> Result<()> {
    let [(rmmod, rmmod_args), (modprobe, modprobe_args)] = driver_reload_steps();
    run_for(DRIVER_TIMEOUT, rmmod, &rmmod_args).await?;
    let mut tries = 0;
    loop {
        tries += 1;
        match run_for(DRIVER_TIMEOUT, modprobe, &modprobe_args).await {
            Ok(_) => break,
            Err(err) if tries < MODPROBE_TRIES => {
                warn!("settings: modprobe {VENDOR_MODULE} failed, retrying: {err:#}");
                tokio::time::sleep(MODPROBE_RETRY_DELAY).await;
            }
            Err(err) => return Err(err),
        }
    }
    let proc_dir = paths.proc.join(interface);
    let deadline = tokio::time::Instant::now() + VENDOR_BACK_TIMEOUT;
    while !tokio::fs::try_exists(&proc_dir).await.unwrap_or(false) {
        if tokio::time::Instant::now() >= deadline {
            bail!("{} did not come back after the reload", proc_dir.display());
        }
        tokio::time::sleep(VENDOR_POLL).await;
    }
    Ok(())
}

/// Values in the driver's `roam_param` order; drivers without the busy fields
/// (v5.9) take four.
pub fn roam_param_line(tuning: &RoamTuning, busy_supported: bool) -> Result<String> {
    if !busy_supported {
        return Ok(format!(
            "{} {} {} {}",
            tuning.idle_diff, tuning.scanr_exp_ms, tuning.scan_int, tuning.idle_th
        ));
    }
    let (Some(busy_diff), Some(busy_th)) = (tuning.busy_diff, tuning.busy_th) else {
        bail!("this driver needs the busy roam values");
    };
    Ok(format!(
        "{} {} {} {} {} {}",
        tuning.idle_diff, busy_diff, tuning.scanr_exp_ms, tuning.scan_int, tuning.idle_th, busy_th
    ))
}

pub fn roam_flags_line(flags: i16) -> String {
    format!("0x{flags:02x}")
}

async fn write_proc(paths: &DriverPaths, interface: &str, file: &str, line: &str) -> Result<()> {
    let path = paths.proc.join(interface).join(file);
    tokio::fs::write(&path, line)
        .await
        .with_context(|| format!("writing {}", path.display()))
}

pub async fn write_roam_tuning(
    paths: &DriverPaths,
    interface: &str,
    tuning: &RoamTuning,
    busy_supported: bool,
) -> Result<()> {
    let line = roam_param_line(tuning, busy_supported)?;
    write_proc(paths, interface, "roam_param", &line).await
}

pub async fn write_roam_flags(paths: &DriverPaths, interface: &str, flags: i16) -> Result<()> {
    write_proc(paths, interface, "roam_flags", &roam_flags_line(flags)).await
}

/// Re-reads the config files; connections keep running.
pub async fn reload_nm() -> Result<()> {
    run("systemctl", &["reload", "NetworkManager"]).await?;
    Ok(())
}

/// UUID of the active connection on `interface`, from
/// `nmcli -t -f UUID,DEVICE connection show --active` (UUIDs and interface
/// names hold no `:`, so the terse output needs no unescaping).
fn active_uuid(nmcli: &str, interface: &str) -> Option<String> {
    nmcli.lines().find_map(|line| {
        let (uuid, device) = line.split_once(':')?;
        (device == interface).then(|| uuid.to_owned())
    })
}

fn reactivate_args(uuid: &str) -> [&str; 6] {
    [
        "--wait",
        NM_ACTIVATION_WAIT_S,
        "connection",
        "up",
        "uuid",
        uuid,
    ]
}

/// Brings the active Wi-Fi connection up again, so values NM applies only on
/// activation (power save, cloned MAC) take effect.
pub async fn reactivate_wifi(interface: &str) -> Result<()> {
    let active = run(
        "nmcli",
        &["-t", "-f", "UUID,DEVICE", "connection", "show", "--active"],
    )
    .await?;
    let uuid = active_uuid(&active, interface)
        .ok_or_else(|| anyhow!("no active connection on {interface}"))?;
    run_for(NM_ACTIVATION_TIMEOUT, "nmcli", &reactivate_args(&uuid)).await?;
    Ok(())
}

/// timesyncd when it runs, else chrony when its config reads the
/// `sources.d` dir our file goes in.
fn ntp_daemon(timesyncd_running: bool, chrony_conf: Option<&str>) -> Result<Option<NtpDaemon>> {
    if timesyncd_running {
        return Ok(Some(NtpDaemon::Timesyncd));
    }
    let Some(conf) = chrony_conf else {
        return Ok(None);
    };
    let reads_sources = conf.lines().any(|line| {
        let mut words = line.split_whitespace();
        words.next() == Some("sourcedir") && words.next() == Some(CHRONY_SOURCES_DIR)
    });
    if !reads_sources {
        bail!("{CHRONY_CONF} has no sourcedir {CHRONY_SOURCES_DIR}");
    }
    Ok(Some(NtpDaemon::Chrony))
}

pub async fn detect_ntp_daemon(conn: &Connection) -> Result<Option<NtpDaemon>> {
    let timesyncd = timesyncd_running(conn).await?;
    let chrony_conf = read_optional(Path::new(CHRONY_CONF)).await?;
    ntp_daemon(timesyncd, chrony_conf.as_deref())
}

/// chrony re-reads its source files live; timesyncd only reads its config at
/// start.
fn ntp_reload_command(daemon: NtpDaemon) -> (&'static str, &'static [&'static str]) {
    match daemon {
        NtpDaemon::Chrony => ("chronyc", &["reload", "sources"]),
        NtpDaemon::Timesyncd => ("systemctl", &["restart", "systemd-timesyncd"]),
    }
}

pub async fn reload_ntp(daemon: NtpDaemon) -> Result<()> {
    let (program, args) = ntp_reload_command(daemon);
    run(program, args).await?;
    Ok(())
}

/// `/etc/hosts` with the `127.0.1.1` line naming `hostname`, added when there
/// is none, so the new name still resolves locally.
fn hosts_with_hostname(hosts: &str, hostname: &str) -> String {
    let own_line = format!("{HOSTS_SELF_ADDRESS}\t{hostname}");
    let mut found = false;
    let mut text = String::with_capacity(hosts.len() + own_line.len() + 1);
    for line in hosts.lines() {
        if line.split_whitespace().next() == Some(HOSTS_SELF_ADDRESS) {
            if !found {
                text.push_str(&own_line);
                text.push('\n');
            }
            found = true;
        } else {
            text.push_str(line);
            text.push('\n');
        }
    }
    if !found {
        text.push_str(&own_line);
        text.push('\n');
    }
    text
}

pub async fn set_hostname(hostname: &str) -> Result<()> {
    run("hostnamectl", &["set-hostname", hostname]).await?;
    let hosts = read_optional(Path::new(HOSTS)).await?.unwrap_or_default();
    let updated = hosts_with_hostname(&hosts, hostname);
    if updated != hosts {
        write_file_atomic(Path::new(HOSTS), &updated, 0o644).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> RoamTuning {
        RoamTuning {
            idle_diff: 15,
            busy_diff: Some(15),
            scanr_exp_ms: 10000,
            scan_int: 5,
            idle_th: 30,
            busy_th: Some(30),
        }
    }

    #[test]
    fn driver_reload_is_rmmod_then_modprobe_never_modprobe_r() {
        assert_eq!(
            driver_reload_steps(),
            [("rmmod", ["rtl8822ce"]), ("modprobe", ["rtl8822ce"])]
        );
    }

    #[test]
    fn roam_param_takes_four_values_on_drivers_without_busy_fields() {
        assert_eq!(roam_param_line(&tuning(), false).unwrap(), "15 10000 5 30");
    }

    #[test]
    fn roam_param_refuses_a_tuning_without_busy_values_on_drivers_that_need_them() {
        let no_busy = RoamTuning {
            busy_diff: None,
            busy_th: None,
            ..tuning()
        };
        assert!(roam_param_line(&no_busy, true).is_err());
    }

    const ACTIVE: &str = "5f3c1e2a-0d4b-4a1e-9c3f-2b7d8e9f0a1b:eth0\n\
                          8a1b2c3d-4e5f-4061-8273-94a5b6c7d8e9:wlP1p1s0\n";

    #[test]
    fn reactivation_brings_up_the_wifi_connection_by_uuid() {
        let uuid = active_uuid(ACTIVE, "wlP1p1s0").unwrap();
        assert_eq!(
            reactivate_args(&uuid),
            [
                "--wait",
                "30",
                "connection",
                "up",
                "uuid",
                "8a1b2c3d-4e5f-4061-8273-94a5b6c7d8e9"
            ]
        );
    }

    #[test]
    fn no_active_connection_on_the_interface_gives_no_uuid() {
        assert_eq!(active_uuid(ACTIVE, "wlan0"), None);
    }

    const CHRONY_CONF: &str = include_str!("../utils/fixtures/settings/chrony_conf_platform_c.txt");

    #[test]
    fn a_running_timesyncd_wins_over_a_chrony_config() {
        assert_eq!(
            ntp_daemon(true, Some(CHRONY_CONF)).unwrap(),
            Some(NtpDaemon::Timesyncd)
        );
    }

    #[test]
    fn chrony_is_used_when_its_config_reads_sources_d() {
        assert_eq!(
            ntp_daemon(false, Some(CHRONY_CONF)).unwrap(),
            Some(NtpDaemon::Chrony)
        );
    }

    #[test]
    fn chrony_without_sourcedir_sources_d_is_an_error() {
        let conf = CHRONY_CONF.replace("sourcedir /etc/chrony/sources.d", "");
        assert!(ntp_daemon(false, Some(&conf)).is_err());
    }

    #[test]
    fn no_ntp_daemon_without_timesyncd_or_a_chrony_config() {
        assert_eq!(ntp_daemon(false, None).unwrap(), None);
    }

    #[test]
    fn ntp_reload_per_daemon() {
        assert_eq!(
            ntp_reload_command(NtpDaemon::Chrony),
            ("chronyc", &["reload", "sources"][..])
        );
        assert_eq!(
            ntp_reload_command(NtpDaemon::Timesyncd),
            ("systemctl", &["restart", "systemd-timesyncd"][..])
        );
    }

    #[test]
    fn hosts_self_line_is_replaced_in_place() {
        let hosts = "127.0.0.1\tlocalhost\n127.0.1.1\torin-1\n\n::1 ip6-localhost\n";
        assert_eq!(
            hosts_with_hostname(hosts, "byag-12"),
            "127.0.0.1\tlocalhost\n127.0.1.1\tbyag-12\n\n::1 ip6-localhost\n"
        );
    }

    #[test]
    fn hosts_self_line_is_added_when_missing() {
        assert_eq!(
            hosts_with_hostname("127.0.0.1 localhost\n", "byag-12"),
            "127.0.0.1 localhost\n127.0.1.1\tbyag-12\n"
        );
    }

    #[test]
    fn duplicate_hosts_self_lines_collapse_into_one() {
        let twice = "127.0.1.1 a\n127.0.1.1 b\n";
        assert_eq!(hosts_with_hostname(twice, "c"), "127.0.1.1\tc\n");
    }

    fn owned_paths(root: &Path) -> OwnedPaths {
        OwnedPaths {
            driver: root.join("modprobe.d/zz-smith-rtl8822ce.conf"),
            nm: root.join("conf.d/zz-smith.conf"),
            chrony: root.join("sources.d/smith.sources"),
            timesyncd: root.join("timesyncd.conf.d/zz-smith.conf"),
        }
    }

    #[tokio::test]
    async fn a_written_file_is_read_back_and_absent_ones_read_as_none() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = owned_paths(tmp.path());
        let text = "options rtl8822ce rtw_power_mgnt=0\n".to_owned();
        change_file(&paths.driver, &FileChange::Write(text.clone()))
            .await
            .unwrap();
        assert_eq!(
            paths.read(Some(NtpDaemon::Chrony)).await.unwrap(),
            OwnedFiles {
                driver: Some(text),
                nm: None,
                ntp: None,
            }
        );
    }

    #[tokio::test]
    async fn removing_a_file_twice_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = owned_paths(tmp.path());
        change_file(&paths.driver, &FileChange::Write("x".to_owned()))
            .await
            .unwrap();
        change_file(&paths.driver, &FileChange::Remove)
            .await
            .unwrap();
        change_file(&paths.driver, &FileChange::Remove)
            .await
            .unwrap();
        assert_eq!(paths.read(None).await.unwrap().driver, None);
    }

    #[tokio::test]
    async fn roam_values_are_written_to_the_interface_proc_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = DriverPaths {
            net: tmp.path().join("net"),
            params: tmp.path().join("params"),
            proc: tmp.path().join("proc"),
        };
        let dir = paths.proc.join("wlP1p1s0");
        std::fs::create_dir_all(&dir).unwrap();
        write_roam_tuning(&paths, "wlP1p1s0", &tuning(), true)
            .await
            .unwrap();
        write_roam_flags(&paths, "wlP1p1s0", 3).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("roam_param")).unwrap(),
            "15 15 10000 5 30 30"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("roam_flags")).unwrap(),
            "0x03"
        );
    }
}
