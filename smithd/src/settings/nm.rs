//! NetworkManager global Wi-Fi defaults, from `NetworkManager --print-config`.

use super::driver::WifiInterface;
use super::read::{parse_int, run};
use crate::utils::schema::DeviceSetting;
use anyhow::{Result, bail};

/// `wifi.powersave` values (NetworkManager `NMSettingWirelessPowersave`).
const NM_POWERSAVE_DEFAULT: i16 = 0;
const NM_POWERSAVE_IGNORE: i16 = 1;
const NM_POWERSAVE_DISABLE: i16 = 2;
const NM_POWERSAVE_ENABLE: i16 = 3;

// Built-in values NetworkManager uses when the config does not set the key
// (NetworkManager.conf(5)); `--print-config` only prints keys that are set.
const NM_DEFAULT_POWERSAVE: i16 = NM_POWERSAVE_IGNORE;
const NM_DEFAULT_AUTOCONNECT_RETRIES: i32 = 4;
const NM_DEFAULT_CLONED_MAC: &str = "preserve";
const NM_DEFAULT_SCAN_RAND_MAC: bool = true;

pub const NM_FIELDS: [DeviceSetting; 4] = [
    DeviceSetting::NmWifiPowersave,
    DeviceSetting::NmAutoconnectRetries,
    DeviceSetting::NmClonedMac,
    DeviceSetting::NmScanRandMac,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NmConfig {
    pub wifi_powersave: i16,
    pub autoconnect_retries: i32,
    pub cloned_mac: String,
    pub scan_rand_mac: bool,
}

struct NmSection {
    name: String,
    entries: Vec<(String, String)>,
}

fn parse_nm_sections(text: &str) -> Vec<NmSection> {
    let mut sections: Vec<NmSection> = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push(NmSection {
                name: name.to_owned(),
                entries: Vec::new(),
            });
        } else if let (Some(section), Some((key, value))) =
            (sections.last_mut(), line.split_once('='))
        {
            section
                .entries
                .push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    sections
}

/// Whether a per-device section's `match-device` selects our Wi-Fi interface.
/// Only `driver:` and `interface-name:` (exact or trailing `*`) are understood;
/// anything else, including `except:`, counts as not matching.
fn nm_match_device(spec: &str, wifi: Option<&WifiInterface>) -> bool {
    let Some(wifi) = wifi else {
        return false;
    };
    let items: Vec<&str> = spec.split(',').map(str::trim).collect();
    if items.iter().any(|item| item.starts_with("except:")) {
        return false;
    }
    let glob = |pattern: &str, value: &str| match pattern.strip_suffix('*') {
        Some(prefix) => value.starts_with(prefix),
        None => pattern == value,
    };
    items.iter().any(|item| {
        if let Some(driver) = item.strip_prefix("driver:") {
            glob(driver, &wifi.driver)
        } else if let Some(name) = item.strip_prefix("interface-name:") {
            glob(name, &wifi.name)
        } else {
            false
        }
    })
}

fn parse_nm_bool(value: &str) -> Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "yes" | "true" | "on" | "1" => Ok(true),
        "no" | "false" | "off" | "0" => Ok(false),
        other => bail!("not a boolean: {other:?}"),
    }
}

fn parse_nm_powersave(value: &str) -> Result<i16> {
    match value.to_ascii_lowercase().as_str() {
        "default" => Ok(NM_POWERSAVE_DEFAULT),
        "ignore" => Ok(NM_POWERSAVE_IGNORE),
        "disable" => Ok(NM_POWERSAVE_DISABLE),
        "enable" => Ok(NM_POWERSAVE_ENABLE),
        other => parse_int(other),
    }
}

