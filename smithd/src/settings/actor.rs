use super::READ_TIMEOUT;
use super::apply::DeviceSystem;
use super::plan::{
    ApplyPlan, FileChange, Group, MAX_DISRUPTIONS, NtpDaemon, OwnedFiles, PlanInput, plan,
};
use super::read::Reading;
use crate::shutdown::ShutdownSignals;
use crate::utils::files::{
    SettingsState, StoredSettings, load_settings_state, load_wanted_settings,
};
use crate::utils::schema::{
    DeviceSetting, RoamTuning, SafeCommandResponse, SafeCommandRx, SettingApplyState,
    SettingStatus, SettingsReport, WantedSettings,
};
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::future::Future;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{self, Duration, Instant};
use tracing::{error, info, warn};

/// Synthetic command id for reports smithd sends without being asked.
const CMD_ID_REPORT_SETTINGS: i32 = -7;

/// Fixed for now; clamped to the bounds below once it becomes configurable.
const CHECK_INTERVAL_S: u64 = 60;
const MIN_CHECK_INTERVAL_S: u64 = 10;
const MAX_CHECK_INTERVAL_S: u64 = 60 * 60;
/// Reports are not acknowledged, so this bounds how long a lost one leaves the
/// api stale.
const FULL_REPORT_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

fn check_interval() -> Duration {
    Duration::from_secs(CHECK_INTERVAL_S.clamp(MIN_CHECK_INTERVAL_S, MAX_CHECK_INTERVAL_S))
}

pub fn check_interval_s() -> i32 {
    i32::try_from(check_interval().as_secs()).unwrap_or(i32::MAX)
}

/// The part of a report that counts as a change. The NTP server in use moves on
/// its own (chrony hops between pool servers), so it is sent with every report
/// but never triggers one. The sync flag does count: the first report after a
/// boot is usually taken before the first sync and would otherwise stay
/// unsynchronized until the daily report.
fn change_key(report: &SettingsReport) -> SettingsReport {
    SettingsReport {
        ntp_server_in_use: None,
        ..report.clone()
    }
}

pub enum ActorMessage {
    CheckNow,
    Apply {
        version: i32,
        settings: WantedSettings,
        reply: oneshot::Sender<SafeCommandRx>,
    },
}

fn should_send(
    current: &SettingsReport,
    last_sent: Option<&SettingsReport>,
    since_last_sent: Duration,
) -> bool {
    last_sent.map(change_key) != Some(change_key(current)) || since_last_sent >= FULL_REPORT_EVERY
}

/// A driver reload makes the vendor interface vanish for a few seconds; skip
/// the first check where it is gone so a reload doesn't report a burst of read
/// errors. If it is still gone on the next check, that one is reported.
fn hold_for_reload(previous_vendor_present: Option<bool>, vendor_present: bool) -> bool {
    previous_vendor_present == Some(true) && !vendor_present
}

