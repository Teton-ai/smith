use crate::utils::schema::{DeviceSetting, SettingsReport};
use anyhow::{Context, Result, anyhow, bail};
use std::path::{Path, PathBuf};

const SYS_CLASS_NET: &str = "/sys/class/net";
const VENDOR_MODULE: &str = "rtl8822ce";
const VENDOR_PARAMS: &str = "/sys/module/rtl8822ce/parameters";
/// The vendor driver names its proc dir after `DRV_NAME`, not the module.
const VENDOR_PROC: &str = "/proc/net/rtl88x2ce";

pub const DRIVER_FIELDS: [DeviceSetting; 9] = [
    DeviceSetting::WirelessMode,
    DeviceSetting::PowerMgnt,
    DeviceSetting::RoamFlags,
    DeviceSetting::IdleDiff,
    DeviceSetting::BusyDiff,
    DeviceSetting::ScanrExpMs,
    DeviceSetting::ScanInt,
    DeviceSetting::IdleTh,
    DeviceSetting::BusyTh,
];

const ROAM_FIELDS: [DeviceSetting; 6] = [
    DeviceSetting::IdleDiff,
    DeviceSetting::BusyDiff,
    DeviceSetting::ScanrExpMs,
    DeviceSetting::ScanInt,
    DeviceSetting::IdleTh,
    DeviceSetting::BusyTh,
];

/// A wireless interface and the kernel driver bound to it, as NetworkManager's
/// `match-device=driver:` sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiInterface {
    pub name: String,
    pub driver: String,
    pub module: String,
}

/// `roam_param` mapped by meaning. `busy_*` are `None` on drivers without the
/// idle/busy split (v5.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoamParams {
    pub idle_diff: i32,
    pub busy_diff: Option<i32>,
    pub scanr_exp_ms: i32,
    pub scan_int: i32,
    pub idle_th: i32,
    pub busy_th: Option<i32>,
}

pub fn wifi_interfaces(net_dir: &Path) -> Vec<WifiInterface> {
    let Ok(entries) = std::fs::read_dir(net_dir) else {
        return Vec::new();
    };
    let mut interfaces: Vec<WifiInterface> = entries
        .flatten()
        .filter(|entry| entry.path().join("phy80211").exists())
        .filter_map(|entry| {
            let path = entry.path();
            Some(WifiInterface {
                name: entry.file_name().to_str()?.to_owned(),
                driver: link_name(&path.join("device/driver"))?,
                module: link_name(&path.join("device/driver/module"))?,
            })
        })
        .collect();
    interfaces.sort_by(|a, b| a.name.cmp(&b.name));
    interfaces
}

fn link_name(path: &Path) -> Option<String> {
    let target = std::fs::read_link(path).ok()?;
    Some(target.file_name()?.to_str()?.to_owned())
}

/// Strips the `(unit:2 sec)` suffixes so each header is one token.
fn roam_header_names(header: &str) -> Vec<String> {
    let mut cleaned = String::with_capacity(header.len());
    let mut depth = 0usize;
    for c in header.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => cleaned.push(c),
            _ => {}
        }
    }
    cleaned.split_whitespace().map(str::to_owned).collect()
}

pub fn parse_roam_param(text: &str) -> Result<RoamParams> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = lines.next().ok_or_else(|| anyhow!("empty roam_param"))?;
    let values = lines
        .next()
        .ok_or_else(|| anyhow!("roam_param has no values line"))?;

    let names = roam_header_names(header);
    let values: Vec<&str> = values.split_whitespace().collect();
    if names.len() != values.len() {
        bail!(
            "roam_param has {} headers but {} values",
            names.len(),
            values.len()
        );
    }

    let (mut idle_diff, mut busy_diff, mut scanr_exp_ms, mut scan_int, mut idle_th, mut busy_th) =
        (None, None, None, None, None, None);
    for (name, value) in names.iter().zip(values) {
        let value: i32 = value
            .parse()
            .with_context(|| format!("roam_param {name}={value:?}"))?;
        let slot = match name.as_str() {
            "idle_rssi_diff_th" | "rssi_diff_th" => &mut idle_diff,
            "busy_rssi_diff_th" => &mut busy_diff,
            "scanr_exp_ms" => &mut scanr_exp_ms,
            "scan_int" | "scan_interval" => &mut scan_int,
            "idle_rssi_th" | "rssi_threshold" => &mut idle_th,
            "busy_rssi_th" => &mut busy_th,
            other => bail!("unknown roam_param field {other:?}"),
        };
        if slot.replace(value).is_some() {
            bail!("roam_param field {name:?} appears twice");
        }
    }

    let required = |v: Option<i32>, name: &str| v.ok_or_else(|| anyhow!("roam_param lacks {name}"));
    Ok(RoamParams {
        idle_diff: required(idle_diff, "the (idle) rssi diff")?,
        busy_diff,
        scanr_exp_ms: required(scanr_exp_ms, "scanr_exp_ms")?,
        scan_int: required(scan_int, "the scan interval")?,
        idle_th: required(idle_th, "the (idle) rssi threshold")?,
        busy_th,
    })
}

