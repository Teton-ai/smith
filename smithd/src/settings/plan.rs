//! Decides what one apply does, from the wanted settings, what the device
//! runs, and the files smithd wrote earlier. Pure, so every rule is unit
//! tested; the executors in `apply` carry the plan out.

use super::driver::VENDOR_MODULE;
use super::nm::parse_nm_sections;
use crate::utils::schema::{
    DeviceSetting, MacRandomization, Managed, RoamTuning, SettingApplyState, SettingStatus,
    SettingsReport, WantedSettings,
};
use std::collections::HashMap;
use std::fmt::Debug;

/// A value still wrong after this many applies is reported failed and left
/// alone until a new version, a reboot or a Re-apply.
pub const MAX_ATTEMPTS: u8 = 2;
/// Driver reloads and NM re-activations allowed per version, the only steps
/// that take the device offline.
pub const MAX_DISRUPTIONS: u32 = 1;

const HEADER: &str = "# Managed by smithd, do not edit.\n";
/// Named sections without `match-device` apply to every device and are read
/// before the plain `[device]` / `[connection]` ones Ubuntu ships, so they win
/// whatever the file order.
const NM_DEVICE_SECTION: &str = "device-smith";
const NM_CONNECTION_SECTION: &str = "connection-smith";

/// One managed setting group, the unit an `ApplySettings` sets as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    WirelessMode,
    PowerMgnt,
    RoamFlags,
    RoamTuning,
    NmWifiPowersave,
    NmAutoconnectRetries,
    Mac,
    Hostname,
    NtpServers,
    Bgscan,
}

impl Group {
    pub fn settings(self) -> &'static [DeviceSetting] {
        use DeviceSetting as S;
        match self {
            Group::WirelessMode => &[S::WirelessMode],
            Group::PowerMgnt => &[S::PowerMgnt],
            Group::RoamFlags => &[S::RoamFlags],
            Group::RoamTuning => &[
                S::IdleDiff,
                S::BusyDiff,
                S::ScanrExpMs,
                S::ScanInt,
                S::IdleTh,
                S::BusyTh,
            ],
            Group::NmWifiPowersave => &[S::NmWifiPowersave],
            Group::NmAutoconnectRetries => &[S::NmAutoconnectRetries],
            Group::Mac => &[S::NmClonedMac, S::NmScanRandMac],
            Group::Hostname => &[S::Hostname],
            Group::NtpServers => &[S::NtpServers],
            Group::Bgscan => &[S::Bgscan],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NtpDaemon {
    Chrony,
    Timesyncd,
}

/// Current content of the files smithd owns; `None` when absent.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct OwnedFiles {
    pub driver: Option<String>,
    pub nm: Option<String>,
    /// The file of the running NTP daemon.
    pub ntp: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub enum FileChange {
    #[default]
    Keep,
    Write(String),
    Remove,
}

pub struct PlanInput<'a> {
    pub settings: &'a WantedSettings,
    /// The read this apply starts from.
    pub report: &'a SettingsReport,
    pub files: &'a OwnedFiles,
    pub ntp_daemon: Option<NtpDaemon>,
    /// wpa_supplicant has a current network, so bgscan can be set.
    pub wifi_connected: bool,
    /// Applies of each group so far for this version, kept in memory.
    pub attempts: &'a HashMap<Group, u8>,
    /// The version has disruptions left (`MAX_DISRUPTIONS`).
    pub disruption_allowed: bool,
}

/// Everything one apply does. Files are written whenever they differ from the
/// wanted set; the other steps run only for groups not yet running their
/// value. A driver reload also re-activates Wi-Fi, so it never comes with an
/// NM re-activation; the actor reloads NM before the driver so the
/// reconnection uses the new drop-in.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ApplyPlan {
    pub driver_file: FileChange,
    pub nm_file: FileChange,
    pub ntp_file: FileChange,
    pub driver_reload: bool,
    /// Written after a driver reload too, which resets them.
    pub roam_flags: Option<i16>,
    pub roam_tuning: Option<RoamTuning>,
    pub nm_reload: bool,
    pub nm_reactivate: bool,
    pub ntp_reload: bool,
    pub hostname: Option<String>,
    pub bgscan: Option<String>,
    /// Groups acted on; the actor counts one attempt for each.
    pub attempted: Vec<Group>,
    /// Groups already running their wanted value.
    pub in_sync: Vec<Group>,
    pub failed: Vec<SettingStatus>,
}

