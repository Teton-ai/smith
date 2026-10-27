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

/// `*` and `?` globbing, as NetworkManager allows in `interface-name:`.
fn glob_match(pattern: &str, value: &str) -> bool {
    match pattern.chars().next() {
        None => value.is_empty(),
        Some('*') => (0..=value.len())
            .filter(|i| value.is_char_boundary(*i))
            .any(|i| glob_match(&pattern[1..], &value[i..])),
        Some(c) => {
            value.chars().next().is_some_and(|v| c == '?' || c == v) && {
                let n = c.len_utf8();
                let m = value.chars().next().map_or(0, char::len_utf8);
                glob_match(&pattern[n..], &value[m..])
            }
        }
    }
}

/// One positive spec from NetworkManager's "Device List Format". Specs we do
/// not evaluate (`mac:`, `s390-subchannels:`, ...) never match.
fn spec_matches(spec: &str, wifi: &WifiInterface) -> bool {
    if spec == "*" {
        return true;
    }
    if let Some(name) = spec.strip_prefix("interface-name:") {
        return match name.strip_prefix('=') {
            Some(literal) => literal == wifi.name,
            None => glob_match(name.strip_prefix('~').unwrap_or(name), &wifi.name),
        };
    }
    if let Some(driver) = spec.strip_prefix("driver:") {
        // An optional `/version` follows the driver name.
        return driver.split('/').next() == Some(wifi.driver.as_str());
    }
    if let Some(kind) = spec.strip_prefix("type:") {
        return kind == "wifi";
    }
    !spec.contains(':') && spec == wifi.name
}