pub fn parse_roam_flags(text: &str) -> Result<i16> {
    let trimmed = text.trim();
    let hex = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .ok_or_else(|| anyhow!("roam_flags {trimmed:?} is not hex"))?;
    i16::from_str_radix(hex, 16).with_context(|| format!("roam_flags {trimmed:?}"))
}

/// Normalizes a country code: none (`00`, the vendor's `unspecified`) becomes
/// `"00"`, anything else must be two ASCII letters.
pub fn parse_country_code(text: &str) -> Result<String> {
    let trimmed = text.trim();
    if trimmed == "00" || trimmed.eq_ignore_ascii_case("unspecified") {
        return Ok("00".to_owned());
    }
    if trimmed.len() == 2 && trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        return Ok(trimmed.to_ascii_uppercase());
    }
    bail!("unrecognized country code {trimmed:?}")
}

fn parse_int<T: std::str::FromStr>(text: &str) -> Result<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    text.trim()
        .parse()
        .with_context(|| format!("parsing {:?}", text.trim()))
}

/// Where the vendor driver exposes its values. Overridable so tests can point
/// at a fake tree.
pub struct DriverPaths {
    pub net: PathBuf,
    pub params: PathBuf,
    pub proc: PathBuf,
}

impl Default for DriverPaths {
    fn default() -> Self {
        Self {
            net: PathBuf::from(SYS_CLASS_NET),
            params: PathBuf::from(VENDOR_PARAMS),
            proc: PathBuf::from(VENDOR_PROC),
        }
    }
}

/// Result of reading the vendor driver. `vendor_present` is false when no
/// interface is bound to the vendor module or its proc dir is missing, which
/// is also what a driver reload looks like for a few seconds.
#[derive(Debug, Default)]
pub struct DriverReading {
    pub vendor_present: bool,
    /// Country code from the vendor proc file, when the vendor driver is there.
    pub country_code: Option<Result<String>>,
}

