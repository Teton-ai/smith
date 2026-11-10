use super::READ_TIMEOUT;
use super::apply::DeviceSystem;
use super::plan::{
    ApplyPlan, FileChange, Group, MAX_DISRUPTIONS, NtpDaemon, OwnedFiles, PlanInput, plan,
};
use super::read::Reading;
use crate::postman::LastPoll;
use crate::shutdown::ShutdownSignals;
use crate::utils::files::{
    Disruption, SettingsBackup, SettingsState, StoredSettings, load_settings_state,
    load_wanted_settings,
};
use crate::utils::net_guard::{NetGuard, NetGuardHold};
use crate::utils::schema::{
    DeviceSetting, Managed, RoamTuning, SafeCommandResponse, SafeCommandRx, SettingApplyState,
    SettingStatus, SettingsReport, WantedSettings,
};
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::future::Future;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{self, Duration, Instant};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// Synthetic command id for reports smithd sends without being asked.
const CMD_ID_REPORT_SETTINGS: i32 = -7;

/// NM keeps moving for a while after link health's nmcli calls return; a
/// driver reload or re-activation in that window would fight the recovery.
const RECOVERY_SETTLE: Duration = Duration::from_secs(30);

/// How long the api may stay unreachable after a driver reload or Wi-Fi
/// re-activation before the replaced files are put back.
const REVERT_DEADLINE: Duration = Duration::from_secs(90);
const API_PROBE_EVERY: Duration = Duration::from_secs(10);
const REVERT_REASON: &str = "reverted: no connectivity after apply";

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

    /// `Some` for the steps that take the link down.
    fn disruption(&self) -> Option<Disruption> {
        match self {
            Step::ReloadDriver => Some(Disruption::ReloadDriver),
            Step::ReactivateWifi => Some(Disruption::ReactivateWifi),
            _ => None,
        }
    }
}

impl From<Disruption> for Step<'static> {
    fn from(disruption: Disruption) -> Self {
        match disruption {
            Disruption::ReloadDriver => Step::ReloadDriver,
            Disruption::ReactivateWifi => Step::ReactivateWifi,
        }
    }
}

/// The roam values running now, as a `roam_param` write.
fn running_roam_tuning(report: &SettingsReport) -> Option<RoamTuning> {
    Some(RoamTuning {
        idle_diff: report.idle_diff?,
        busy_diff: report.busy_diff,
        scanr_exp_ms: report.scanr_exp_ms?,
        scan_int: report.scan_int?,
        idle_th: report.idle_th?,
        busy_th: report.busy_th,
    })
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
    fn queue_report(&mut self, response: SafeCommandResponse) -> impl Future<Output = ()> + Send;
    fn last_poll(&self) -> LastPoll;
    /// A TCP connect to the api host: no request, so it adds no api load.
    fn api_connects(&self) -> impl Future<Output = bool> + Send + 'static;
}

#[derive(Debug, PartialEq, Eq)]
enum PollWait {
    Reached,
    TimedOut,
    /// smithd is stopping (the postman's channel closes when it exits).
    Stopping,
}

/// Waits up to `deadline` for the api to be reached later than `after` (one
/// from before proves nothing about the link since): a home poll the api
/// answered, or a TCP connect to it. The probe is there because the postman's
/// backoff after the posts that failed during the disruption can outlast the
/// deadline, and polling it sooner would defeat that backoff fleet wide.
async fn reached_after<Fut: Future<Output = bool>>(
    mut last_poll: LastPoll,
    after: Instant,
    deadline: Duration,
    stopping: &CancellationToken,
    probe: impl Fn() -> Fut,
) -> PollWait {
    let reached = async {
        let mut probes = time::interval(API_PROBE_EVERY);
        probes.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                // Mapped inside: the guard `wait_for` returns is not `Send`.
                open = async { last_poll.wait_for(|at| at.is_some_and(|at| at > after)).await.is_ok() } => {
                    return if open { PollWait::Reached } else { PollWait::Stopping };
                }
                _ = probes.tick() => {
                    if probe().await {
                        return PollWait::Reached;
                    }
                }
            }
        }
    };
    tokio::select! {
        biased;
        _ = stopping.cancelled() => PollWait::Stopping,
        waited = time::timeout(deadline, reached) => waited.unwrap_or(PollWait::TimedOut),
    }
}

/// A driver reload or re-activation that ran, holding link health off until
/// the api answers or the revert is done.
struct Disrupted {
    at: Instant,
    _network_busy: NetGuardHold,
}