/// The state for the stored version: a record of another version, or none,
/// starts fresh. `None` when the file could not be read: applying could then
/// repeat a reload or re-apply a reverted version.
fn initial_state(
    wanted: Option<&StoredSettings>,
    loaded: Result<Option<SettingsState>>,
) -> Option<SettingsState> {
    let version = wanted.map_or(0, |w| w.version);
    match loaded {
        Ok(Some(state)) if state.version == version => Some(state),
        Ok(_) => Some(SettingsState {
            version,
            ..Default::default()
        }),
        Err(err) => {
            error!("Settings state unreadable, not applying until the next ApplySettings: {err:#}");
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedFile {
    Driver,
    Nm,
    Ntp(NtpDaemon),
}

/// One side effect of an apply, in the order `Actor::execute` runs them.
#[derive(Debug, Clone, Copy)]
pub enum Step<'a> {
    File(OwnedFile, &'a FileChange),
    ReloadNm,
    ReloadDriver,
    RoamFlags(i16),
    RoamTuning(&'a RoamTuning, bool),
    ReactivateWifi,
    ReloadNtp(NtpDaemon),
    Hostname(&'a str),
    Bgscan(&'a str),
}

impl Step<'_> {
    /// The groups a failure of this step is reported on.
    fn groups(&self) -> &'static [Group] {
        match self {
            Step::File(OwnedFile::Driver, _) | Step::ReloadDriver => {
                &[Group::WirelessMode, Group::PowerMgnt]
            }
            Step::File(OwnedFile::Nm, _) | Step::ReloadNm | Step::ReactivateWifi => &[
                Group::NmWifiPowersave,
                Group::Mac,
                Group::NmAutoconnectRetries,
            ],
            Step::File(OwnedFile::Ntp(_), _) | Step::ReloadNtp(_) => &[Group::NtpServers],
            Step::RoamFlags(_) => &[Group::RoamFlags],
            Step::RoamTuning(..) => &[Group::RoamTuning],
            Step::Hostname(_) => &[Group::Hostname],
            Step::Bgscan(_) => &[Group::Bgscan],
        }
    }
}

/// Everything the actor does to the device or outside itself, so tests can run
/// it against a fake device.
pub trait System: Send {
    fn read(&mut self) -> impl Future<Output = Reading> + Send;
    fn ntp_daemon(&mut self) -> impl Future<Output = Result<Option<NtpDaemon>>> + Send;
    fn owned_files(
        &mut self,
        ntp: Option<NtpDaemon>,
    ) -> impl Future<Output = Result<OwnedFiles>> + Send;
    fn run(&mut self, step: Step<'_>) -> impl Future<Output = Result<()>> + Send;
    fn save_wanted(&mut self, wanted: &StoredSettings) -> impl Future<Output = Result<()>> + Send;
    fn save_state(&mut self, state: &SettingsState) -> impl Future<Output = Result<()>> + Send;
    fn hold_police(&mut self, hold: bool) -> impl Future<Output = ()> + Send;
    fn queue_report(&mut self, response: SafeCommandResponse) -> impl Future<Output = ()> + Send;
}

pub struct Actor<S> {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<ActorMessage>,
    system: S,
    last_sent: Option<SettingsReport>,
    last_sent_at: Instant,
    previous_vendor_present: Option<bool>,
    wanted: Option<StoredSettings>,
    /// `None` while the state file is unreadable: nothing is applied then.
    state: Option<SettingsState>,
    /// Applies of each group for the stored version; reset on a restart.
    attempts: HashMap<Group, u8>,
    /// Settings put back by local enforcement since the last report.
    corrected: Vec<DeviceSetting>,
}

impl Actor<DeviceSystem> {
    pub async fn load(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<ActorMessage>,
        system: DeviceSystem,
    ) -> Self {
        let wanted = load_wanted_settings().await;
        let state = initial_state(wanted.as_ref(), load_settings_state().await);
        Self::new(shutdown, receiver, system, wanted, state)
    }
}

impl<S: System> Actor<S> {
    pub fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<ActorMessage>,
        system: S,
        wanted: Option<StoredSettings>,
        state: Option<SettingsState>,
    ) -> Self {
        Self {
            shutdown,
            receiver,
            system,
            last_sent: None,
            last_sent_at: Instant::now(),
            previous_vendor_present: None,
            wanted,
            state,
            attempts: HashMap::new(),
            corrected: Vec::new(),
        }
    }

    pub async fn run(&mut self) {
        info!("Settings reporter starting");

        // The first tick fires at once, so a stored set is applied at boot
        // without contacting the api.
        let mut interval = time::interval(check_interval());
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = self.shutdown.token.cancelled() => break,
                _ = interval.tick() => self.check().await,
                Some(message) = self.receiver.recv() => match message {
                    ActorMessage::CheckNow => self.check().await,
                    ActorMessage::Apply { version, settings, reply } => {
                        let result = self.apply(version, settings).await;
                        if reply.send(result).is_err() {
                            warn!("ApplySettings {version} finished after its caller left");
                        }
                    }
                },
            }
        }

        info!("Settings reporter shut down");
    }

    async fn read(&mut self) -> Option<Reading> {
        match time::timeout(READ_TIMEOUT, self.system.read()).await {
            Ok(reading) => Some(reading),
            Err(_) => {
                warn!("Settings read timed out after {READ_TIMEOUT:?}");
                None
            }
        }
    }

    async fn check(&mut self) {
        let Some(reading) = self.read().await else {
            return;
        };
        let hold = hold_for_reload(self.previous_vendor_present, reading.vendor_present);
        self.previous_vendor_present = Some(reading.vendor_present);
        if hold {
            info!("Wi-Fi driver interface missing; waiting one check before reporting");
            return;
        }

        let report = self.enforce(reading, true).await;
        if !should_send(
            &report,
            self.last_sent.as_ref(),
            self.last_sent_at.elapsed(),
        ) {
            return;
        }
        self.system
            .queue_report(SafeCommandResponse {
                id: CMD_ID_REPORT_SETTINGS,
                command: SafeCommandRx::ReportSettings {
                    report: Box::new(report.clone()),
                },
                status: 0,
            })
            .await;
        info!("Settings report queued for the next poll");
        self.sent(report);
    }

    fn sent(&mut self, report: SettingsReport) {
        self.corrected.clear();
        self.last_sent = Some(report);
        self.last_sent_at = Instant::now();
    }

    /// Stores the set, starts its version afresh (a Re-apply of the same
    /// version included) and applies it now.
    async fn apply(&mut self, version: i32, settings: WantedSettings) -> SafeCommandRx {
        info!("Applying settings version {version}");
        let wanted = StoredSettings { version, settings };
        if let Err(err) = self.system.save_wanted(&wanted).await {
            error!("Saving settings version {version}: {err:#}; applying it without a boot copy");
        }
        let state = SettingsState {
            version,
            ..Default::default()
        };
        if let Err(err) = self.system.save_state(&state).await {
            error!("Saving the state of settings version {version}: {err:#}");
        }
        self.wanted = Some(wanted);
        self.state = Some(state);
        self.attempts.clear();

        let reading = self.read().await.unwrap_or_default();
        let report = self.enforce(reading, false).await;
        self.sent(report.clone());
        SafeCommandRx::ApplySettingsResult {
            applied_version: version,
            reverted: false,
            report: Box::new(report),
        }
    }

    /// Applies what differs from the stored set, then reports from a read
    /// taken after the apply. `enforcing` is false for an `ApplySettings`,
    /// whose changes are not drift.
    async fn enforce(&mut self, reading: Reading, enforcing: bool) -> SettingsReport {
        let Some(wanted) = self.wanted.clone() else {
            return reading.report;
        };
        // A reverted version, or one whose state is unreadable, is not
        // applied, so it is not reported as applied either.
        if self.state.as_ref().is_none_or(|s| s.reverted) {
            return reading.report;
        }
        let mut report = reading.report.clone();
        report.applied_version = Some(wanted.version);

        let ntp = self
            .system
            .ntp_daemon()
            .await
            .inspect_err(|err| warn!("settings: NTP daemon: {err:#}"))
            .ok()
            .flatten();
        let files = match self.system.owned_files(ntp).await {
            Ok(files) => files,
            Err(err) => {
                error!("settings: reading smithd's own files: {err:#}; not applying");
                return report;
            }
        };
        let first = self.plan(&wanted, &reading, &files, ntp);

        if !first.has_steps() {
            self.settle(&first);
            report.apply_status = dedup_statuses(Vec::new(), first.failed);
            report.corrected = self.corrected.clone();
            return report;
        }

        let errors = self.execute(&first, ntp, &reading).await;
        for group in &first.attempted {
            *self.attempts.entry(*group).or_insert(0) += 1;
        }

        let after = self.read().await.unwrap_or(reading);
        let files = self.system.owned_files(ntp).await.unwrap_or(files);
        let second = self.plan(&wanted, &after, &files, ntp);
        self.settle(&second);
        if enforcing {
            for group in first
                .attempted
                .iter()
                .filter(|g| second.in_sync.contains(g))
            {
                for setting in group.settings() {
                    if !self.corrected.contains(setting) {
                        self.corrected.push(*setting);
                    }
                }
            }
        }

        let mut report = after.report;
        report.applied_version = Some(wanted.version);
        report.apply_status = dedup_statuses(errors, second.failed);
        report.corrected = self.corrected.clone();
        report
    }

    fn plan(
        &self,
        wanted: &StoredSettings,
        reading: &Reading,
        files: &OwnedFiles,
        ntp: Option<NtpDaemon>,
    ) -> ApplyPlan {
        let disruptions = self
            .state
            .as_ref()
            .map_or(MAX_DISRUPTIONS, |s| s.disruptions);
        plan(&PlanInput {
            settings: &wanted.settings,
            report: &reading.report,
            files,
            ntp_daemon: ntp,
            wifi_connected: reading.wifi_connected,
            attempts: &self.attempts,
            disruption_allowed: disruptions < MAX_DISRUPTIONS,
        })
    }

    /// A group running its value starts its attempts over, so a later drift
    /// is corrected again.
    fn settle(&mut self, plan: &ApplyPlan) {
        for group in &plan.in_sync {
            self.attempts.remove(group);
        }
    }

    /// Counts the disruption on disk before taking it, so a restart in the
    /// middle cannot take it twice. `false` when it could not be recorded.
    async fn spend_disruption(&mut self) -> bool {
        let Some(state) = self.state.as_mut() else {
            return false;
        };
        state.disruptions += 1;
        match self.system.save_state(state).await {
            Ok(()) => true,
            Err(err) => {
                error!("settings: recording the disruption: {err:#}");
                false
            }
        }
    }

    /// Runs the plan in a fixed order: files, NM reload (so a driver reload
    /// reconnects with the new drop-in), driver reload, roam values (which a
    /// reload resets), NM re-activation, NTP, hostname, then bgscan (which NM
    /// overwrites on activation). Returns a status per failed step.
    async fn execute(
        &mut self,
        plan: &ApplyPlan,
        ntp: Option<NtpDaemon>,
        reading: &Reading,
    ) -> Vec<SettingStatus> {
        let busy_supported = reading.report.busy_diff.is_some();
        let mut steps = vec![
            Step::File(OwnedFile::Driver, &plan.driver_file),
            Step::File(OwnedFile::Nm, &plan.nm_file),
        ];
        if let Some(daemon) = ntp {
            steps.push(Step::File(OwnedFile::Ntp(daemon), &plan.ntp_file));
        }
        steps.retain(|step| !matches!(step, Step::File(_, FileChange::Keep)));
        if plan.nm_reload {
            steps.push(Step::ReloadNm);
        }
        if plan.driver_reload {
            steps.push(Step::ReloadDriver);
        }
        if let Some(flags) = plan.roam_flags {
            steps.push(Step::RoamFlags(flags));
        }
        if let Some(tuning) = &plan.roam_tuning {
            steps.push(Step::RoamTuning(tuning, busy_supported));
        }
        if plan.nm_reactivate {
            steps.push(Step::ReactivateWifi);
        }
        if let (true, Some(daemon)) = (plan.ntp_reload, ntp) {
            steps.push(Step::ReloadNtp(daemon));
        }
        if let Some(hostname) = &plan.hostname {
            steps.push(Step::Hostname(hostname));
        }
        if let Some(bgscan) = &plan.bgscan {
            steps.push(Step::Bgscan(bgscan));
        }

        let disrupts = plan.disrupts();
        if disrupts {
            self.system.hold_police(true).await;
        }
        let mut errors = Vec::new();
        for step in steps {
            let disruptive = matches!(step, Step::ReloadDriver | Step::ReactivateWifi);
            // Checked before spending the version's one disruption on a step
            // that cannot run.
            let result = if disruptive && reading.wifi.is_none() {
                Err(anyhow!("no Wi-Fi interface"))
            } else if disruptive && !self.spend_disruption().await {
                Err(anyhow!("could not record the disruption"))
            } else {
                self.system.run(step).await
            };
            if let Err(err) = result {
                error!("settings: {step:?} failed: {err:#}");
                errors.extend(
                    step.groups()
                        .iter()
                        .filter(|g| plan.attempted.contains(g))
                        .flat_map(|g| g.settings())
                        .map(|setting| SettingStatus {
                            setting: *setting,
                            state: SettingApplyState::Failed,
                            reason: format!("{err:#}"),
                        }),
                );
            }
        }
        if disrupts {
            self.system.hold_police(false).await;
        }
        errors
    }
}

/// One status per setting: a step error is more specific than the planner's
/// reason, and the api stores one row per setting.
fn dedup_statuses(first: Vec<SettingStatus>, then: Vec<SettingStatus>) -> Vec<SettingStatus> {
    let mut statuses: Vec<SettingStatus> = Vec::new();
    for status in first.into_iter().chain(then) {
        if !statuses.iter().any(|s| s.setting == status.setting) {
            statuses.push(status);
        }
    }
    statuses
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::driver::WifiInterface;
    use crate::shutdown::ShutdownHandler;
    use crate::utils::schema::Managed;
    use std::sync::{Arc, Mutex};

    #[test]
    fn sends_on_change_or_daily_but_not_on_ntp_hops() {
        let a = SettingsReport {
            hostname: Some("a".to_owned()),
            ntp_server_in_use: Some("185.125.190.122".to_owned()),
            ntp_synchronized: Some(false),
            ..Default::default()
        };
        let synced = SettingsReport {
            ntp_synchronized: Some(true),
            ..a.clone()
        };
        let renamed = SettingsReport {
            hostname: Some("b".to_owned()),
            ..a.clone()
        };
        let hopped = SettingsReport {
            ntp_server_in_use: Some("185.125.190.123".to_owned()),
            ..a.clone()
        };
        let hour = Duration::from_secs(3600);

        assert!(!should_send(&a, Some(&a), hour), "unchanged");
        assert!(!should_send(&hopped, Some(&a), hour), "NTP hop");
        assert!(should_send(&renamed, Some(&a), hour), "changed");
        assert!(should_send(&synced, Some(&a), hour), "sync flip");
        assert!(should_send(&a, None, hour), "never sent");
        assert!(should_send(&a, Some(&a), FULL_REPORT_EVERY), "daily");
    }

    #[test]
    fn reload_hold_skips_only_the_first_check_without_the_interface() {
        assert!(hold_for_reload(Some(true), false), "just vanished");
        assert!(!hold_for_reload(Some(false), false), "still gone");
        assert!(!hold_for_reload(None, false), "never had it");
    }

    /// A device that runs what smithd tells it, unless `stuck` names the
    /// setting group it ignores.
    #[derive(Default)]
    struct Device {
        report: SettingsReport,
        files: OwnedFiles,
        steps: Vec<String>,
        stuck: Option<Group>,
        no_wifi: bool,
    }

    #[derive(Clone, Default)]
    struct FakeSystem(Arc<Mutex<Device>>);

    impl FakeSystem {
        fn device(&self) -> std::sync::MutexGuard<'_, Device> {
            self.0.lock().unwrap()
        }
    }

    impl System for FakeSystem {
        async fn read(&mut self) -> Reading {
            let device = self.device();
            Reading {
                report: device.report.clone(),
                vendor_present: true,
                wifi: (!device.no_wifi).then(|| WifiInterface {
                    name: "wlP1p1s0".to_owned(),
                    driver: "rtl88x2ce".to_owned(),
                    module: "rtl8822ce".to_owned(),
                }),
                wifi_connected: !device.no_wifi,
            }
        }

        async fn ntp_daemon(&mut self) -> Result<Option<NtpDaemon>> {
            Ok(None)
        }

        async fn owned_files(&mut self, _ntp: Option<NtpDaemon>) -> Result<OwnedFiles> {
            Ok(self.device().files.clone())
        }

        async fn run(&mut self, step: Step<'_>) -> Result<()> {
            let mut device = self.device();
            device.steps.push(format!("{step:?}"));
            let stuck = device.stuck;
            match step {
                Step::File(OwnedFile::Driver, change) => {
                    device.files.driver = match change {
                        FileChange::Write(text) => Some(text.clone()),
                        _ => None,
                    }
                }
                Step::Hostname(name) if stuck != Some(Group::Hostname) => {
                    device.report.hostname = Some(name.to_owned())
                }
                Step::RoamFlags(flags) if stuck != Some(Group::RoamFlags) => {
                    device.report.roam_flags = Some(flags)
                }
                _ => {}
            }
            Ok(())
        }

        async fn save_wanted(&mut self, _wanted: &StoredSettings) -> Result<()> {
            Ok(())
        }

        async fn save_state(&mut self, state: &SettingsState) -> Result<()> {
            let step = format!("save_state({})", state.disruptions);
            self.device().steps.push(step);
            Ok(())
        }

        async fn hold_police(&mut self, hold: bool) {
            self.device().steps.push(format!("hold_police({hold})"));
        }

        async fn queue_report(&mut self, _response: SafeCommandResponse) {}
    }

    fn stored(version: i32, settings: WantedSettings) -> StoredSettings {
        StoredSettings { version, settings }
    }

    fn hostname(name: &str) -> WantedSettings {
        WantedSettings {
            hostname: Some(Managed::Set(name.to_owned())),
            ..Default::default()
        }
    }

    fn actor_with(
        system: FakeSystem,
        wanted: Option<StoredSettings>,
        state: Option<SettingsState>,
        receiver: mpsc::Receiver<ActorMessage>,
    ) -> Actor<FakeSystem> {
        Actor::new(
            ShutdownHandler::new().signals(),
            receiver,
            system,
            wanted,
            state,
        )
    }

    fn actor(system: FakeSystem, wanted: Option<StoredSettings>) -> Actor<FakeSystem> {
        let state = initial_state(wanted.as_ref(), Ok(None));
        actor_with(system, wanted, state, mpsc::channel(8).1)
    }

    async fn check(actor: &mut Actor<FakeSystem>) -> SettingsReport {
        let reading = actor.system.read().await;
        actor.enforce(reading, true).await
    }

    #[tokio::test]
    async fn local_enforcement_puts_a_drifted_value_back_and_reports_it_corrected() {
        let system = FakeSystem::default();
        let mut actor = actor(system.clone(), Some(stored(4, hostname("byag-12"))));
        system.device().report.hostname = Some("orin-1".to_owned());
        let report = check(&mut actor).await;
        assert_eq!(report.hostname.as_deref(), Some("byag-12"));
        assert_eq!(report.corrected, vec![DeviceSetting::Hostname]);
        assert_eq!(report.applied_version, Some(4));
        assert!(report.apply_status.is_empty());
    }

    #[tokio::test]
    async fn a_value_that_does_not_stick_is_reported_failed_after_two_attempts() {
        let system = FakeSystem::default();
        system.device().stuck = Some(Group::Hostname);
        system.device().report.hostname = Some("orin-1".to_owned());
        let mut actor = actor(system.clone(), Some(stored(4, hostname("byag-12"))));

        assert!(check(&mut actor).await.apply_status.is_empty(), "attempt 1");
        assert_eq!(
            check(&mut actor).await.apply_status,
            [SettingStatus {
                setting: DeviceSetting::Hostname,
                state: SettingApplyState::Failed,
                reason: "still Some(\"orin-1\") after 2 attempts".to_owned(),
            }],
            "after attempt 2"
        );

        check(&mut actor).await;
        let tries = system
            .device()
            .steps
            .iter()
            .filter(|s| s.starts_with("Hostname"))
            .count();
        assert_eq!(tries, 2, "not retried once failed");
    }

    #[tokio::test]
    async fn steps_run_in_the_fixed_order_with_the_disruption_saved_before_the_reload() {
        let system = FakeSystem::default();
        system.device().report = SettingsReport {
            wireless_mode: Some(95),
            roam_flags: Some(3),
            hostname: Some("orin-1".to_owned()),
            ..Default::default()
        };
        let settings = WantedSettings {
            wireless_mode: Some(Managed::Set(11)),
            roam_flags: Some(Managed::Set(7)),
            nm_autoconnect_retries: Some(Managed::Set(0)),
            hostname: Some(Managed::Set("byag-12".to_owned())),
            ..Default::default()
        };
        let mut actor = actor(system.clone(), None);
        actor.apply(4, settings).await;

        let steps: Vec<String> = system
            .device()
            .steps
            .iter()
            .map(|s| s.split(['(', ' ']).next().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(
            steps,
            [
                "save_state",
                "hold_police",
                "File",
                "File",
                "ReloadNm",
                "save_state",
                "ReloadDriver",
                "RoamFlags",
                "Hostname",
                "hold_police"
            ]
        );
    }

    #[tokio::test]
    async fn a_spent_disruption_is_not_taken_again_after_a_restart() {
        let system = FakeSystem::default();
        system.device().report.wireless_mode = Some(95);
        let wanted = stored(
            4,
            WantedSettings {
                wireless_mode: Some(Managed::Set(11)),
                ..Default::default()
            },
        );
        let spent = SettingsState {
            version: 4,
            reverted: false,
            disruptions: MAX_DISRUPTIONS,
        };
        let state = initial_state(Some(&wanted), Ok(Some(spent)));
        let mut actor = actor_with(system.clone(), Some(wanted), state, mpsc::channel(8).1);
        let report = check(&mut actor).await;
        assert!(!system.device().steps.iter().any(|s| s == "ReloadDriver"));
        assert_eq!(
            report.apply_status,
            [SettingStatus {
                setting: DeviceSetting::WirelessMode,
                state: SettingApplyState::Failed,
                reason: "still 95: needs a driver reload or Wi-Fi re-activation, \
                         already done for this version"
                    .to_owned(),
            }]
        );
    }

    #[tokio::test]
    async fn an_unreadable_state_file_applies_nothing() {
        let system = FakeSystem::default();
        system.device().report.hostname = Some("orin-1".to_owned());
        let wanted = stored(4, hostname("byag-12"));
        let state = initial_state(Some(&wanted), Err(anyhow!("EIO")));
        let mut actor = actor_with(system.clone(), Some(wanted), state, mpsc::channel(8).1);
        let report = check(&mut actor).await;
        assert!(system.device().steps.is_empty());
        assert_eq!(report.applied_version, None);
    }

    #[tokio::test]
    async fn a_reverted_version_is_neither_applied_nor_reported_applied() {
        let system = FakeSystem::default();
        system.device().report.hostname = Some("orin-1".to_owned());
        let wanted = stored(4, hostname("byag-12"));
        let reverted = SettingsState {
            version: 4,
            reverted: true,
            disruptions: 1,
        };
        let state = initial_state(Some(&wanted), Ok(Some(reverted)));
        let mut actor = actor_with(system.clone(), Some(wanted), state, mpsc::channel(8).1);
        let report = check(&mut actor).await;
        assert!(system.device().steps.is_empty());
        assert_eq!(report.applied_version, None);
    }

    #[tokio::test]
    async fn no_wifi_interface_fails_the_reload_without_spending_the_disruption() {
        let system = FakeSystem::default();
        system.device().no_wifi = true;
        system.device().report.wireless_mode = Some(95);
        let mut actor = actor(system.clone(), None);
        let settings = WantedSettings {
            wireless_mode: Some(Managed::Set(11)),
            ..Default::default()
        };
        actor.apply(4, settings).await;
        let steps = system.device().steps.clone();
        assert!(!steps.iter().any(|s| s == "ReloadDriver"));
        assert_eq!(
            actor.state.map(|s| s.disruptions),
            Some(0),
            "steps: {steps:?}"
        );
    }

    #[test]
    fn a_state_of_another_version_starts_fresh() {
        let wanted = stored(5, WantedSettings::default());
        let old = SettingsState {
            version: 4,
            reverted: true,
            disruptions: 1,
        };
        assert_eq!(
            initial_state(Some(&wanted), Ok(Some(old))),
            Some(SettingsState {
                version: 5,
                ..Default::default()
            })
        );
    }

    #[tokio::test]
    async fn consecutive_applies_run_in_order_and_each_is_answered() {
        let system = FakeSystem::default();
        system.device().report.hostname = Some("orin-1".to_owned());
        let (sender, receiver) = mpsc::channel(8);
        let state = initial_state(None, Ok(None));
        let mut actor = actor_with(system.clone(), None, state, receiver);
        tokio::spawn(async move { actor.run().await });

        let mut replies = Vec::new();
        for (version, name) in [(1, "first"), (2, "second")] {
            let (reply, answer) = oneshot::channel();
            sender
                .send(ActorMessage::Apply {
                    version,
                    settings: hostname(name),
                    reply,
                })
                .await
                .unwrap();
            replies.push(answer);
        }
        let mut versions = Vec::new();
        for answer in replies {
            match answer.await.unwrap() {
                SafeCommandRx::ApplySettingsResult {
                    applied_version,
                    report,
                    ..
                } => versions.push((applied_version, report.hostname)),
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(
            versions,
            [
                (1, Some("first".to_owned())),
                (2, Some("second".to_owned()))
            ]
        );
        let hostnames: Vec<_> = system
            .device()
            .steps
            .iter()
            .filter(|s| s.starts_with("Hostname"))
            .cloned()
            .collect();
        assert_eq!(hostnames, ["Hostname(\"first\")", "Hostname(\"second\")"]);
    }
}
