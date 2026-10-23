//! Realtek vendor driver (`rtl8822ce`) settings, Wi-Fi interface discovery and
//! the regulatory country.

use super::read::{parse_int, run};
use crate::utils::schema::{DeviceSetting, SettingsReport};
use anyhow::{Context, Result, anyhow, bail};
use std::path::{Path, PathBuf};

pub(crate) const SYS_CLASS_NET: &str = "/sys/class/net";
pub(crate) const VENDOR_MODULE: &str = "rtl8822ce";
const VENDOR_PARAMS: &str = "/sys/module/rtl8822ce/parameters";
/// The vendor driver names its proc dir after `DRV_NAME`, not the module.
const VENDOR_PROC: &str = "/proc/net/rtl88x2ce";

/// Reported when no country is set (`00` is the world regulatory domain).
const NO_COUNTRY: &str = "00";
/// What the v5.9 vendor driver prints instead of `00`.
const VENDOR_UNSPECIFIED_COUNTRY: &str = "unspecified";

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

/// The Wi-Fi interface every reader works on: the vendor driver's if there is
/// one, else the first physical one. Virtual interfaces (a hotspot `ap0`) have
/// no `device` link and are skipped.
pub async fn wifi_interface(net_dir: &Path) -> Option<WifiInterface> {
    let interfaces = wifi_interfaces(net_dir).await;
    let vendor = interfaces.iter().position(|i| i.module == VENDOR_MODULE);
    interfaces.into_iter().nth(vendor.unwrap_or(0))
}

/// Sorted by name so the fallback choice is stable.
async fn wifi_interfaces(net_dir: &Path) -> Vec<WifiInterface> {
    let mut interfaces = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(net_dir).await else {
        return interfaces;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !tokio::fs::try_exists(path.join("phy80211"))
            .await
            .unwrap_or(false)
        {
            continue;
        }
        let (Some(name), Some(driver), Some(module)) = (
            entry.file_name().to_str().map(str::to_owned),
            link_name(&path.join("device/driver")).await,
            link_name(&path.join("device/driver/module")).await,
        ) else {
            continue;
        };
        interfaces.push(WifiInterface {
            name,
            driver,
            module,
        });
    }
    interfaces.sort_by(|a, b| a.name.cmp(&b.name));
    interfaces
}

async fn link_name(path: &Path) -> Option<String> {
    let target = tokio::fs::read_link(path).await.ok()?;
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

/// Normalizes a country code: none becomes [`NO_COUNTRY`], anything else must
/// be two ASCII letters.
pub fn parse_country_code(text: &str) -> Result<String> {
    let trimmed = text.trim();
    if trimmed == NO_COUNTRY || trimmed.eq_ignore_ascii_case(VENDOR_UNSPECIFIED_COUNTRY) {
        return Ok(NO_COUNTRY.to_owned());
    }
    if trimmed.len() == 2 && trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        return Ok(trimmed.to_ascii_uppercase());
    }
    bail!("unrecognized country code {trimmed:?}")
}

/// Country of the Wi-Fi phy's own regulatory domain when `iw reg get` lists
/// one (self-managed drivers), else the global one.
pub fn parse_iw_reg(text: &str, phy: Option<&str>) -> Result<String> {
    let wanted = phy
        .and_then(|p| p.strip_prefix("phy"))
        .map(|n| format!("phy#{n}"));
    let mut block = String::new();
    let mut global = None;
    let mut ours = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "global" || trimmed.starts_with("phy#") {
            block = trimmed
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned();
        } else if let Some(rest) = trimmed.strip_prefix("country ") {
            let code = rest.split(':').next().unwrap_or_default();
            if block == "global" {
                global = Some(code.to_owned());
            } else if wanted.as_deref() == Some(block.as_str()) {
                ours = Some(code.to_owned());
            }
        }
    }
    let code = ours
        .or(global)
        .ok_or_else(|| anyhow!("no country in iw reg get"))?;
    parse_country_code(&code)
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

async fn read_value<T: std::str::FromStr>(path: PathBuf) -> Result<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let text = tokio::fs::read_to_string(&path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    parse_int(&text)
}

async fn read_proc(dir: Option<&Path>, file: &str) -> Result<String> {
    let dir = dir.ok_or_else(|| anyhow!("no interface bound to {VENDOR_MODULE}"))?;
    tokio::fs::read_to_string(dir.join(file))
        .await
        .with_context(|| format!("reading {file}"))
}