impl ApplyPlan {
    /// Whether the plan changes anything on the device.
    pub fn has_steps(&self) -> bool {
        self.driver_file != FileChange::Keep
            || self.nm_file != FileChange::Keep
            || self.ntp_file != FileChange::Keep
            || self.driver_reload
            || self.roam_flags.is_some()
            || self.roam_tuning.is_some()
            || self.nm_reload
            || self.nm_reactivate
            || self.ntp_reload
            || self.hostname.is_some()
            || self.bgscan.is_some()
    }
}

fn set<T>(wanted: &Option<Managed<T>>) -> Option<&T> {
    match wanted {
        Some(Managed::Set(value)) => Some(value),
        _ => None,
    }
}

/// A value in a file shared by several groups: absent leaves what the file
/// has, `Unmanaged` drops it.
fn merged<T: Clone>(wanted: &Option<Managed<T>>, current: Option<T>) -> Option<T> {
    match wanted {
        None => current,
        Some(Managed::Set(value)) => Some(value.clone()),
        Some(Managed::Unmanaged) => None,
    }
}

fn file_change(current: Option<&str>, wanted: Option<String>) -> FileChange {
    match (current, wanted) {
        (Some(current), Some(wanted)) if current == wanted => FileChange::Keep,
        (_, Some(wanted)) => FileChange::Write(wanted),
        (Some(_), None) => FileChange::Remove,
        (None, None) => FileChange::Keep,
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct DriverOptions {
    pub wireless_mode: Option<i16>,
    pub power_mgnt: Option<i16>,
}

pub fn parse_driver_file(text: &str) -> DriverOptions {
    let mut options = DriverOptions::default();
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if words.next() != Some("options") || words.next() != Some(VENDOR_MODULE) {
            continue;
        }
        for (key, value) in words.filter_map(|w| w.split_once('=')) {
            match key {
                "rtw_wireless_mode" => options.wireless_mode = value.parse().ok(),
                "rtw_power_mgnt" => options.power_mgnt = value.parse().ok(),
                _ => {}
            }
        }
    }
    options
}

/// `modprobe` reads `/etc/modprobe.d` in name order and the last value of a
/// parameter wins, so this file overrides older hand written ones.
pub fn render_driver_file(options: &DriverOptions) -> Option<String> {
    let mut params = String::new();
    if let Some(mode) = options.wireless_mode {
        params.push_str(&format!(" rtw_wireless_mode={mode}"));
    }
    if let Some(power) = options.power_mgnt {
        params.push_str(&format!(" rtw_power_mgnt={power}"));
    }
    (!params.is_empty()).then(|| format!("{HEADER}options {VENDOR_MODULE}{params}\n"))
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct NmOptions {
    pub wifi_powersave: Option<i16>,
    pub autoconnect_retries: Option<i32>,
    pub mac: Option<MacRandomization>,
}

pub fn parse_nm_file(text: &str) -> NmOptions {
    let (mut powersave, mut retries, mut cloned_mac, mut scan_rand_mac) = (None, None, None, None);
    for section in parse_nm_sections(text) {
        for (key, value) in &section.entries {
            match (section.name.as_str(), key.as_str()) {
                ("main", "autoconnect-retries-default") => retries = value.parse().ok(),
                (NM_DEVICE_SECTION, "wifi.scan-rand-mac-address") => {
                    scan_rand_mac = match value.as_str() {
                        "yes" => Some(true),
                        "no" => Some(false),
                        _ => None,
                    }
                }
                (NM_CONNECTION_SECTION, "wifi.powersave") => powersave = value.parse().ok(),
                (NM_CONNECTION_SECTION, "wifi.cloned-mac-address") => {
                    cloned_mac = Some(value.clone())
                }
                _ => {}
            }
        }
    }
    NmOptions {
        wifi_powersave: powersave,
        autoconnect_retries: retries,
        mac: cloned_mac
            .zip(scan_rand_mac)
            .map(|(cloned_mac, scan_rand_mac)| MacRandomization {
                cloned_mac,
                scan_rand_mac,
            }),
    }
}

pub fn render_nm_file(options: &NmOptions) -> Option<String> {
    let mut text = String::new();
    if let Some(retries) = options.autoconnect_retries {
        text.push_str(&format!(
            "\n[main]\nautoconnect-retries-default={retries}\n"
        ));
    }
    if let Some(mac) = &options.mac {
        let scan = if mac.scan_rand_mac { "yes" } else { "no" };
        text.push_str(&format!(
            "\n[{NM_DEVICE_SECTION}]\nwifi.scan-rand-mac-address={scan}\n"
        ));
    }
    if options.wifi_powersave.is_some() || options.mac.is_some() {
        text.push_str(&format!("\n[{NM_CONNECTION_SECTION}]\n"));
        if let Some(powersave) = options.wifi_powersave {
            text.push_str(&format!("wifi.powersave={powersave}\n"));
        }
        if let Some(mac) = &options.mac {
            text.push_str(&format!("wifi.cloned-mac-address={}\n", mac.cloned_mac));
        }
    }
    (!text.is_empty()).then(|| format!("{HEADER}{text}"))
}

/// chrony keeps the release's own pools next to these; timesyncd replaces its
/// `NTP=` list with this drop-in's.
pub fn render_ntp_file(daemon: NtpDaemon, servers: &[String]) -> String {
    match daemon {
        NtpDaemon::Chrony => servers.iter().fold(HEADER.to_owned(), |text, s| {
            text + &format!("server {s} iburst\n")
        }),
        NtpDaemon::Timesyncd => format!("{HEADER}[Time]\nNTP={}\n", servers.join(" ")),
    }
}

fn ntp_running(daemon: NtpDaemon, running: &[String], wanted: &[String]) -> bool {
    match daemon {
        NtpDaemon::Chrony => wanted.iter().all(|s| running.contains(s)),
        NtpDaemon::Timesyncd => running == wanted,
    }
}

/// `nm_wifi_powersave` 2 and 3 turn interface power save off and on; 0 and 1
/// leave it to the profile or the driver.
fn expected_power_save(powersave: i16) -> Option<bool> {
    match powersave {
        2 => Some(false),
        3 => Some(true),
        _ => None,
    }
}

fn roam_running(report: &SettingsReport, wanted: &RoamTuning) -> Option<bool> {
    let idle_diff = report.idle_diff?;
    // The busy fields exist only on drivers that report them.
    let busy = report.busy_diff.is_none()
        || (report.busy_diff == wanted.busy_diff && report.busy_th == wanted.busy_th);
    Some(
        busy && idle_diff == wanted.idle_diff
            && report.scanr_exp_ms == Some(wanted.scanr_exp_ms)
            && report.scan_int == Some(wanted.scan_int)
            && report.idle_th == Some(wanted.idle_th),
    )
}

struct Planner<'a> {
    input: &'a PlanInput<'a>,
    plan: ApplyPlan,
}

impl Planner<'_> {
    /// Whether to act on a group: not when it already runs its value, nor
    /// once its attempts or, for a step taking the device offline, the
    /// version's disruptions are spent (then it is reported failed, with
    /// `running` in the reason).
    fn act(&mut self, group: Group, needed: bool, running: &dyn Debug, disrupts: bool) -> bool {
        if !needed {
            self.plan.in_sync.push(group);
            return false;
        }
        let attempts = self.input.attempts.get(&group).copied().unwrap_or(0);
        let reason = if attempts >= MAX_ATTEMPTS {
            format!("still {running:?} after {attempts} attempts")
        } else if disrupts && !self.input.disruption_allowed {
            format!(
                "still {running:?}: needs a driver reload or Wi-Fi re-activation, already done for this version"
            )
        } else {
            self.plan.attempted.push(group);
            return true;
        };
        self.fail(group, &reason);
        false
    }

    fn fail(&mut self, group: Group, reason: &str) {
        self.plan
            .failed
            .extend(group.settings().iter().map(|setting| SettingStatus {
                setting: *setting,
                state: SettingApplyState::Failed,
                reason: reason.to_owned(),
            }));
    }

    fn driver(&mut self) {
        let wanted = self.input.settings;
        let report = self.input.report;
        let current = self
            .input
            .files
            .driver
            .as_deref()
            .map(parse_driver_file)
            .unwrap_or_default();
        let options = DriverOptions {
            wireless_mode: merged(&wanted.wireless_mode, current.wireless_mode),
            power_mgnt: merged(&wanted.power_mgnt, current.power_mgnt),
        };
        self.plan.driver_file = file_change(
            self.input.files.driver.as_deref(),
            render_driver_file(&options),
        );

        // Unmanaged does not reload: the release's value comes back at the
        // next boot.
        for (group, wanted, running) in [
            (
                Group::WirelessMode,
                set(&wanted.wireless_mode),
                report.wireless_mode,
            ),
            (Group::PowerMgnt, set(&wanted.power_mgnt), report.power_mgnt),
        ] {
            let (Some(wanted), Some(running)) = (wanted, running) else {
                continue;
            };
            if self.act(group, running != *wanted, &running, true) {
                self.plan.driver_reload = true;
            }
        }
    }

    /// Live `/proc` values: nothing to remove when unmanaged, and a reload
    /// resets them, so they are written after one whatever they read before.
    fn roam(&mut self) {
        let wanted = self.input.settings;
        let report = self.input.report;
        let reload = self.plan.driver_reload;
        if let Some(flags) = set(&wanted.roam_flags) {
            if reload {
                self.plan.roam_flags = Some(*flags);
            } else if let Some(running) = report.roam_flags
                && self.act(Group::RoamFlags, running != *flags, &running, false)
            {
                self.plan.roam_flags = Some(*flags);
            }
        }
        if let Some(tuning) = set(&wanted.roam_tuning) {
            if reload {
                self.plan.roam_tuning = Some(tuning.clone());
            } else if let Some(running) = roam_running(report, tuning)
                && self.act(Group::RoamTuning, !running, &report.idle_diff, false)
            {
                self.plan.roam_tuning = Some(tuning.clone());
            }
        }
    }

    fn nm(&mut self) {
        let wanted = self.input.settings;
        let report = self.input.report;
        let current = self
            .input
            .files
            .nm
            .as_deref()
            .map(parse_nm_file)
            .unwrap_or_default();
        let options = NmOptions {
            wifi_powersave: merged(&wanted.nm_wifi_powersave, current.wifi_powersave),
            autoconnect_retries: merged(
                &wanted.nm_autoconnect_retries,
                current.autoconnect_retries,
            ),
            mac: merged(&wanted.mac, current.mac.clone()),
        };
        self.plan.nm_file = file_change(self.input.files.nm.as_deref(), render_nm_file(&options));
        let mut reactivate = false;
        let mut reload = self.plan.nm_file != FileChange::Keep;

        // The config values come from `NetworkManager --print-config`, which
        // reads the files, so they differ only when another file overrides
        // ours; power save is also read from the interface, which shows
        // whether the running connection picked it up.
        if let Some(powersave) = set(&wanted.nm_wifi_powersave) {
            let running = report.wifi_power_save_on;
            let not_running = self.input.wifi_connected
                && expected_power_save(*powersave)
                    .zip(running)
                    .is_some_and(|(expected, on)| expected != on);
            let needed = current.wifi_powersave != Some(*powersave)
                || report.nm_wifi_powersave.is_some_and(|v| v != *powersave)
                || not_running;
            if self.act(Group::NmWifiPowersave, needed, &running, true) {
                reactivate = true;
            }
        }
        if let Some(mac) = set(&wanted.mac) {
            let running = (report.nm_cloned_mac.clone(), report.nm_scan_rand_mac);
            let needed = current.mac.as_ref() != Some(mac)
                || running.0.as_ref().is_some_and(|v| *v != mac.cloned_mac)
                || running.1.is_some_and(|v| v != mac.scan_rand_mac);
            if self.act(Group::Mac, needed, &running, true) {
                reactivate = true;
            }
        }
        // Read when a connection retries, so a reload is enough.
        if let Some(retries) = set(&wanted.nm_autoconnect_retries) {
            let running = report.nm_autoconnect_retries;
            let needed = current.autoconnect_retries != Some(*retries)
                || running.is_some_and(|v| v != *retries);
            if self.act(Group::NmAutoconnectRetries, needed, &running, false) {
                reload = true;
            }
        }
        self.plan.nm_reload = reload || reactivate;
        self.plan.nm_reactivate = reactivate && !self.plan.driver_reload;
    }

    fn ntp(&mut self) {
        let Some(wanted) = &self.input.settings.ntp_servers else {
            return;
        };
        let current = self.input.files.ntp.as_deref();
        // Nothing is tried without a daemon, so no attempt is spent: a daemon
        // that appears later (chrony config fixed, package installed) is
        // applied on the next check.
        let Some(daemon) = self.input.ntp_daemon else {
            if let Managed::Set(_) = wanted {
                self.fail(Group::NtpServers, "no NTP daemon");
            }
            return;
        };
        match wanted {
            Managed::Unmanaged => {
                self.plan.ntp_file = file_change(current, None);
                self.plan.ntp_reload = self.plan.ntp_file == FileChange::Remove;
            }
            Managed::Set(servers) => {
                self.plan.ntp_file = file_change(current, Some(render_ntp_file(daemon, servers)));
                let running = &self.input.report.ntp_servers;
                let needed = self.plan.ntp_file != FileChange::Keep
                    || running
                        .as_ref()
                        .is_some_and(|r| !ntp_running(daemon, r, servers));
                self.plan.ntp_reload = self.act(Group::NtpServers, needed, running, false);
            }
        }
    }

    /// Live only: an unmanaged hostname or bgscan stays as it is.
    fn hostname_and_bgscan(&mut self) {
        let wanted = self.input.settings;
        let report = self.input.report;
        if let Some(hostname) = set(&wanted.hostname) {
            let running = &report.hostname;
            if self.act(
                Group::Hostname,
                running.as_ref() != Some(hostname),
                running,
                false,
            ) {
                self.plan.hostname = Some(hostname.clone());
            }
        }
        // Nothing to set it on while Wi-Fi is not connected.
        if let Some(bgscan) = set(&wanted.bgscan)
            && self.input.wifi_connected
        {
            let running = &report.bgscan;
            if self.act(
                Group::Bgscan,
                running.as_ref() != Some(bgscan),
                running,
                false,
            ) {
                self.plan.bgscan = Some(bgscan.clone());
            }
        }
    }
}