/// Applies `[device]` / `[connection]` sections, and their `-suffix` variants
/// when `match-device` selects `wifi`, in file order so later sections win.
pub fn parse_nm_config(text: &str, wifi: Option<&WifiInterface>) -> Result<NmConfig> {
    let mut config = NmConfig {
        wifi_powersave: NM_DEFAULT_POWERSAVE,
        autoconnect_retries: NM_DEFAULT_AUTOCONNECT_RETRIES,
        cloned_mac: NM_DEFAULT_CLONED_MAC.to_owned(),
        scan_rand_mac: NM_DEFAULT_SCAN_RAND_MAC,
    };
    for section in parse_nm_sections(text) {
        let kind = if section.name == "device" || section.name.starts_with("device-") {
            "device"
        } else if section.name == "connection" || section.name.starts_with("connection-") {
            "connection"
        } else {
            continue;
        };
        if section.name.contains('-') {
            let spec = section
                .entries
                .iter()
                .find(|(key, _)| key == "match-device")
                .map(|(_, value)| value.as_str());
            if !spec.is_some_and(|spec| nm_match_device(spec, wifi)) {
                continue;
            }
        }
        for (key, value) in &section.entries {
            match (kind, key.as_str()) {
                ("device", "wifi.scan-rand-mac-address") => {
                    config.scan_rand_mac = parse_nm_bool(value)?
                }
                ("connection", "wifi.powersave") => {
                    config.wifi_powersave = parse_nm_powersave(value)?
                }
                ("connection", "connection.autoconnect-retries") => {
                    config.autoconnect_retries = parse_int(value)?
                }
                ("connection", "wifi.cloned-mac-address") => config.cloned_mac = value.clone(),
                _ => {}
            }
        }
    }
    Ok(config)
}

pub async fn read_nm_config(wifi: Option<&WifiInterface>) -> Result<NmConfig> {
    parse_nm_config(&run("NetworkManager", &["--print-config"]).await?, wifi)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NM_1_22: &str = include_str!("../utils/fixtures/settings/nm_print_config_1_22.txt");
    const NM_1_36: &str = include_str!("../utils/fixtures/settings/nm_print_config_1_36.txt");

    fn vendor_wifi() -> WifiInterface {
        WifiInterface {
            name: "wlP1p1s0".to_owned(),
            driver: "rtl88x2ce".to_owned(),
            module: "rtl8822ce".to_owned(),
        }
    }

    #[test]
    fn nm_fixtures_set_keys_and_fall_back_to_built_ins() {
        for fixture in [NM_1_22, NM_1_36] {
            let config = parse_nm_config(fixture, Some(&vendor_wifi())).unwrap();
            assert_eq!(
                config,
                NmConfig {
                    wifi_powersave: NM_POWERSAVE_ENABLE,
                    autoconnect_retries: NM_DEFAULT_AUTOCONNECT_RETRIES,
                    cloned_mac: NM_DEFAULT_CLONED_MAC.to_owned(),
                    scan_rand_mac: false,
                }
            );
        }
    }

    #[test]
    fn nm_per_device_sections_apply_only_when_matching() {
        let text = "[device]\nwifi.scan-rand-mac-address=no\n\n\
            [device]\nwifi.scan-rand-mac-address=no\n\n\
            [device-other]\nmatch-device=driver:eagle_sdio,driver:wl\nwifi.scan-rand-mac-address=yes\n\n\
            [connection-ours]\nmatch-device=driver:rtl88x2ce\nwifi.powersave=2\nconnection.autoconnect-retries=0\n\n\
            [connection-except]\nmatch-device=except:driver:iwlwifi\nwifi.cloned-mac-address=random\n";
        let config = parse_nm_config(text, Some(&vendor_wifi())).unwrap();
        assert!(
            !config.scan_rand_mac,
            "non-matching [device-*] must be ignored"
        );
        assert_eq!(config.wifi_powersave, NM_POWERSAVE_DISABLE);
        assert_eq!(config.autoconnect_retries, 0);
        assert_eq!(config.cloned_mac, NM_DEFAULT_CLONED_MAC);
    }
}