/// Fills the vendor driver fields of `report`, listing what could not be read
/// in `report.read_errors`.
pub fn read_driver(paths: &DriverPaths, report: &mut SettingsReport) -> DriverReading {
    let interface = wifi_interfaces(&paths.net)
        .into_iter()
        .find(|iface| iface.module == VENDOR_MODULE);

    match std::fs::read_to_string(paths.params.join("rtw_wireless_mode"))
        .map_err(anyhow::Error::from)
        .and_then(|t| parse_int(&t))
    {
        Ok(v) => report.wireless_mode = Some(v),
        Err(_) => report.read_errors.push(DeviceSetting::WirelessMode),
    }
    match std::fs::read_to_string(paths.params.join("rtw_power_mgnt"))
        .map_err(anyhow::Error::from)
        .and_then(|t| parse_int(&t))
    {
        Ok(v) => report.power_mgnt = Some(v),
        Err(_) => report.read_errors.push(DeviceSetting::PowerMgnt),
    }

    let proc_dir = interface.as_ref().map(|iface| paths.proc.join(&iface.name));
    let vendor_present = proc_dir.as_ref().is_some_and(|dir| dir.is_dir());
    let read_proc = |file: &str| -> Result<String> {
        let dir = proc_dir
            .as_ref()
            .ok_or_else(|| anyhow!("no interface bound to {VENDOR_MODULE}"))?;
        std::fs::read_to_string(dir.join(file)).with_context(|| format!("reading {file}"))
    };

    match read_proc("roam_flags").and_then(|t| parse_roam_flags(&t)) {
        Ok(v) => report.roam_flags = Some(v),
        Err(_) => report.read_errors.push(DeviceSetting::RoamFlags),
    }
    match read_proc("roam_param").and_then(|t| parse_roam_param(&t)) {
        Ok(roam) => {
            report.idle_diff = Some(roam.idle_diff);
            report.busy_diff = roam.busy_diff;
            report.scanr_exp_ms = Some(roam.scanr_exp_ms);
            report.scan_int = Some(roam.scan_int);
            report.idle_th = Some(roam.idle_th);
            report.busy_th = roam.busy_th;
        }
        Err(_) => report.read_errors.extend(ROAM_FIELDS),
    }

    DriverReading {
        vendor_present,
        country_code: interface
            .is_some()
            .then(|| read_proc("country_code").and_then(|t| parse_country_code(&t))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    const ROAM_V5_9: &str = include_str!("../utils/fixtures/settings/roam_param_v5_9.txt");
    const ROAM_V5_14: &str = include_str!("../utils/fixtures/settings/roam_param_v5_14.txt");

    #[test]
    fn roam_param_v5_14_maps_six_fields_by_name() {
        let roam = parse_roam_param(ROAM_V5_14).unwrap();
        assert_eq!(
            roam,
            RoamParams {
                idle_diff: 5,
                busy_diff: Some(10),
                scanr_exp_ms: 10000,
                scan_int: 2,
                idle_th: 40,
                busy_th: Some(40),
            }
        );
    }

    #[test]
    fn roam_param_v5_9_maps_four_fields_to_idle() {
        let roam = parse_roam_param(ROAM_V5_9).unwrap();
        assert_eq!(
            roam,
            RoamParams {
                idle_diff: 10,
                busy_diff: None,
                scanr_exp_ms: 5000,
                scan_int: 5,
                idle_th: 70,
                busy_th: None,
            }
        );
    }

    #[test]
    fn roam_param_rejects_count_mismatch_and_unknown_fields() {
        let short = ROAM_V5_14.replace("40            40", "40");
        assert!(parse_roam_param(&short).is_err());
        assert!(parse_roam_param("foo_th\n1\n").is_err());
        assert!(parse_roam_param("").is_err());
    }

    #[test]
    fn roam_flags_and_country_code_fixtures() {
        let flags = |name| match name {
            "v5_9" => include_str!("../utils/fixtures/settings/roam_flags_v5_9.txt"),
            _ => include_str!("../utils/fixtures/settings/roam_flags_v5_14.txt"),
        };
        assert_eq!(parse_roam_flags(flags("v5_9")).unwrap(), 0x03);
        assert_eq!(parse_roam_flags(flags("v5_14")).unwrap(), 0x07);
        assert!(parse_roam_flags("7").is_err());

        let cc_v5_9 = include_str!("../utils/fixtures/settings/country_code_v5_9.txt");
        let cc_v5_14 = include_str!("../utils/fixtures/settings/country_code_v5_14.txt");
        assert_eq!(parse_country_code(cc_v5_9).unwrap(), "00");
        assert_eq!(parse_country_code(cc_v5_14).unwrap(), "00");
        assert_eq!(parse_country_code("dk\n").unwrap(), "DK");
        assert!(parse_country_code("\u{ff}\u{ff}").is_err());
    }

    /// Builds a fake `/sys/class/net/<iface>` with a driver and module link.
    fn fake_iface(root: &Path, name: &str, driver: &str, module: &str) {
        let iface = root.join("net").join(name);
        let drivers = root.join("drivers").join(driver);
        let modules = root.join("modules").join(module);
        std::fs::create_dir_all(iface.join("device")).unwrap();
        std::fs::create_dir_all(&drivers).unwrap();
        std::fs::create_dir_all(&modules).unwrap();
        std::fs::create_dir_all(iface.join("phy80211")).unwrap();
        symlink(&drivers, iface.join("device/driver")).unwrap();
        symlink(&modules, drivers.join("module")).unwrap();
    }

    fn paths(root: &Path) -> DriverPaths {
        DriverPaths {
            net: root.join("net"),
            params: root.join("params"),
            proc: root.join("proc"),
        }
    }

    #[test]
    fn reads_vendor_driver_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtl88x2ce", "rtl8822ce");
        std::fs::create_dir_all(root.join("params")).unwrap();
        std::fs::write(root.join("params/rtw_wireless_mode"), "95\n").unwrap();
        std::fs::write(root.join("params/rtw_power_mgnt"), "2\n").unwrap();
        let proc = root.join("proc/wlP1p1s0");
        std::fs::create_dir_all(&proc).unwrap();
        std::fs::write(proc.join("roam_param"), ROAM_V5_14).unwrap();
        std::fs::write(proc.join("roam_flags"), "0x07\n").unwrap();
        std::fs::write(proc.join("country_code"), "00\n").unwrap();

        let mut report = SettingsReport::default();
        let reading = read_driver(&paths(root), &mut report);

        assert!(reading.vendor_present);
        assert_eq!(reading.country_code.unwrap().unwrap(), "00");
        assert_eq!(report.wireless_mode, Some(95));
        assert_eq!(report.power_mgnt, Some(2));
        assert_eq!(report.roam_flags, Some(7));
        assert_eq!(report.busy_th, Some(40));
        assert!(report.read_errors.is_empty());
    }

    #[test]
    fn wrong_roam_param_marks_all_six_roam_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlan0", "rtl88x2ce", "rtl8822ce");
        let proc = root.join("proc/wlan0");
        std::fs::create_dir_all(&proc).unwrap();
        std::fs::write(proc.join("roam_param"), "a b c\n1 2\n").unwrap();
        std::fs::write(proc.join("roam_flags"), "0x03\n").unwrap();

        let mut report = SettingsReport::default();
        read_driver(&paths(root), &mut report);

        for field in ROAM_FIELDS {
            assert!(report.read_errors.contains(&field), "{field:?}");
        }
        assert_eq!(report.roam_flags, Some(3));
        assert_eq!(report.idle_diff, None);
    }

    #[test]
    fn missing_vendor_interface_marks_every_driver_field() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtw_8822ce", "rtw88_8822ce");

        let mut report = SettingsReport::default();
        let reading = read_driver(&paths(root), &mut report);

        assert!(!reading.vendor_present);
        assert!(reading.country_code.is_none());
        for field in DRIVER_FIELDS {
            assert!(report.read_errors.contains(&field), "{field:?}");
        }
        assert_eq!(
            wifi_interfaces(&root.join("net")),
            vec![WifiInterface {
                name: "wlP1p1s0".to_owned(),
                driver: "rtw_8822ce".to_owned(),
                module: "rtw88_8822ce".to_owned(),
            }]
        );
    }
}