/// Whether a per-device section's `match-device` selects our Wi-Fi interface:
/// any matching `except:` wins, and a list of only `except:` specs behaves as
/// `*` plus those exceptions (NetworkManager.conf(5), "Device List Format").
fn nm_match_device(list: &str, wifi: Option<&WifiInterface>) -> bool {
    let Some(wifi) = wifi else {
        return false;
    };
    let specs: Vec<&str> = list
        .split([',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let (negative, positive): (Vec<&str>, Vec<&str>) =
        specs.iter().partition(|s| s.starts_with("except:"));
    if negative
        .iter()
        .filter_map(|s| s.strip_prefix("except:"))
        .any(|s| spec_matches(s, wifi))
    {
        return false;
    }
    positive.is_empty() || positive.iter().any(|s| spec_matches(s, wifi))
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

/// NetworkManager accepts 0 to `i32::MAX` and falls back to its built-in value
/// for anything else, empty and negative included (`nm-config-data.c`).
fn parse_retries_default(value: &str) -> i32 {
    parse_int::<i32>(value)
        .ok()
        .filter(|retries| *retries >= 0)
        .unwrap_or(NM_DEFAULT_AUTOCONNECT_RETRIES)
}

/// Reads `[device*]` / `[connection*]` the way NetworkManager does
/// (NetworkManager.conf(5)): sections in order, plain `[device]` /
/// `[connection]` last, the first value found for a key wins. A section with
/// `match-device` counts only when it selects `wifi` (without it, it applies to
/// every device); a matching `stop-match=yes` ends the search for its kind.
/// The autoconnect retries default is a `[main]` key; NetworkManager rejects
/// `connection.autoconnect-retries` in `[connection]` as unknown.
pub fn parse_nm_config(text: &str, wifi: Option<&WifiInterface>) -> Result<NmConfig> {
    let (mut powersave, mut retries, mut cloned_mac, mut scan_rand_mac) = (None, None, None, None);
    let (mut device_stopped, mut connection_stopped) = (false, false);
    let mut sections = parse_nm_sections(text);
    sections.sort_by_key(|section| !section.name.contains('-'));
    for section in sections {
        if section.name == "main" {
            if let Some((_, value)) = section
                .entries
                .iter()
                .find(|(key, _)| key == "autoconnect-retries-default")
            {
                retries = Some(parse_retries_default(value));
            }
            continue;
        }
        let kind = if section.name == "device" || section.name.starts_with("device-") {
            "device"
        } else if section.name == "connection" || section.name.starts_with("connection-") {
            "connection"
        } else {
            continue;
        };
        let stopped = match kind {
            "device" => &mut device_stopped,
            _ => &mut connection_stopped,
        };
        let value_of = |wanted: &str| {
            section
                .entries
                .iter()
                .find(|(key, _)| key == wanted)
                .map(|(_, value)| value.as_str())
        };
        if *stopped || value_of("match-device").is_some_and(|spec| !nm_match_device(spec, wifi)) {
            continue;
        }
        *stopped = match value_of("stop-match") {
            Some(value) => parse_nm_bool(value)?,
            None => false,
        };
        for (key, value) in &section.entries {
            match (kind, key.as_str()) {
                ("device", "wifi.scan-rand-mac-address") if scan_rand_mac.is_none() => {
                    scan_rand_mac = Some(parse_nm_bool(value)?)
                }
                ("connection", "wifi.powersave") if powersave.is_none() => {
                    powersave = Some(parse_nm_powersave(value)?)
                }
                ("connection", "wifi.cloned-mac-address") if cloned_mac.is_none() => {
                    cloned_mac = Some(value.clone())
                }
                _ => {}
            }
        }
    }
    Ok(NmConfig {
        wifi_powersave: powersave.unwrap_or(NM_DEFAULT_POWERSAVE),
        autoconnect_retries: retries.unwrap_or(NM_DEFAULT_AUTOCONNECT_RETRIES),
        cloned_mac: cloned_mac.unwrap_or_else(|| NM_DEFAULT_CLONED_MAC.to_owned()),
        scan_rand_mac: scan_rand_mac.unwrap_or(NM_DEFAULT_SCAN_RAND_MAC),
    })
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
    fn nm_sections_follow_network_manager_precedence() {
        // Plain sections are read last even though they come first; the first
        // matching section wins; a non-matching except: is a match; a section
        // without match-device applies; stop-match ends only its own kind.
        let text = "[device]\nmatch-device=interface-name:eth3\nwifi.scan-rand-mac-address=no\n\n\
            [connection]\nmatch-device=interface-name:eth3\nwifi.cloned-mac-address=random\n\n\
            [connection]\nwifi.powersave=3\nwifi.cloned-mac-address=stable\n\n\
            [device-other]\nmatch-device=driver:eagle_sdio;driver:wl\nwifi.scan-rand-mac-address=yes\n\n\
            [device-everyone]\nstop-match=yes\n\n\
            [device-too-late]\nwifi.scan-rand-mac-address=no\n\n\
            [connection-ours]\nmatch-device=interface-name:wlP*\nwifi.powersave=2\n\n\
            [connection-also-ours]\nmatch-device=except:driver:iwlwifi\nwifi.powersave=1\nconnection.autoconnect-retries=0\n\n\
            [connection-not-ours]\nmatch-device=type:wifi,except:driver:rtl88x2ce\nwifi.cloned-mac-address=random\n";
        let config = parse_nm_config(text, Some(&vendor_wifi())).unwrap();
        assert_eq!(
            config,
            NmConfig {
                wifi_powersave: NM_POWERSAVE_DISABLE,
                autoconnect_retries: NM_DEFAULT_AUTOCONNECT_RETRIES,
                cloned_mac: "stable".to_owned(),
                scan_rand_mac: NM_DEFAULT_SCAN_RAND_MAC,
            }
        );
    }

    #[test]
    fn autoconnect_retries_default_comes_from_main() {
        let text = "[main]\nplugins=ifupdown,keyfile\nautoconnect-retries-default=0\n\n\
            [connection]\nconnection.autoconnect-retries=7\n";
        let config = parse_nm_config(text, Some(&vendor_wifi())).unwrap();
        assert_eq!(config.autoconnect_retries, 0);

        let config = parse_nm_config("[main]\nplugins=keyfile\n", Some(&vendor_wifi())).unwrap();
        assert_eq!(config.autoconnect_retries, NM_DEFAULT_AUTOCONNECT_RETRIES);

        for invalid in ["", "abc", "-1", "4294967296"] {
            let text = format!("[main]\nautoconnect-retries-default={invalid}\n");
            let config = parse_nm_config(&text, Some(&vendor_wifi())).unwrap();
            assert_eq!(
                config.autoconnect_retries, NM_DEFAULT_AUTOCONNECT_RETRIES,
                "{invalid:?}"
            );
        }
    }
}
