//! Collects every setting into one [`SettingsReport`], plus the readers too
//! small for their own module (bgscan, hostname) and shared helpers.

use super::driver::{DriverPaths, iw_country, read_driver, wifi_interface};
use super::nm::{NM_FIELDS, read_nm_config};
use super::ntp::{NTP_FIELDS, read_ntp};
use crate::utils::schema::{DeviceSetting, SettingsReport};
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::time::Duration;
use tracing::{debug, warn};
use zbus::Connection;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

const HOSTNAME_PATH: &str = "/etc/hostname";
/// Upper bound for every helper process (`iw`, `chronyc`, `NetworkManager`),
/// each run once per check.
const PROCESS_TIMEOUT: Duration = Duration::from_secs(5);
/// wpa_supplicant's object path for "no network".
const NO_NETWORK_PATH: &str = "/";

#[zbus::proxy(
    interface = "fi.w1.wpa_supplicant1",
    default_service = "fi.w1.wpa_supplicant1",
    default_path = "/fi/w1/wpa_supplicant1"
)]
trait WpaSupplicant {
    #[zbus(property)]
    fn interfaces(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
}

#[zbus::proxy(
    interface = "fi.w1.wpa_supplicant1.Interface",
    default_service = "fi.w1.wpa_supplicant1"
)]
trait WpaInterface {
    #[zbus(property)]
    fn current_network(&self) -> zbus::Result<OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "fi.w1.wpa_supplicant1.Network",
    default_service = "fi.w1.wpa_supplicant1"
)]
trait WpaNetwork {
    #[zbus(property)]
    fn properties(&self) -> zbus::Result<HashMap<String, OwnedValue>>;
}

pub(super) fn parse_int<T: std::str::FromStr>(text: &str) -> Result<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    text.trim()
        .parse()
        .with_context(|| format!("parsing {:?}", text.trim()))
}

pub(super) async fn run(program: impl AsRef<OsStr>, args: &[&str]) -> Result<String> {
    let name = program.as_ref().to_string_lossy().into_owned();
    let output = tokio::time::timeout(
        PROCESS_TIMEOUT,
        tokio::process::Command::new(program).args(args).output(),
    )
    .await
    .with_context(|| format!("{name} timed out"))?
    .with_context(|| format!("running {name}"))?;
    if !output.status.success() {
        bail!("{name} exited with {}", output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// wpa_supplicant returns string network properties in config syntax, quoted.
pub fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

/// bgscan of the network wpa_supplicant is connected to, on any interface.
/// `None` when no interface is connected (e.g. a device on Ethernet) or the
/// network has no bgscan set.
async fn read_bgscan(conn: &Connection) -> Result<Option<String>> {
    let supplicant = WpaSupplicantProxy::builder(conn)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    for path in supplicant.interfaces().await? {
        let interface = WpaInterfaceProxy::builder(conn)
            .path(path)?
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        let network = interface.current_network().await?;
        if network.as_str() == NO_NETWORK_PATH {
            continue;
        }
        let network = WpaNetworkProxy::builder(conn)
            .path(network)?
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        let properties = network.properties().await?;
        return Ok(match properties.get("bgscan") {
            Some(value) => {
                let value = String::try_from(value.try_clone()?)?;
                Some(unquote(&value).to_owned()).filter(|v| !v.is_empty())
            }
            None => None,
        });
    }
    Ok(None)
}

/// Reads every setting; keeps the system D-Bus connection between checks.
#[derive(Default)]
pub struct SettingsReader {
    driver_paths: DriverPaths,
    conn: Option<Connection>,
}

/// One full read. `vendor_present` feeds the reload hold in the actor.
pub struct Reading {
    pub report: SettingsReport,
    pub vendor_present: bool,
}

impl SettingsReader {
    async fn connection(&mut self) -> Result<Connection> {
        if let Some(conn) = &self.conn {
            return Ok(conn.clone());
        }
        let conn = Connection::system().await?;
        self.conn = Some(conn.clone());
        Ok(conn)
    }

    pub async fn read(&mut self, check_interval_s: i32) -> Reading {
        let mut report = SettingsReport {
            check_interval_s: Some(check_interval_s),
            ..Default::default()
        };
        let wifi = wifi_interface(&self.driver_paths.net).await;
        let driver = read_driver(&self.driver_paths, wifi.as_ref(), &mut report).await;

        match driver.country_code {
            Some(Ok(code)) => report.country_code = Some(code),
            Some(Err(err)) => {
                warn!("settings: vendor country code: {err:#}");
                report.read_errors.push(DeviceSetting::CountryCode);
            }
            None => match iw_country(&self.driver_paths.net, wifi.as_ref()).await {
                Ok(code) => report.country_code = Some(code),
                Err(err) => {
                    debug!("settings: country code: {err:#}");
                    report.read_errors.push(DeviceSetting::CountryCode);
                }
            },
        }

        match read_nm_config(wifi.as_ref()).await {
            Ok(nm) => {
                report.nm_wifi_powersave = Some(nm.wifi_powersave);
                report.nm_autoconnect_retries = Some(nm.autoconnect_retries);
                report.nm_cloned_mac = Some(nm.cloned_mac);
                report.nm_scan_rand_mac = Some(nm.scan_rand_mac);
            }
            Err(err) => {
                warn!("settings: NetworkManager config: {err:#}");
                report.read_errors.extend(NM_FIELDS);
            }
        }

        match tokio::fs::read_to_string(HOSTNAME_PATH).await {
            Ok(name) if !name.trim().is_empty() => report.hostname = Some(name.trim().to_owned()),
            Ok(_) => report.read_errors.push(DeviceSetting::Hostname),
            Err(err) => {
                warn!("settings: {HOSTNAME_PATH}: {err}");
                report.read_errors.push(DeviceSetting::Hostname);
            }
        }

        match self.connection().await {
            Ok(conn) => {
                match read_bgscan(&conn).await {
                    Ok(bgscan) => report.bgscan = bgscan,
                    Err(err) => {
                        warn!("settings: bgscan: {err:#}");
                        report.read_errors.push(DeviceSetting::Bgscan);
                    }
                }
                read_ntp(&conn, &mut report).await;
            }
            Err(err) => {
                warn!("settings: system D-Bus: {err:#}");
                self.conn = None;
                report.read_errors.push(DeviceSetting::Bgscan);
                report.read_errors.extend(NTP_FIELDS);
            }
        }

        Reading {
            report,
            vendor_present: driver.vendor_present,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgscan_is_unquoted() {
        assert_eq!(unquote("\"simple:30:-70:86400\""), "simple:30:-70:86400");
    }
}
