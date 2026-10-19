//! Time sync: systemd-timesyncd over D-Bus, or chrony (orin-nano-rcmptr) from its
//! config and `chronyc tracking`; synchronized status from timedated on both.

use super::read::run;
use crate::utils::schema::{DeviceSetting, SettingsReport};
use anyhow::{Context, Result, anyhow};
use std::path::Path;
use tracing::warn;
use zbus::Connection;
use zbus::proxy::CacheProperties;

const TIMESYNC_BUS_NAME: &str = "org.freedesktop.timesync1";

const CHRONY_CONF: &str = "/etc/chrony/chrony.conf";
const CHRONY_CONF_DIR: &str = "/etc/chrony/conf.d";
const CHRONY_SOURCES_DIR: &str = "/etc/chrony/sources.d";
/// Servers chrony learned from DHCP.
const CHRONY_DHCP_DIR: &str = "/run/chrony-dhcp";

pub const NTP_FIELDS: [DeviceSetting; 5] = [
    DeviceSetting::NtpServers,
    DeviceSetting::NtpLinkServers,
    DeviceSetting::NtpFallbackServers,
    DeviceSetting::NtpServerInUse,
    DeviceSetting::NtpSynchronized,
];

/// Everything except `ntp_synchronized`, which comes from timedated.
const NTP_SERVER_FIELDS: [DeviceSetting; 4] = [
    DeviceSetting::NtpServers,
    DeviceSetting::NtpLinkServers,
    DeviceSetting::NtpFallbackServers,
    DeviceSetting::NtpServerInUse,
];

#[zbus::proxy(
    interface = "org.freedesktop.timesync1.Manager",
    default_service = "org.freedesktop.timesync1",
    default_path = "/org/freedesktop/timesync1"
)]
trait Timesync {
    #[zbus(property, name = "SystemNTPServers")]
    fn system_ntp_servers(&self) -> zbus::Result<Vec<String>>;
    #[zbus(property, name = "LinkNTPServers")]
    fn link_ntp_servers(&self) -> zbus::Result<Vec<String>>;
    #[zbus(property, name = "FallbackNTPServers")]
    fn fallback_ntp_servers(&self) -> zbus::Result<Vec<String>>;
    #[zbus(property)]
    fn server_name(&self) -> zbus::Result<String>;
}

#[zbus::proxy(
    interface = "org.freedesktop.timedate1",
    default_service = "org.freedesktop.timedate1",
    default_path = "/org/freedesktop/timedate1"
)]
trait Timedate {
    #[zbus(property, name = "NTPSynchronized")]
    fn ntp_synchronized(&self) -> zbus::Result<bool>;
}

/// Server names from chrony `server` / `pool` lines; other directives are
/// ignored.
pub fn parse_chrony_sources(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#') && !line.starts_with('!'))
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            match words.next() {
                Some("server" | "pool") => words.next().map(str::to_owned),
                _ => None,
            }
        })
        .collect()
}

/// Address from the `Reference ID` line of `chronyc -n tracking`; `None` when
/// chrony has no reference yet (empty parentheses).
pub fn parse_chrony_tracking(text: &str) -> Result<Option<String>> {
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("Reference ID"))
        .ok_or_else(|| anyhow!("no Reference ID in chronyc tracking"))?;
    let inner = line
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(inner, _)| inner.trim())
        .ok_or_else(|| anyhow!("unexpected Reference ID line {line:?}"))?;
    Ok(Some(inner.to_owned()).filter(|s| !s.is_empty()))
}

async fn read_sources_dir(dir: &str, extension: &str) -> Vec<String> {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return Vec::new();
    };
    let mut files = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == extension) {
            files.push(path);
        }
    }
    files.sort();
    let mut servers = Vec::new();
    for file in files {
        match tokio::fs::read_to_string(&file).await {
            Ok(text) => servers.extend(parse_chrony_sources(&text)),
            Err(err) => warn!("settings: reading {}: {err}", file.display()),
        }
    }
    servers
}

async fn read_timesyncd(conn: &Connection, report: &mut SettingsReport) -> Result<()> {
    let timesync = TimesyncProxy::builder(conn)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    report.ntp_servers = Some(timesync.system_ntp_servers().await?);
    report.ntp_link_servers = Some(timesync.link_ntp_servers().await?);
    report.ntp_fallback_servers = Some(timesync.fallback_ntp_servers().await?);
    report.ntp_server_in_use = Some(timesync.server_name().await?).filter(|s| !s.is_empty());
    Ok(())
}

async fn read_synchronized(conn: &Connection) -> Result<bool> {
    let timedate = TimedateProxy::builder(conn)
        .cache_properties(CacheProperties::No)
        .build()
        .await?;
    Ok(timedate.ntp_synchronized().await?)
}

/// NameHasOwner never bus-activates timesyncd, which must not start next to
/// chrony.
async fn timesyncd_running(conn: &Connection) -> Result<bool> {
    let dbus = zbus::fdo::DBusProxy::new(conn).await?;
    let name = zbus::names::BusName::try_from(TIMESYNC_BUS_NAME)?;
    Ok(dbus.name_has_owner(name).await?)
}

pub async fn read_ntp(conn: &Connection, report: &mut SettingsReport) {
    match read_synchronized(conn).await {
        Ok(synced) => report.ntp_synchronized = Some(synced),
        Err(err) => {
            warn!("settings: NTPSynchronized: {err:#}");
            report.read_errors.push(DeviceSetting::NtpSynchronized);
        }
    }

    let servers = match timesyncd_running(conn).await {
        Ok(true) => read_timesyncd(conn, report).await,
        Ok(false) => read_chrony(report).await,
        Err(err) => Err(err.context("NameHasOwner")),
    };
    if let Err(err) = servers {
        warn!("settings: NTP servers: {err:#}");
        report.read_errors.extend(NTP_SERVER_FIELDS);
    }
}

async fn read_chrony(report: &mut SettingsReport) -> Result<()> {
    let in_use = parse_chrony_tracking(&run("chronyc", &["-n", "tracking"]).await?)?;
    let conf = tokio::fs::read_to_string(Path::new(CHRONY_CONF))
        .await
        .with_context(|| format!("reading {CHRONY_CONF}"))?;
    let mut servers = parse_chrony_sources(&conf);
    servers.extend(read_sources_dir(CHRONY_CONF_DIR, "conf").await);
    servers.extend(read_sources_dir(CHRONY_SOURCES_DIR, "sources").await);
    report.ntp_servers = Some(servers);
    report.ntp_link_servers = Some(read_sources_dir(CHRONY_DHCP_DIR, "sources").await);
    report.ntp_server_in_use = in_use;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrony_fixtures() {
        let conf = include_str!("../utils/fixtures/settings/chrony_conf_platform_c.txt");
        assert_eq!(
            parse_chrony_sources(conf),
            vec![
                "ntp.ubuntu.com",
                "0.ubuntu.pool.ntp.org",
                "1.ubuntu.pool.ntp.org",
                "2.ubuntu.pool.ntp.org"
            ]
        );
        let cameras =
            include_str!("../utils/fixtures/settings/chrony_conf_d_cameras_platform_c.txt");
        assert!(parse_chrony_sources(cameras).is_empty());

        let tracking = include_str!("../utils/fixtures/settings/chronyc_tracking_platform_c.txt");
        assert_eq!(
            parse_chrony_tracking(tracking).unwrap().as_deref(),
            Some("80.248.138.19")
        );
        assert_eq!(
            parse_chrony_tracking("Reference ID    : 00000000 ()\n").unwrap(),
            None
        );
    }
}