pub fn plan(input: &PlanInput) -> ApplyPlan {
    let mut planner = Planner {
        input,
        plan: ApplyPlan::default(),
    };
    planner.driver();
    planner.roam();
    planner.nm();
    planner.ntp();
    planner.hostname_and_bgscan();
    planner.plan
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHRONY: Option<NtpDaemon> = Some(NtpDaemon::Chrony);

    /// A Wi-Fi connected L4T 36 device running the vendor defaults.
    fn report() -> SettingsReport {
        SettingsReport {
            wireless_mode: Some(95),
            power_mgnt: Some(2),
            roam_flags: Some(7),
            idle_diff: Some(5),
            busy_diff: Some(10),
            scanr_exp_ms: Some(10000),
            scan_int: Some(2),
            idle_th: Some(40),
            busy_th: Some(40),
            nm_wifi_powersave: Some(3),
            nm_autoconnect_retries: Some(4),
            nm_cloned_mac: Some("preserve".to_owned()),
            nm_scan_rand_mac: Some(false),
            wifi_power_save_on: Some(true),
            bgscan: Some("simple:30:-70:86400".to_owned()),
            hostname: Some("orin-1".to_owned()),
            ntp_servers: Some(vec!["ntp.ubuntu.com".to_owned()]),
            ..Default::default()
        }
    }

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

    struct Case {
        settings: WantedSettings,
        report: SettingsReport,
        files: OwnedFiles,
        ntp_daemon: Option<NtpDaemon>,
        wifi_connected: bool,
        attempts: HashMap<Group, u8>,
        disruption_allowed: bool,
    }

    impl Case {
        fn new(settings: WantedSettings) -> Self {
            Self {
                settings,
                report: report(),
                files: OwnedFiles::default(),
                ntp_daemon: CHRONY,
                wifi_connected: true,
                attempts: HashMap::new(),
                disruption_allowed: true,
            }
        }

        fn plan(&self) -> ApplyPlan {
            plan(&PlanInput {
                settings: &self.settings,
                report: &self.report,
                files: &self.files,
                ntp_daemon: self.ntp_daemon,
                wifi_connected: self.wifi_connected,
                attempts: &self.attempts,
                disruption_allowed: self.disruption_allowed,
            })
        }
    }

    fn written(change: &FileChange) -> &str {
        match change {
            FileChange::Write(text) => text,
            other => panic!("expected a write, got {other:?}"),
        }
    }

    #[test]
    fn nothing_wanted_changes_nothing() {
        assert_eq!(
            Case::new(WantedSettings::default()).plan(),
            ApplyPlan::default()
        );
    }

    #[test]
    fn a_driver_value_change_writes_the_file_and_reloads_once() {
        let plan = Case::new(WantedSettings {
            wireless_mode: Some(Managed::Set(11)),
            power_mgnt: Some(Managed::Set(0)),
            ..Default::default()
        })
        .plan();
        assert_eq!(
            written(&plan.driver_file),
            format!("{HEADER}options rtl8822ce rtw_wireless_mode=11 rtw_power_mgnt=0\n")
        );
        assert!(plan.driver_reload);
        assert_eq!(plan.attempted, vec![Group::WirelessMode, Group::PowerMgnt]);
    }

    #[test]
    fn an_unmanaged_driver_value_drops_its_line_keeps_absent_ones_and_never_reloads() {
        let mut case = Case::new(WantedSettings {
            power_mgnt: Some(Managed::Unmanaged),
            ..Default::default()
        });
        case.files.driver = render_driver_file(&DriverOptions {
            wireless_mode: Some(11),
            power_mgnt: Some(0),
        });
        let plan = case.plan();
        assert_eq!(
            written(&plan.driver_file),
            format!("{HEADER}options rtl8822ce rtw_wireless_mode=11\n")
        );
        assert!(!plan.driver_reload);
    }

    #[test]
    fn a_driver_file_left_without_values_is_removed() {
        let mut case = Case::new(WantedSettings {
            wireless_mode: Some(Managed::Unmanaged),
            power_mgnt: Some(Managed::Unmanaged),
            ..Default::default()
        });
        case.files.driver = render_driver_file(&DriverOptions {
            wireless_mode: Some(11),
            power_mgnt: Some(0),
        });
        assert_eq!(case.plan().driver_file, FileChange::Remove);
    }

    #[test]
    fn a_running_driver_value_needs_no_reload() {
        let mut case = Case::new(WantedSettings {
            wireless_mode: Some(Managed::Set(95)),
            ..Default::default()
        });
        case.files.driver = render_driver_file(&DriverOptions {
            wireless_mode: Some(95),
            power_mgnt: None,
        });
        let plan = case.plan();
        assert_eq!(plan.driver_file, FileChange::Keep);
        assert!(!plan.driver_reload);
        assert_eq!(plan.in_sync, vec![Group::WirelessMode]);
    }

    #[test]
    fn an_unreadable_driver_value_writes_the_file_but_does_not_reload() {
        let mut case = Case::new(WantedSettings {
            wireless_mode: Some(Managed::Set(11)),
            ..Default::default()
        });
        case.report.wireless_mode = None;
        let plan = case.plan();
        assert!(matches!(plan.driver_file, FileChange::Write(_)));
        assert!(!plan.driver_reload);
        assert!(plan.attempted.is_empty() && plan.failed.is_empty());
    }

    #[test]
    fn spent_disruptions_report_failed_instead_of_reloading() {
        let mut case = Case::new(WantedSettings {
            wireless_mode: Some(Managed::Set(11)),
            ..Default::default()
        });
        case.disruption_allowed = false;
        let plan = case.plan();
        assert!(!plan.driver_reload);
        assert_eq!(plan.failed.len(), 1);
        assert_eq!(plan.failed[0].setting, DeviceSetting::WirelessMode);
        assert_eq!(plan.failed[0].state, SettingApplyState::Failed);
        assert!(
            plan.failed[0]
                .reason
                .contains("already done for this version")
        );
    }

    #[test]
    fn a_group_is_reported_failed_once_its_attempts_are_spent() {
        let mut case = Case::new(WantedSettings {
            bgscan: Some(Managed::Set("simple:30:-65:300".to_owned())),
            ..Default::default()
        });
        case.attempts.insert(Group::Bgscan, MAX_ATTEMPTS - 1);
        assert!(case.plan().bgscan.is_some(), "one attempt left");

        case.attempts.insert(Group::Bgscan, MAX_ATTEMPTS);
        let plan = case.plan();
        assert_eq!(plan.bgscan, None);
        assert_eq!(plan.failed.len(), 1);
        assert_eq!(plan.failed[0].setting, DeviceSetting::Bgscan);
        assert!(plan.failed[0].reason.contains("after 2 attempts"));
    }

    #[test]
    fn roam_values_are_written_only_when_they_differ() {
        let plan = Case::new(WantedSettings {
            roam_flags: Some(Managed::Set(7)),
            roam_tuning: Some(Managed::Set(tuning())),
            ..Default::default()
        })
        .plan();
        assert_eq!(plan.roam_flags, None);
        assert_eq!(plan.roam_tuning, Some(tuning()));
        assert_eq!(plan.in_sync, vec![Group::RoamFlags]);
        assert_eq!(plan.attempted, vec![Group::RoamTuning]);
    }

    #[test]
    fn roam_values_are_written_after_a_reload_without_counting_an_attempt() {
        let plan = Case::new(WantedSettings {
            wireless_mode: Some(Managed::Set(11)),
            roam_flags: Some(Managed::Set(7)),
            ..Default::default()
        })
        .plan();
        assert!(plan.driver_reload);
        assert_eq!(plan.roam_flags, Some(7), "a reload resets them");
        assert_eq!(plan.attempted, vec![Group::WirelessMode]);
    }

    #[test]
    fn roam_tuning_ignores_busy_fields_on_drivers_without_them() {
        let mut case = Case::new(WantedSettings {
            roam_tuning: Some(Managed::Set(RoamTuning {
                busy_diff: None,
                busy_th: None,
                ..tuning()
            })),
            ..Default::default()
        });
        case.report = SettingsReport {
            idle_diff: Some(15),
            busy_diff: None,
            scanr_exp_ms: Some(10000),
            scan_int: Some(5),
            idle_th: Some(30),
            busy_th: None,
            ..report()
        };
        assert_eq!(case.plan().in_sync, vec![Group::RoamTuning]);
    }

    #[test]
    fn nm_power_save_and_mac_reload_and_reactivate() {
        let mac = MacRandomization {
            cloned_mac: "permanent".to_owned(),
            scan_rand_mac: false,
        };
        let plan = Case::new(WantedSettings {
            nm_wifi_powersave: Some(Managed::Set(2)),
            mac: Some(Managed::Set(mac)),
            ..Default::default()
        })
        .plan();
        assert_eq!(
            written(&plan.nm_file),
            format!(
                "{HEADER}\n[device-smith]\nwifi.scan-rand-mac-address=no\n\n\
                 [connection-smith]\nwifi.powersave=2\nwifi.cloned-mac-address=permanent\n"
            )
        );
        assert!(plan.nm_reload && plan.nm_reactivate);
    }

    #[test]
    fn nm_autoconnect_retries_only_reloads() {
        let plan = Case::new(WantedSettings {
            nm_autoconnect_retries: Some(Managed::Set(0)),
            ..Default::default()
        })
        .plan();
        assert_eq!(
            written(&plan.nm_file),
            format!("{HEADER}\n[main]\nautoconnect-retries-default=0\n")
        );
        assert!(plan.nm_reload);
        assert!(!plan.nm_reactivate && !plan.driver_reload);
    }

    fn power_save_in_file() -> Case {
        let mut case = Case::new(WantedSettings {
            nm_wifi_powersave: Some(Managed::Set(2)),
            ..Default::default()
        });
        case.files.nm = render_nm_file(&NmOptions {
            wifi_powersave: Some(2),
            ..Default::default()
        });
        case.report.nm_wifi_powersave = Some(2);
        case
    }

    #[test]
    fn nm_power_save_still_on_the_interface_reactivates_with_the_file_unchanged() {
        let plan = power_save_in_file().plan();
        assert_eq!(plan.nm_file, FileChange::Keep);
        assert!(plan.nm_reactivate);
    }

    #[test]
    fn nm_power_save_off_on_the_interface_is_in_sync() {
        let mut case = power_save_in_file();
        case.report.wifi_power_save_on = Some(false);
        let plan = case.plan();
        assert!(!plan.nm_reload && !plan.nm_reactivate);
        assert_eq!(plan.in_sync, vec![Group::NmWifiPowersave]);
    }

    #[test]
    fn a_driver_reload_stands_in_for_the_nm_reactivation() {
        let plan = Case::new(WantedSettings {
            wireless_mode: Some(Managed::Set(11)),
            nm_wifi_powersave: Some(Managed::Set(2)),
            ..Default::default()
        })
        .plan();
        assert!(plan.driver_reload && plan.nm_reload);
        assert!(!plan.nm_reactivate);
    }

    fn every_nm_option() -> NmOptions {
        NmOptions {
            wifi_powersave: Some(2),
            autoconnect_retries: Some(0),
            mac: Some(MacRandomization {
                cloned_mac: "stable".to_owned(),
                scan_rand_mac: true,
            }),
        }
    }

    #[test]
    fn nm_file_parses_its_own_rendering() {
        let text = render_nm_file(&every_nm_option());
        assert_eq!(text.as_deref().map(parse_nm_file), Some(every_nm_option()));
    }

    #[test]
    fn an_nm_file_left_without_values_is_removed_without_reactivating() {
        let mut case = Case::new(WantedSettings {
            nm_wifi_powersave: Some(Managed::Unmanaged),
            nm_autoconnect_retries: Some(Managed::Unmanaged),
            mac: Some(Managed::Unmanaged),
            ..Default::default()
        });
        case.files.nm = render_nm_file(&every_nm_option());
        let plan = case.plan();
        assert_eq!(plan.nm_file, FileChange::Remove);
        assert!(plan.nm_reload);
        assert!(!plan.nm_reactivate);
    }

    fn servers() -> Vec<String> {
        vec!["10.122.0.1".to_owned(), "0.pool.ntp.org".to_owned()]
    }

    fn ntp_case() -> Case {
        Case::new(WantedSettings {
            ntp_servers: Some(Managed::Set(servers())),
            ..Default::default()
        })
    }

    #[test]
    fn new_ntp_servers_write_the_chrony_file_and_reload() {
        let plan = ntp_case().plan();
        assert_eq!(
            written(&plan.ntp_file),
            format!("{HEADER}server 10.122.0.1 iburst\nserver 0.pool.ntp.org iburst\n")
        );
        assert!(plan.ntp_reload);
    }

    #[test]
    fn chrony_is_in_sync_when_it_runs_ours_next_to_the_release_pools() {
        let mut case = ntp_case();
        case.files.ntp = Some(render_ntp_file(NtpDaemon::Chrony, &servers()));
        case.report.ntp_servers = Some(vec![
            "ntp.ubuntu.com".to_owned(),
            "10.122.0.1".to_owned(),
            "0.pool.ntp.org".to_owned(),
        ]);
        let plan = case.plan();
        assert!(!plan.ntp_reload);
        assert_eq!(plan.in_sync, vec![Group::NtpServers]);
    }

    #[test]
    fn timesyncd_not_running_exactly_ours_reloads_with_the_file_unchanged() {
        let mut case = ntp_case();
        case.ntp_daemon = Some(NtpDaemon::Timesyncd);
        case.files.ntp = Some(format!("{HEADER}[Time]\nNTP=10.122.0.1 0.pool.ntp.org\n"));
        let plan = case.plan();
        assert_eq!(plan.ntp_file, FileChange::Keep);
        assert!(plan.ntp_reload);
    }

    #[test]
    fn unmanaged_ntp_without_a_file_does_nothing() {
        let plan = Case::new(WantedSettings {
            ntp_servers: Some(Managed::Unmanaged),
            ..Default::default()
        })
        .plan();
        assert_eq!(plan, ApplyPlan::default());
    }

    #[test]
    fn unmanaged_ntp_removes_its_file_and_reloads() {
        let mut case = Case::new(WantedSettings {
            ntp_servers: Some(Managed::Unmanaged),
            ..Default::default()
        });
        case.files.ntp = Some(render_ntp_file(NtpDaemon::Chrony, &servers()));
        let plan = case.plan();
        assert_eq!(plan.ntp_file, FileChange::Remove);
        assert!(plan.ntp_reload);
    }

    #[test]
    fn ntp_without_a_daemon_fails_without_an_attempt() {
        let mut case = ntp_case();
        case.ntp_daemon = None;
        let plan = case.plan();
        assert_eq!(plan.ntp_file, FileChange::Keep);
        assert!(!plan.ntp_reload);
        assert!(plan.attempted.is_empty());
        assert_eq!(
            plan.failed,
            [SettingStatus {
                setting: DeviceSetting::NtpServers,
                state: SettingApplyState::Failed,
                reason: "no NTP daemon".to_owned(),
            }]
        );
    }

    #[test]
    fn a_different_hostname_and_bgscan_are_set_live() {
        let plan = Case::new(WantedSettings {
            hostname: Some(Managed::Set("byag-12".to_owned())),
            bgscan: Some(Managed::Set("simple:30:-65:300".to_owned())),
            ..Default::default()
        })
        .plan();
        assert_eq!(plan.hostname.as_deref(), Some("byag-12"));
        assert_eq!(plan.bgscan.as_deref(), Some("simple:30:-65:300"));
    }

    #[test]
    fn bgscan_waits_for_a_wifi_connection() {
        let mut case = Case::new(WantedSettings {
            bgscan: Some(Managed::Set("simple:30:-65:300".to_owned())),
            ..Default::default()
        });
        case.wifi_connected = false;
        assert_eq!(case.plan(), ApplyPlan::default());
    }

    #[test]
    fn driver_file_parses_its_own_rendering() {
        let options = DriverOptions {
            wireless_mode: Some(95),
            power_mgnt: Some(0),
        };
        let text = render_driver_file(&options).unwrap();
        assert_eq!(parse_driver_file(&text), options);
    }

    #[test]
    fn driver_file_ignores_options_of_other_modules() {
        assert_eq!(
            parse_driver_file("options other_mod rtw_wireless_mode=3\n"),
            DriverOptions::default()
        );
    }
}