/// Fills the vendor driver fields of `report`, listing what could not be read
/// in `report.read_errors`.
pub async fn read_driver(
    paths: &DriverPaths,
    wifi: Option<&WifiInterface>,
    report: &mut SettingsReport,
) -> DriverReading {
    let interface = wifi.filter(|iface| iface.module == VENDOR_MODULE);

    match read_value(paths.params.join("rtw_wireless_mode")).await {
        Ok(v) => report.wireless_mode = Some(v),
        Err(_) => report.read_errors.push(DeviceSetting::WirelessMode),
    }
    match read_value(paths.params.join("rtw_power_mgnt")).await {
        Ok(v) => report.power_mgnt = Some(v),
        Err(_) => report.read_errors.push(DeviceSetting::PowerMgnt),
    }

    let proc_dir = interface.as_ref().map(|iface| paths.proc.join(&iface.name));
    let vendor_present = match &proc_dir {
        Some(dir) => tokio::fs::metadata(dir)
            .await
            .is_ok_and(|meta| meta.is_dir()),
        None => false,
    };

    match read_proc(proc_dir.as_deref(), "roam_flags")
        .await
        .and_then(|t| parse_roam_flags(&t))
    {
        Ok(v) => report.roam_flags = Some(v),
        Err(_) => report.read_errors.push(DeviceSetting::RoamFlags),
    }
    match read_proc(proc_dir.as_deref(), "roam_param")
        .await
        .and_then(|t| parse_roam_param(&t))
    {
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

    let country_code = match interface {
        Some(_) => Some(
            read_proc(proc_dir.as_deref(), "country_code")
                .await
                .and_then(|t| parse_country_code(&t)),
        ),
        None => None,
    };
    DriverReading {
        vendor_present,
        country_code,
    }
}

/// Country from `iw reg get` for drivers without the vendor proc file.
pub async fn iw_country(net_dir: &Path, wifi: Option<&WifiInterface>) -> Result<String> {
    let wifi = wifi.ok_or_else(|| anyhow!("no Wi-Fi interface"))?;
    let phy = link_name(&net_dir.join(&wifi.name).join("phy80211")).await;
    parse_iw_reg(&run("iw", &["reg", "get"]).await?, phy.as_deref())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    const DRIVER_FIELDS: [DeviceSetting; 9] = [
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
    const ROAM_V5_9: &str = include_str!("../utils/fixtures/settings/roam_param_v5_9.txt");
    const ROAM_V5_14: &str = include_str!("../utils/fixtures/settings/roam_param_v5_14.txt");

    #[test]
    fn roam_param_maps_both_driver_formats_by_name() {
        assert_eq!(
            parse_roam_param(ROAM_V5_14).unwrap(),
            RoamParams {
                idle_diff: 5,
                busy_diff: Some(10),
                scanr_exp_ms: 10000,
                scan_int: 2,
                idle_th: 40,
                busy_th: Some(40),
            }
        );
        assert_eq!(
            parse_roam_param(ROAM_V5_9).unwrap(),
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
    fn roam_flags_and_country_code_fixtures() {
        let flags_v5_9 = include_str!("../utils/fixtures/settings/roam_flags_v5_9.txt");
        let flags_v5_14 = include_str!("../utils/fixtures/settings/roam_flags_v5_14.txt");
        assert_eq!(parse_roam_flags(flags_v5_9).unwrap(), 0x03);
        assert_eq!(parse_roam_flags(flags_v5_14).unwrap(), 0x07);

        let cc_v5_9 = include_str!("../utils/fixtures/settings/country_code_v5_9.txt");
        let cc_v5_14 = include_str!("../utils/fixtures/settings/country_code_v5_14.txt");
        assert_eq!(parse_country_code(cc_v5_9).unwrap(), NO_COUNTRY);
        assert_eq!(parse_country_code(cc_v5_14).unwrap(), NO_COUNTRY);
    }

    #[test]
    fn iw_reg_prefers_the_phy_domain_over_the_global_one() {
        let c = include_str!("../utils/fixtures/settings/iw_reg_get_platform_c.txt");
        assert_eq!(parse_iw_reg(c, Some("phy0")).unwrap(), "DE");

        let both = "global\ncountry DE: DFS-ETSI\n\nphy#0 (self-managed)\ncountry US: DFS-FCC\n";
        assert_eq!(parse_iw_reg(both, Some("phy0")).unwrap(), "US");
    }

    /// Builds a fake `/sys/class/net/<iface>` with a driver and module link.
    pub(crate) fn fake_iface(root: &Path, name: &str, driver: &str, module: &str) {
        let iface = root.join("net").join(name);
        let drivers = root.join("drivers").join(driver);
        let modules = root.join("modules").join(module);
        std::fs::create_dir_all(iface.join("device")).unwrap();
        std::fs::create_dir_all(iface.join("phy80211")).unwrap();
        std::fs::create_dir_all(&drivers).unwrap();
        std::fs::create_dir_all(&modules).unwrap();
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

    #[tokio::test]
    async fn reads_the_vendor_tree_and_flags_a_bad_roam_param() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtl88x2ce", VENDOR_MODULE);
        std::fs::create_dir_all(root.join("params")).unwrap();
        std::fs::write(root.join("params/rtw_wireless_mode"), "95\n").unwrap();
        std::fs::write(root.join("params/rtw_power_mgnt"), "2\n").unwrap();
        let proc = root.join("proc/wlP1p1s0");
        std::fs::create_dir_all(&proc).unwrap();
        std::fs::write(proc.join("roam_param"), ROAM_V5_14).unwrap();
        std::fs::write(proc.join("roam_flags"), "0x07\n").unwrap();
        std::fs::write(proc.join("country_code"), "00\n").unwrap();

        let mut report = SettingsReport::default();
        let wifi = wifi_interface(&root.join("net")).await;
        let reading = read_driver(&paths(root), wifi.as_ref(), &mut report).await;
        assert!(reading.vendor_present);
        assert_eq!(reading.country_code.unwrap().unwrap(), NO_COUNTRY);
        assert_eq!(
            (report.wireless_mode, report.roam_flags, report.busy_th),
            (Some(95), Some(7), Some(40))
        );
        assert!(report.read_errors.is_empty());

        std::fs::write(proc.join("roam_param"), "a b c\n1 2\n").unwrap();
        let mut report = SettingsReport::default();
        read_driver(&paths(root), wifi.as_ref(), &mut report).await;
        assert_eq!(report.read_errors, ROAM_FIELDS);
    }

    #[tokio::test]
    async fn other_wifi_drivers_are_found_but_have_no_vendor_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtw_8822ce", "rtw88_8822ce");

        let mut report = SettingsReport::default();
        let wifi = wifi_interface(&root.join("net")).await;
        let reading = read_driver(&paths(root), wifi.as_ref(), &mut report).await;
        assert!(!reading.vendor_present);
        assert!(reading.country_code.is_none());
        assert_eq!(report.read_errors, DRIVER_FIELDS);

        assert_eq!(wifi.unwrap().driver, "rtw_8822ce");
    }
}