/// Puts a backed up file back; one that did not exist before the apply is removed.
fn restore(backup: &Option<String>) -> FileChange {
    match backup {
        Some(text) => FileChange::Write(text.clone()),
        None => FileChange::Remove,
    }
}

pub struct Actor<S> {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<ActorMessage>,
    system: S,
    net_guard: NetGuard,
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
        net_guard: NetGuard,
    ) -> Self {
        let wanted = load_wanted_settings().await;
        let state = initial_state(wanted.as_ref(), load_settings_state().await);
        Self::new(shutdown, receiver, system, net_guard, wanted, state)
    }
}

impl<S: System> Actor<S> {
    pub fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<ActorMessage>,
        system: S,
        net_guard: NetGuard,
        wanted: Option<StoredSettings>,
        state: Option<SettingsState>,
    ) -> Self {
        Self {
            shutdown,
            receiver,
            system,
            net_guard,
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
        self.revert_unconfirmed().await;

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

    /// A backup still on disk at start means smithd stopped between a
    /// disruption and the api's answer, so nothing proved the new files keep
    /// the device online.
    async fn revert_unconfirmed(&mut self) {
        if self.state.as_ref().is_none_or(|s| s.backup.is_none()) {
            return;
        }
        warn!("settings: the last apply was neither confirmed nor fully reverted; reverting it");
        // Learns the Wi-Fi interface the redone step needs.
        self.read().await;
        let _network_busy = self.net_guard.hold_after_recovery(RECOVERY_SETTLE).await;
        self.revert().await;
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
        // Applied without a boot copy, the set would be replaced by the
        // previous version's at the next restart while the api counts it as
        // applied, so it is refused and the previous set stays in force.
        if let Err(err) = self.system.save_wanted(&wanted).await {
            error!("Saving settings version {version}: {err:#}; not applying it");
            return self
                .refuse(&wanted, format!("could not store settings: {err:#}"))
                .await;
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
            reverted: self.state.as_ref().is_some_and(|s| s.reverted),
            report: Box::new(report),
        }
    }

    /// Answers an apply that was not run: the report is a plain read with the
    /// previously applied version, and every requested setting is failed.
    async fn refuse(&mut self, wanted: &StoredSettings, reason: String) -> SafeCommandRx {
        let previous = self
            .wanted
            .as_ref()
            .filter(|_| self.state.as_ref().is_some_and(|s| !s.reverted))
            .map(|w| w.version);
        let mut report = self.read().await.unwrap_or_default().report;
        report.applied_version = previous;
        report.apply_status = requested_groups(&wanted.settings)
            .into_iter()
            .flat_map(|group| group.settings())
            .map(|setting| SettingStatus {
                setting: *setting,
                state: SettingApplyState::Failed,
                reason: reason.clone(),
            })
            .collect();
        SafeCommandRx::ApplySettingsResult {
            applied_version: previous.unwrap_or(0),
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
        match &self.state {
            None => return reading.report,
            Some(state) if state.reverted => return self.reverted_report(reading.report),
            Some(_) => {}
        }
        // A detection error says nothing about the daemon, so NTP is left out
        // of this check rather than reported failed for "no NTP daemon".
        let (ntp, wanted) = match self.system.ntp_daemon().await {
            Ok(ntp) => (ntp, wanted),
            Err(err) => {
                warn!("settings: NTP daemon: {err:#}; NTP left for the next check");
                let settings = WantedSettings {
                    ntp_servers: None,
                    ..wanted.settings
                };
                (None, StoredSettings { settings, ..wanted })
            }
        };
        let files = match self.system.owned_files(ntp).await {
            Ok(files) => files,
            Err(err) => {
                error!("settings: reading smithd's own files: {err:#}; not applying");
                return reading.report;
            }
        };
        let mut report = reading.report.clone();
        report.applied_version = Some(wanted.version);
        let first = self.plan(&wanted, &reading, &files, ntp);
        for group in &first.attempted {
            *self.attempts.entry(*group).or_insert(0) += 1;
        }

        if !first.has_steps() {
            self.settle(&first);
            report.apply_status = dedup_statuses(Vec::new(), first.failed);
            report.corrected = self.corrected.clone();
            return report;
        }

        let (errors, disrupted) = self.execute(&first, ntp, &reading, &files).await;
        if let Some(disrupted) = disrupted {
            self.await_api(disrupted).await;
        }

        let after = self.read().await.unwrap_or(reading);
        if self.state.as_ref().is_some_and(|s| s.reverted) {
            return self.reverted_report(after.report);
        }
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

    /// The api replaces statuses on every report, so a reverted version
    /// repeats its revert, for the groups it set that the revert put back.
    fn reverted_report(&self, mut report: SettingsReport) -> SettingsReport {
        let Some(wanted) = &self.wanted else {
            return report;
        };
        report.apply_status = set_groups(&wanted.settings)
            .into_iter()
            .filter(|group| !matches!(group, Group::Hostname | Group::NtpServers))
            .flat_map(|group| group.settings())
            .map(|setting| SettingStatus {
                setting: *setting,
                state: SettingApplyState::Reverted,
                reason: REVERT_REASON.to_owned(),
            })
            .collect();
        report
    }

    /// Counts the disruption and backs up the files it takes into use, on
    /// disk before taking it, so a restart in the middle can neither take it
    /// twice nor lose the way back. `false` when it could not be recorded.
    async fn spend_disruption(&mut self, backup: SettingsBackup) -> bool {
        let Some(state) = self.state.as_mut() else {
            return false;
        };
        state.disruptions += 1;
        state.backup = Some(backup);
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
    /// overwrites on activation). Returns a status per failed step, and the
    /// driver reload or re-activation if one ran.
    async fn execute(
        &mut self,
        plan: &ApplyPlan,
        ntp: Option<NtpDaemon>,
        reading: &Reading,
        before: &OwnedFiles,
    ) -> (Vec<SettingStatus>, Option<Disrupted>) {
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

        let mut errors = Vec::new();
        // Taken before the first reload or re-activation and kept to the end,
        // so link health neither runs during it nor right after.
        let mut network_busy: Option<NetGuardHold> = None;
        let mut disrupted_at = None;
        for step in steps {
            let disruption = step.disruption();
            if disruption.is_some() && network_busy.is_none() {
                network_busy = Some(self.net_guard.hold_after_recovery(RECOVERY_SETTLE).await);
            }
            // Checked before spending the version's one disruption on a step
            // that cannot run.
            let result = match disruption {
                Some(_) if reading.wifi.is_none() => Err(anyhow!("no Wi-Fi interface")),
                Some(disruption) => {
                    let backup = SettingsBackup {
                        driver: before.driver.clone(),
                        nm: before.nm.clone(),
                        disruption,
                        roam_flags: reading.report.roam_flags,
                        roam_tuning: running_roam_tuning(&reading.report),
                        bgscan: reading.report.bgscan.clone(),
                    };
                    if self.spend_disruption(backup).await {
                        info!(
                            "settings: {step:?}; reverting unless the api is reached within {REVERT_DEADLINE:?}"
                        );
                        let result = self.system.run(step).await;
                        // A failed reload can leave the link down too.
                        disrupted_at = Some(Instant::now());
                        result
                    } else {
                        Err(anyhow!("could not record the disruption"))
                    }
                }
                None => self.system.run(step).await,
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
        let disrupted = disrupted_at
            .zip(network_busy)
            .map(|(at, network_busy)| Disrupted {
                at,
                _network_busy: network_busy,
            });
        (errors, disrupted)
    }

    /// Keeps the version once the api answers after the disruption, else
    /// reverts it. Link health stays out until this returns. When smithd
    /// stops first, the backup stays on disk for the next start to revert:
    /// a revert cut short by systemd's kill could leave the driver unloaded.
    async fn await_api(&mut self, disrupted: Disrupted) {
        let stopping = self.shutdown.token.clone();
        let system = &self.system;
        let reached = reached_after(
            system.last_poll(),
            disrupted.at,
            REVERT_DEADLINE,
            &stopping,
            || system.api_connects(),
        )
        .await;
        match reached {
            PollWait::Reached => {
                info!(
                    "settings: api reached {:?} after the apply; keeping it",
                    disrupted.at.elapsed()
                );
                self.confirm().await;
            }
            PollWait::TimedOut => {
                warn!(
                    "settings: api not reached within {REVERT_DEADLINE:?} of the apply; reverting"
                );
                self.revert().await;
            }
            PollWait::Stopping => {
                warn!(
                    "settings: smithd stopping before the api was reached; the next start reverts the apply"
                );
            }
        }
    }

    /// The api was reached after the disruption: its backup is no longer
    /// needed, and a restart must not revert the version.
    async fn confirm(&mut self) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        state.backup = None;
        if let Err(err) = self.system.save_state(state).await {
            error!("settings: clearing the apply backup: {err:#}; a restart will revert it");
        }
    }

    /// Puts the backed up files back, redoes the step that takes them into
    /// use and writes back the live values it reset, then marks the version
    /// reverted, so it is neither enforced nor applied at boot until a new
    /// version or a Re-apply. A device still offline after this is left to
    /// Police.
    async fn revert(&mut self) {
        let Some(backup) = self.state.as_mut().and_then(|s| s.backup.take()) else {
            return;
        };
        info!(
            "settings: reverting, restoring the driver and NM files and redoing {:?}",
            backup.disruption
        );
        let driver = restore(&backup.driver);
        let nm = restore(&backup.nm);
        let mut files_restored = true;
        for step in [
            Step::File(OwnedFile::Driver, &driver),
            Step::File(OwnedFile::Nm, &nm),
            Step::ReloadNm,
            backup.disruption.into(),
        ] {
            if let Err(err) = self.system.run(step).await {
                error!("settings: reverting, {step:?} failed: {err:#}");
                files_restored &= !matches!(step, Step::File(..));
            }
        }
        // Learns the interface again, which a boot without it did not know.
        self.read().await;
        let mut live = Vec::new();
        if let Some(flags) = backup.roam_flags {
            live.push(Step::RoamFlags(flags));
        }
        if let Some(tuning) = &backup.roam_tuning {
            live.push(Step::RoamTuning(tuning, tuning.busy_diff.is_some()));
        }
        if let Some(bgscan) = &backup.bgscan {
            live.push(Step::Bgscan(bgscan));
        }
        for step in live {
            if let Err(err) = self.system.run(step).await {
                error!("settings: reverting, {step:?} failed: {err:#}");
            }
        }
        let Some(state) = self.state.as_mut() else {
            return;
        };
        // A file left as applied is loaded again on every boot, so the next
        // start retries the revert. Failed reloads and live values are not
        // kept for it: a boot reloads the restored files anyway, and a value
        // that never takes would reload the driver on every start.
        if !files_restored {
            warn!("settings: a file was not restored; the next start retries the revert");
            state.backup = Some(backup);
        }
        state.reverted = true;
        info!("settings: version {} reverted", state.version);
        if let Err(err) = self.system.save_state(state).await {
            error!("settings: recording the revert: {err:#}");
        }
    }
}

/// The groups an `ApplySettings` gives an instruction for.
fn requested_groups(settings: &WantedSettings) -> Vec<Group> {
    [
        (settings.wireless_mode.is_some(), Group::WirelessMode),
        (settings.power_mgnt.is_some(), Group::PowerMgnt),
        (settings.roam_flags.is_some(), Group::RoamFlags),
        (settings.roam_tuning.is_some(), Group::RoamTuning),
        (settings.nm_wifi_powersave.is_some(), Group::NmWifiPowersave),
        (
            settings.nm_autoconnect_retries.is_some(),
            Group::NmAutoconnectRetries,
        ),
        (settings.mac.is_some(), Group::Mac),
        (settings.hostname.is_some(), Group::Hostname),
        (settings.ntp_servers.is_some(), Group::NtpServers),
        (settings.bgscan.is_some(), Group::Bgscan),
    ]
    .into_iter()
    .filter_map(|(requested, group)| requested.then_some(group))
    .collect()
}

/// The groups an `ApplySettings` gives a value for.
fn set_groups(settings: &WantedSettings) -> Vec<Group> {
    fn set<T>(managed: &Option<Managed<T>>) -> bool {
        matches!(managed, Some(Managed::Set(_)))
    }
    [
        (set(&settings.wireless_mode), Group::WirelessMode),
        (set(&settings.power_mgnt), Group::PowerMgnt),
        (set(&settings.roam_flags), Group::RoamFlags),
        (set(&settings.roam_tuning), Group::RoamTuning),
        (set(&settings.nm_wifi_powersave), Group::NmWifiPowersave),
        (
            set(&settings.nm_autoconnect_retries),
            Group::NmAutoconnectRetries,
        ),
        (set(&settings.mac), Group::Mac),
        (set(&settings.hostname), Group::Hostname),
        (set(&settings.ntp_servers), Group::NtpServers),
        (set(&settings.bgscan), Group::Bgscan),
    ]
    .into_iter()
    .filter_map(|(set, group)| set.then_some(group))
    .collect()
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
    use std::sync::{Arc, Mutex};
    use tokio::sync::watch;

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
        files_unreadable: bool,
        wanted_unwritable: bool,
        ntp_detection_fails: bool,
        ntp_daemon: Option<NtpDaemon>,
        /// A home poll succeeds this long after a reload or re-activation;
        /// `None` means the api stays unreachable.
        api_back_after: Option<Duration>,
        /// A TCP connect to the api succeeds.
        api_connects: bool,
        /// Steps whose `Debug` starts with one of these fail.
        failing: Vec<&'static str>,
    }

    #[derive(Clone)]
    struct FakeSystem {
        device: Arc<Mutex<Device>>,
        polls: Arc<watch::Sender<Option<Instant>>>,
    }

    impl Default for FakeSystem {
        fn default() -> Self {
            Self {
                device: Arc::default(),
                polls: Arc::new(watch::Sender::new(None)),
            }
        }
    }

    impl FakeSystem {
        fn device(&self) -> std::sync::MutexGuard<'_, Device> {
            self.device.lock().unwrap()
        }

        fn api_back_after(self, after: Duration) -> Self {
            self.device().api_back_after = Some(after);
            self
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
            if self.device().ntp_detection_fails {
                return Err(anyhow!("D-Bus timeout"));
            }
            Ok(self.device().ntp_daemon)
        }

        async fn owned_files(&mut self, _ntp: Option<NtpDaemon>) -> Result<OwnedFiles> {
            let device = self.device();
            if device.files_unreadable {
                return Err(anyhow!("EACCES"));
            }
            Ok(device.files.clone())
        }

        async fn run(&mut self, step: Step<'_>) -> Result<()> {
            let mut device = self.device();
            let name = format!("{step:?}");
            device.steps.push(name.clone());
            if device.failing.iter().any(|prefix| name.starts_with(prefix)) {
                return Err(anyhow!("{name} failed"));
            }
            if matches!(step, Step::ReloadDriver | Step::ReactivateWifi)
                && let Some(after) = device.api_back_after
            {
                let polls = Arc::clone(&self.polls);
                tokio::spawn(async move {
                    time::sleep(after).await;
                    polls.send_replace(Some(Instant::now()));
                });
            }
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
            if self.device().wanted_unwritable {
                return Err(anyhow!("EROFS"));
            }
            Ok(())
        }

        async fn save_state(&mut self, state: &SettingsState) -> Result<()> {
            let step = format!("save_state({})", state.disruptions);
            self.device().steps.push(step);
            Ok(())
        }

        async fn queue_report(&mut self, _response: SafeCommandResponse) {}

        fn last_poll(&self) -> LastPoll {
            self.polls.subscribe()
        }

        fn api_connects(&self) -> impl Future<Output = bool> + Send + 'static {
            let connects = self.device().api_connects;
            async move { connects }
        }
    }

    fn stored(version: i32, settings: WantedSettings) -> StoredSettings {
        StoredSettings { version, settings }
    }

    fn wireless_mode(mode: i16) -> WantedSettings {
        WantedSettings {
            wireless_mode: Some(Managed::Set(mode)),
            ..Default::default()
        }
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
            NetGuard::new(),
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

    #[tokio::test(start_paused = true)]
    async fn steps_run_in_the_fixed_order_with_the_disruption_saved_before_the_reload() {
        let system = FakeSystem::default().api_back_after(Duration::from_secs(20));
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
                "File",
                "File",
                "ReloadNm",
                "save_state",
                "ReloadDriver",
                "RoamFlags",
                "Hostname",
                "save_state"
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_reload_waits_for_a_link_recovery_to_finish() {
        let system = FakeSystem::default();
        system.device().report.wireless_mode = Some(95);
        let mut actor = actor(system.clone(), None);
        let guard = NetGuard::new();
        actor.net_guard = guard.clone();
        let recovery = guard.try_recovery(Duration::ZERO);
        let settings = WantedSettings {
            wireless_mode: Some(Managed::Set(11)),
            ..Default::default()
        };
        let apply = tokio::spawn(async move { actor.apply(4, settings).await });
        let reloaded = || system.device().steps.iter().any(|s| s == "ReloadDriver");

        time::sleep(Duration::from_secs(120)).await;
        assert!(!reloaded(), "recovery still running");
        drop(recovery);
        apply.await.unwrap();
        assert!(reloaded());
    }

    #[tokio::test(start_paused = true)]
    async fn a_poll_after_the_apply_within_the_deadline_is_connectivity() {
        let (polls, last_poll) = watch::channel(None);
        let applied_at = Instant::now();
        tokio::spawn(async move {
            time::sleep(Duration::from_secs(30)).await;
            polls.send_replace(Some(Instant::now()));
            // Kept open: a closed channel would end the wait on its own.
            time::sleep(REVERT_DEADLINE).await;
        });
        assert_eq!(
            reached_after(
                last_poll,
                applied_at,
                REVERT_DEADLINE,
                &CancellationToken::new(),
                || async { false },
            )
            .await,
            PollWait::Reached
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_tcp_connect_to_the_api_within_the_deadline_is_connectivity() {
        let (_polls, last_poll) = watch::channel(None);
        let applied_at = Instant::now();
        let back = applied_at + Duration::from_secs(25);
        let waited = reached_after(
            last_poll,
            applied_at,
            REVERT_DEADLINE,
            &CancellationToken::new(),
            || async move { Instant::now() >= back },
        )
        .await;
        assert_eq!(waited, PollWait::Reached);
        assert_eq!(applied_at.elapsed(), Duration::from_secs(30), "next probe");
    }

    #[tokio::test(start_paused = true)]
    async fn an_apply_whose_api_host_connects_is_kept_without_a_poll() {
        let system = FakeSystem::default();
        system.device().report.wireless_mode = Some(95);
        system.device().api_connects = true;
        let mut actor = actor(system.clone(), None);
        actor.apply(4, wireless_mode(11)).await;
        let state = actor.state.as_ref().unwrap();
        assert!(!state.reverted);
        assert!(state.backup.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn no_poll_within_the_deadline_is_no_connectivity() {
        let (_polls, last_poll) = watch::channel(None);
        assert_eq!(
            reached_after(
                last_poll,
                Instant::now(),
                REVERT_DEADLINE,
                &CancellationToken::new(),
                || async { false },
            )
            .await,
            PollWait::TimedOut
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_poll_from_before_the_apply_does_not_count() {
        let (_polls, last_poll) = watch::channel(Some(Instant::now()));
        time::advance(Duration::from_secs(1)).await;
        assert_eq!(
            reached_after(
                last_poll,
                Instant::now(),
                REVERT_DEADLINE,
                &CancellationToken::new(),
                || async { false },
            )
            .await,
            PollWait::TimedOut
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_stopped_postman_is_not_a_lost_api() {
        let (polls, last_poll) = watch::channel(None);
        drop(polls);
        assert_eq!(
            reached_after(
                last_poll,
                Instant::now(),
                REVERT_DEADLINE,
                &CancellationToken::new(),
                || async { false },
            )
            .await,
            PollWait::Stopping
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_apply_cut_short_by_a_shutdown_keeps_its_backup_for_the_next_start() {
        let system = FakeSystem::default();
        system.device().report.wireless_mode = Some(95);
        let mut actor = actor(system.clone(), None);
        actor.shutdown.token.cancel();
        actor.apply(4, wireless_mode(11)).await;

        let steps = system.device().steps.clone();
        assert_eq!(steps.iter().filter(|s| *s == "ReloadDriver").count(), 1);
        let state = actor.state.as_ref().unwrap();
        assert!(!state.reverted);
        assert_eq!(
            state.backup.as_ref().map(|b| b.disruption),
            Some(Disruption::ReloadDriver)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_apply_that_loses_the_api_is_reverted_and_reported_reverted() {
        let system = FakeSystem::default();
        system.device().report.wireless_mode = Some(95);
        let mut actor = actor(system.clone(), None);
        let SafeCommandRx::ApplySettingsResult {
            reverted, report, ..
        } = actor.apply(4, wireless_mode(11)).await
        else {
            panic!("expected ApplySettingsResult");
        };

        let steps = system.device().steps.clone();
        let after_reload = steps
            .iter()
            .position(|s| s == "ReloadDriver")
            .map_or(&[][..], |i| &steps[i + 1..]);
        assert_eq!(
            after_reload,
            [
                "File(Driver, Remove)",
                "File(Nm, Remove)",
                "ReloadNm",
                "ReloadDriver",
                "save_state(1)"
            ]
        );
        assert!(reverted);
        assert_eq!(report.applied_version, None);
        assert_eq!(
            report.apply_status,
            [SettingStatus {
                setting: DeviceSetting::WirelessMode,
                state: SettingApplyState::Reverted,
                reason: REVERT_REASON.to_owned(),
            }]
        );
        assert_eq!(
            actor.state,
            Some(SettingsState {
                version: 4,
                reverted: true,
                disruptions: 1,
                backup: None,
            })
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_revert_writes_back_the_live_values_that_ran_before_the_apply() {
        let system = FakeSystem::default();
        system.device().report = SettingsReport {
            wireless_mode: Some(95),
            roam_flags: Some(3),
            bgscan: Some("simple:30:-70:86400".to_owned()),
            ..Default::default()
        };
        let settings = WantedSettings {
            roam_flags: Some(Managed::Set(7)),
            ..wireless_mode(11)
        };
        let mut actor = actor(system.clone(), None);
        actor.apply(4, settings).await;

        let device = system.device();
        let tail = &device.steps[device.steps.len() - 3..];
        assert_eq!(
            tail,
            [
                "RoamFlags(3)",
                "Bgscan(\"simple:30:-70:86400\")",
                "save_state(1)"
            ]
        );
        assert_eq!(device.report.roam_flags, Some(3));
    }

    #[tokio::test(start_paused = true)]
    async fn an_apply_that_reaches_the_api_is_kept_and_drops_its_backup() {
        let system = FakeSystem::default().api_back_after(Duration::from_secs(20));
        system.device().report.wireless_mode = Some(95);
        let mut actor = actor(system.clone(), None);
        actor.apply(4, wireless_mode(11)).await;
        let reloads = system
            .device()
            .steps
            .iter()
            .filter(|s| *s == "ReloadDriver")
            .count();
        assert_eq!(reloads, 1);
        assert_eq!(
            actor.state,
            Some(SettingsState {
                version: 4,
                reverted: false,
                disruptions: 1,
                backup: None,
            })
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_backup_left_by_a_restart_is_reverted_at_start() {
        let system = FakeSystem::default();
        let wanted = stored(4, wireless_mode(11));
        let unconfirmed = SettingsState {
            version: 4,
            reverted: false,
            disruptions: 1,
            backup: Some(SettingsBackup {
                driver: Some("options rtl8822ce rtw_wireless_mode=95\n".to_owned()),
                nm: None,
                disruption: Disruption::ReloadDriver,
                roam_flags: None,
                roam_tuning: None,
                bgscan: None,
            }),
        };
        let state = initial_state(Some(&wanted), Ok(Some(unconfirmed)));
        let mut actor = actor_with(system.clone(), Some(wanted), state, mpsc::channel(8).1);
        actor.revert_unconfirmed().await;
        assert_eq!(
            system.device().steps,
            [
                "File(Driver, Write(\"options rtl8822ce rtw_wireless_mode=95\\n\"))",
                "File(Nm, Remove)",
                "ReloadNm",
                "ReloadDriver",
                "save_state(1)"
            ]
        );
        assert!(actor.state.is_some_and(|s| s.reverted));
    }

    async fn revert_at_start_failing(failing: &'static str) -> SettingsState {
        let system = FakeSystem::default();
        system.device().failing = vec![failing];
        let wanted = stored(4, wireless_mode(11));
        let unconfirmed = SettingsState {
            version: 4,
            reverted: false,
            disruptions: 1,
            backup: Some(SettingsBackup {
                driver: Some("options rtl8822ce rtw_wireless_mode=95\n".to_owned()),
                nm: None,
                disruption: Disruption::ReloadDriver,
                roam_flags: None,
                roam_tuning: None,
                bgscan: Some("simple:30:-70:86400".to_owned()),
            }),
        };
        let state = initial_state(Some(&wanted), Ok(Some(unconfirmed)));
        let mut actor = actor_with(system.clone(), Some(wanted), state, mpsc::channel(8).1);
        actor.revert_unconfirmed().await;
        let steps = system.device().steps.clone();
        assert!(steps.iter().any(|s| s.starts_with(failing)), "{steps:?}");
        actor.state.unwrap()
    }

    #[tokio::test]
    async fn a_file_that_is_not_restored_keeps_the_backup_for_the_next_start() {
        let state = revert_at_start_failing("File(Driver").await;
        assert!(state.reverted);
        assert!(state.backup.is_some());
    }

    #[tokio::test]
    async fn a_live_value_that_is_not_restored_does_not_keep_the_backup() {
        let state = revert_at_start_failing("Bgscan").await;
        assert!(state.reverted);
        assert!(state.backup.is_none());
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
            backup: None,
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
    async fn a_reverted_version_is_not_applied_and_keeps_reporting_its_revert() {
        let system = FakeSystem::default();
        system.device().report.hostname = Some("orin-1".to_owned());
        // The revert leaves hostname as applied and never touched an unmanaged group.
        let settings = WantedSettings {
            hostname: Some(Managed::Set("byag-12".to_owned())),
            roam_flags: Some(Managed::Unmanaged),
            ..wireless_mode(11)
        };
        let wanted = stored(4, settings);
        let reverted = SettingsState {
            version: 4,
            reverted: true,
            disruptions: 1,
            backup: None,
        };
        let state = initial_state(Some(&wanted), Ok(Some(reverted)));
        let mut actor = actor_with(system.clone(), Some(wanted), state, mpsc::channel(8).1);
        let report = check(&mut actor).await;
        assert!(system.device().steps.is_empty());
        assert_eq!(report.applied_version, None);
        assert_eq!(
            report.apply_status,
            [SettingStatus {
                setting: DeviceSetting::WirelessMode,
                state: SettingApplyState::Reverted,
                reason: REVERT_REASON.to_owned(),
            }]
        );
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

    #[tokio::test]
    async fn unreadable_owned_files_apply_nothing_and_report_no_applied_version() {
        let system = FakeSystem::default();
        system.device().files_unreadable = true;
        system.device().report.hostname = Some("orin-1".to_owned());
        let mut actor = actor(system.clone(), Some(stored(4, hostname("byag-12"))));
        let report = check(&mut actor).await;
        assert!(system.device().steps.is_empty());
        assert_eq!(report.applied_version, None);
    }

    fn ntp_servers() -> WantedSettings {
        WantedSettings {
            ntp_servers: Some(Managed::Set(vec!["10.122.0.1".to_owned()])),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn ntp_servers_without_an_ntp_daemon_fail_at_once_and_spend_no_attempt() {
        let system = FakeSystem::default();
        let mut actor = actor(system, Some(stored(4, ntp_servers())));
        let failed: Vec<_> = check(&mut actor)
            .await
            .apply_status
            .into_iter()
            .map(|s| s.setting)
            .collect();
        assert_eq!(failed, [DeviceSetting::NtpServers]);
        assert!(actor.attempts.is_empty());
    }

    #[tokio::test]
    async fn ntp_servers_are_applied_once_an_ntp_daemon_appears() {
        let system = FakeSystem::default();
        let mut actor = actor(system.clone(), Some(stored(4, ntp_servers())));
        for _ in 0..3 {
            check(&mut actor).await;
        }
        system.device().ntp_daemon = Some(NtpDaemon::Chrony);
        check(&mut actor).await;
        assert!(
            system
                .device()
                .steps
                .contains(&"ReloadNtp(Chrony)".to_owned()),
            "steps: {:?}",
            system.device().steps
        );
    }

    #[tokio::test]
    async fn an_apply_whose_set_cannot_be_stored_is_refused() {
        let system = FakeSystem::default();
        system.device().report.hostname = Some("orin-1".to_owned());
        let mut actor = actor(system.clone(), Some(stored(3, hostname("orin-1"))));
        system.device().wanted_unwritable = true;
        let SafeCommandRx::ApplySettingsResult {
            applied_version,
            report,
            ..
        } = actor.apply(4, hostname("byag-12")).await
        else {
            panic!("expected ApplySettingsResult");
        };
        assert!(system.device().steps.is_empty());
        assert_eq!((applied_version, report.applied_version), (3, Some(3)));
        assert_eq!(
            report.apply_status,
            [SettingStatus {
                setting: DeviceSetting::Hostname,
                state: SettingApplyState::Failed,
                reason: "could not store settings: EROFS".to_owned(),
            }]
        );
        assert_eq!(
            actor.wanted.map(|w| w.version),
            Some(3),
            "previous set kept"
        );
    }

    #[tokio::test]
    async fn an_ntp_detection_error_spends_no_ntp_attempt() {
        let system = FakeSystem::default();
        system.device().ntp_detection_fails = true;
        let mut actor = actor(system, Some(stored(4, ntp_servers())));
        for _ in 0..3 {
            check(&mut actor).await;
        }
        assert!(actor.attempts.is_empty());
    }

    #[test]
    fn a_state_of_another_version_starts_fresh() {
        let wanted = stored(5, WantedSettings::default());
        let old = SettingsState {
            version: 4,
            reverted: true,
            disruptions: 1,
            backup: None,
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
