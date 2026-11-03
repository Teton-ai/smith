//! Link health actor: brings back two Wi-Fi states a device never leaves by
//! itself, before Police has to reboot it.
//!
//! - **Roam buffer stall** (rtl8822ce vendor driver): after a driver-initiated
//!   reassociation, `roam_buf_pkt` can stay set, parking every outgoing frame
//!   while the link still looks connected. Only a disconnect flushes it.
//! - **No-secrets lockout**: when an AP refuses the association, NM can fail
//!   the device with `NO_SECRETS` and then block autoconnect for good.
//!
//! Recovery goes through nmcli, waits while the commander or the settings
//! actor changes networks, and holds them off while it runs (see
//! [`NetGuard`]). Everything is logged on target
//! `link_health`; nothing is reported to the api.

mod detect;
mod escalate;

use crate::commander::network::{associated_bssid, execute_nmcli_command};
use crate::police::PoliceHandle;
use crate::settings::driver::{
    DriverPaths, RELOADABLE_WIFI_MODULES, VENDOR_MODULE, WifiInterface, wifi_interface,
};
use crate::shutdown::ShutdownSignals;
use crate::utils::net_guard::NetGuard;
use anyhow::{Context, Result, bail};
use detect::{LockoutNote, LockoutTracker, RoamDetector, RoamEvent, RoamLimit, parse_trx_info};
use escalate::Escalation;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant, MissedTickBehavior, interval, timeout};
use tracing::{debug, info, warn};

const TICK: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// NM keeps moving for a while after the commander's nmcli calls return.
const NETWORK_SETTLE: Duration = Duration::from_secs(60);
/// `nmcli connection up` waits 90 s by default, past the 60 s cap of
/// `execute_nmcli_command`.
const CONNECTION_UP_WAIT: &str = "45";
/// A driver reload or rename is rare; failed reads also force a fresh lookup.
const IFACE_REFRESH: Duration = Duration::from_secs(5 * 60);
/// A reconnect can emit a dozen state changes while the actor is busy in
/// nmcli; the lockout check re-reads association, so a drop is not fatal.
const EVENT_QUEUE: usize = 64;

/// A Wi-Fi device `StateChanged(new, old, reason)` signal from NetworkManager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceStateEvent {
    pub new: u32,
    pub old: u32,
    pub reason: u32,
    /// Read from the device's current state when subscribing, not a signal.
    pub seeded: bool,
}

/// The device side effects, behind a trait so the actor can be tested.
trait Recovery {
    async fn associated_bssid(&self, iface: &str) -> Option<String>;
    async fn reconnect(&self, iface: &str) -> Result<()>;
    async fn connect(&self, iface: &str) -> Result<()>;
    async fn reload_driver(&self, paths: &DriverPaths, module: &str, iface: &str) -> Result<()>;
}

struct Nmcli;

async fn nmcli(args: &[&str]) -> Result<()> {
    let mut cmd = Command::new("nmcli");
    cmd.args(args);
    let output = execute_nmcli_command(cmd).await?;
    if !output.status.success() {
        bail!(
            "nmcli {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// The UUID of the profile active on `iface`, from
/// `nmcli -g GENERAL.CON-UUID device show`; empty or `--` when none is.
fn parse_con_uuid(output: &str) -> Option<String> {
    let uuid = output.trim();
    (!uuid.is_empty() && uuid != "--").then(|| uuid.to_owned())
}

async fn active_connection(iface: &str) -> Result<String> {
    let mut cmd = Command::new("nmcli");
    cmd.args(["-g", "GENERAL.CON-UUID", "device", "show", iface]);
    let output = execute_nmcli_command(cmd).await?;
    if !output.status.success() {
        bail!(
            "nmcli device show {iface} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    parse_con_uuid(&String::from_utf8_lossy(&output.stdout))
        .with_context(|| format!("no profile active on {iface}"))
}

fn connection_up_args<'a>(uuid: &'a str, iface: &'a str) -> [&'a str; 8] {
    [
        "--wait",
        CONNECTION_UP_WAIT,
        "connection",
        "up",
        "uuid",
        uuid,
        "ifname",
        iface,
    ]
}

/// Every path through a recovery leaves the device with autoconnect on, so NM
/// keeps retrying by itself when our own connect fails.
async fn enable_autoconnect(iface: &str) {
    if let Err(err) = nmcli(&["device", "set", iface, "autoconnect", "yes"]).await {
        warn!(target: "link_health", "Re-enabling autoconnect on {iface} failed: {err:#}");
    }
}

impl Recovery for Nmcli {
    async fn associated_bssid(&self, iface: &str) -> Option<String> {
        let output = Command::new("iw")
            .args(["dev", iface, "link"])
            .kill_on_drop(true)
            .output();
        match timeout(READ_TIMEOUT, output).await {
            Ok(Ok(output)) if output.status.success() => {
                associated_bssid(&String::from_utf8_lossy(&output.stdout))
            }
            Ok(Ok(_)) => None,
            Ok(Err(err)) => {
                warn!(target: "link_health", "iw dev {iface} link failed: {err}");
                None
            }
            Err(_) => {
                warn!(target: "link_health", "iw dev {iface} link timed out");
                None
            }
        }
    }

    /// Re-activating the active profile makes NM tear the link down first, a
    /// local disconnect in the driver that flushes the roam buffer. Unlike
    /// `nmcli device disconnect` it leaves the profile's autoconnect alone,
    /// and unlike `nmcli device connect` it does not need the AP in NM's scan
    /// list: that pair stranded a device until Police (Lake Oswego room 17,
    /// 2026-10-28).
    async fn reconnect(&self, iface: &str) -> Result<()> {
        enable_autoconnect(iface).await;
        let uuid = active_connection(iface).await?;
        nmcli(&connection_up_args(&uuid, iface))
            .await
            .context("connection up")
    }

    async fn connect(&self, iface: &str) -> Result<()> {
        enable_autoconnect(iface).await;
        nmcli(&["device", "connect", iface]).await
    }

    async fn reload_driver(&self, paths: &DriverPaths, module: &str, iface: &str) -> Result<()> {
        crate::settings::reload_driver(paths, module, iface).await
    }
}

pub struct LinkHealthHandle {
    device_events: mpsc::Sender<DeviceStateEvent>,
}

impl LinkHealthHandle {
    pub fn new(shutdown: ShutdownSignals, net_guard: NetGuard, police: PoliceHandle) -> Self {
        let (device_events, receiver) = mpsc::channel(EVENT_QUEUE);
        let mut actor = LinkHealth::new(net_guard, Nmcli, DriverPaths::default(), police);
        tokio::spawn(async move { actor.run(shutdown, receiver).await });
        Self { device_events }
    }

    /// Where the NM watcher forwards Wi-Fi device state changes.
    pub fn device_events(&self) -> mpsc::Sender<DeviceStateEvent> {
        self.device_events.clone()
    }
}

struct LinkHealth<R> {
    net_guard: NetGuard,
    recovery: R,
    paths: DriverPaths,
    police: PoliceHandle,
    escalation: Escalation,
    roam: RoamDetector,
    lockout: LockoutTracker,
    iface: Option<WifiInterface>,
    iface_checked: Option<Instant>,
    /// Say once per boot why the roam detector is idle.
    roam_unavailable_logged: bool,
    /// Say once per busy period that a recovery waits for the network guard.
    guard_wait_logged: bool,
}

impl<R: Recovery> LinkHealth<R> {
    fn new(net_guard: NetGuard, recovery: R, paths: DriverPaths, police: PoliceHandle) -> Self {
        Self {
            net_guard,
            recovery,
            paths,
            police,
            escalation: Escalation::default(),
            roam: RoamDetector::default(),
            lockout: LockoutTracker::default(),
            iface: None,
            iface_checked: None,
            roam_unavailable_logged: false,
            guard_wait_logged: false,
        }
    }

    async fn run(
        &mut self,
        shutdown: ShutdownSignals,
        mut events: mpsc::Receiver<DeviceStateEvent>,
    ) {
        let mut tick = interval(TICK);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        info!(target: "link_health", "Link health watching Wi-Fi");

        loop {
            tokio::select! {
                biased;
                _ = shutdown.token.cancelled() => break,
                Some(event) = events.recv() => self.on_device_state(event),
                _ = tick.tick() => self.on_tick().await,
            }
        }

        info!(target: "link_health", "Link health shut down");
    }

    fn on_device_state(&mut self, event: DeviceStateEvent) {
        debug!(
            target: "link_health",
            old = event.old,
            new = event.new,
            reason = event.reason,
            "Wi-Fi device state changed"
        );
        let now = Instant::now();
        let note = if event.seeded {
            self.lockout.on_initial_state(event.new, event.reason, now)
        } else {
            self.lockout.on_state_changed(event.new, event.reason, now)
        };
        match note {
            LockoutNote::None => {}
            LockoutNote::Started => info!(
                target: "link_health",
                kind = "no_secrets",
                old = event.old,
                new = event.new,
                "Wi-Fi failed with no secrets; NM will not retry it by itself"
            ),
            LockoutNote::Recovered { .. } => log_lockout_recovered(note),
            LockoutNote::Cancelled { elapsed } => info!(
                target: "link_health",
                kind = "no_secrets",
                outcome = "cancelled",
                elapsed_s = elapsed.as_secs(),
                "Wi-Fi disconnected on request; not reconnecting it"
            ),
            LockoutNote::OtherFailure { reason } => info!(
                target: "link_health",
                old = event.old,
                reason,
                "Wi-Fi device failed"
            ),
        }
    }

    async fn on_tick(&mut self) {
        let Some(iface) = self.wifi_interface().await else {
            return;
        };
        if self.net_guard.change_quiet(NETWORK_SETTLE) {
            self.guard_wait_logged = false;
        }

        let lockout_acted = if self.lockout.due(Instant::now()) {
            self.recover_lockout(&iface.name).await
        } else {
            false
        };

        self.check_escalation(&iface, lockout_acted).await;

        let read = if iface.module == VENDOR_MODULE {
            self.read_roam_buf(&iface.name).await
        } else {
            None
        };
        match self.roam.observe(read, Instant::now()) {
            RoamEvent::Idle => {}
            RoamEvent::Stall { count } => self.recover_roam_stall(&iface.name, count).await,
            RoamEvent::Limited(limit) => info!(
                target: "link_health",
                kind = "roam_stall",
                outcome = "skipped_rate_limit",
                limit = match limit {
                    RoamLimit::Spacing => "spacing",
                    RoamLimit::HourlyCap => "hourly_cap",
                },
                "Roam buffer stuck but recovery is rate limited"
            ),
            RoamEvent::Recovered { elapsed } => self.roam_cleared(&iface.name, elapsed).await,
            RoamEvent::StillStuck { count } => warn!(
                target: "link_health",
                kind = "roam_stall",
                outcome = "still_stuck",
                roam_buf_count = count,
                "Roam buffer still stuck after reconnect; leaving it to Police"
            ),
        }
    }

    /// The cached Wi-Fi interface, looked up again every few minutes or after
    /// a failed read.
    async fn wifi_interface(&mut self) -> Option<WifiInterface> {
        let fresh = self
            .iface_checked
            .is_some_and(|at| at.elapsed() < IFACE_REFRESH);
        if self.iface.is_none() || !fresh {
            self.iface = wifi_interface(&self.paths.net).await;
            self.iface_checked = Some(Instant::now());
        }
        self.iface.clone()
    }

    async fn read_roam_buf(&mut self, iface: &str) -> Option<detect::RoamBuf> {
        let path = self.paths.proc.join(iface).join("trx_info");
        let text = match timeout(READ_TIMEOUT, tokio::fs::read_to_string(&path)).await {
            Ok(Ok(text)) => text,
            Ok(Err(err)) => {
                self.iface = None;
                self.roam_unavailable(&format!("reading {}: {err}", path.display()));
                return None;
            }
            Err(_) => {
                warn!(target: "link_health", "Reading {} timed out", path.display());
                return None;
            }
        };
        let read = parse_trx_info(&text);
        if read.is_none() {
            self.roam_unavailable("trx_info has no roam_buf_pkt line");
        }
        read
    }

    fn roam_unavailable(&mut self, why: &str) {
        if !self.roam_unavailable_logged {
            self.roam_unavailable_logged = true;
            info!(target: "link_health", "Roam stall detection unavailable: {why}");
        } else {
            debug!(target: "link_health", "Roam stall detection unavailable: {why}");
        }
    }

    /// Returns whether it called `connect()`.
    async fn recover_lockout(&mut self, iface: &str) -> bool {
        let Some(_recovering) = self.net_guard.try_recovery(NETWORK_SETTLE) else {
            self.wait_for_guard("no_secrets");
            return false;
        };
        // The ACTIVATED event that ends an incident can be dropped when the
        // queue is full; never reconnect a link that is already up.
        if self.recovery.associated_bssid(iface).await.is_some() {
            log_lockout_recovered(self.lockout.resolved(Instant::now()));
            return false;
        }

        let result = self.recovery.connect(iface).await;
        self.lockout.attempted(Instant::now());
        match result {
            Ok(()) => info!(
                target: "link_health",
                kind = "no_secrets",
                action = "activate",
                iface,
                "Asked NM to connect Wi-Fi"
            ),
            Err(err) => warn!(
                target: "link_health",
                kind = "no_secrets",
                action = "activate",
                "Wi-Fi activation failed: {err:#}"
            ),
        }
        true
    }

    /// Escalates while Police has a reboot pending: reassociate at 5 minutes, reload at 15.
    /// A `connect()` by `recover_lockout` this tick already counts as the reassociation.
    async fn check_escalation(&mut self, iface: &WifiInterface, lockout_acted: bool) {
        let status = self.police.status().await;
        let schedule_id = status.schedule_id.unwrap_or_default();
        let elapsed_s = status.elapsed_seconds;
        match self.escalation.due(&status) {
            escalate::Due::Nothing => {}
            escalate::Due::Reassociate if lockout_acted => {
                self.escalation.mark(escalate::Step::Reassociated);
                log_escalation_step(
                    "reassociate",
                    "merged_with_lockout",
                    schedule_id,
                    elapsed_s,
                    format_args!(
                        "No-secrets recovery already reassociated; skipping a separate reconnect"
                    ),
                );
            }
            escalate::Due::Reassociate => {
                let Some(_recovering) = self.net_guard.try_recovery(NETWORK_SETTLE) else {
                    self.wait_for_guard("api_unreachable");
                    return;
                };
                let result = self.recovery.reconnect(&iface.name).await;
                self.escalation.mark(escalate::Step::Reassociated);
                match result {
                    Ok(()) => log_escalation_step(
                        "reassociate",
                        "ok",
                        schedule_id,
                        elapsed_s,
                        format_args!("Reassociated ahead of the Police reboot"),
                    ),
                    Err(err) => log_escalation_step(
                        "reassociate",
                        "failed",
                        schedule_id,
                        elapsed_s,
                        format_args!("Reassociation failed: {err:#}"),
                    ),
                }
            }
            escalate::Due::Reload if !RELOADABLE_WIFI_MODULES.contains(&iface.module.as_str()) => {
                self.escalation.mark(escalate::Step::Reloaded);
                info!(
                    target: "link_health",
                    kind = "api_unreachable",
                    step = "reload",
                    outcome = "skipped_unsupported_driver",
                    schedule_id,
                    elapsed_s,
                    module = %iface.module,
                    "Driver reload not supported on this module"
                );
            }
            escalate::Due::Reload => {
                let Some(_recovering) = self.net_guard.try_recovery(NETWORK_SETTLE) else {
                    self.wait_for_guard("api_unreachable");
                    return;
                };
                // NetGuard is advisory on our side only: the commander never
                // checks it before running a network command, so one landing
                // during this ~150s reload targets an interface that is
                // briefly gone (rmmod'd) and will fail outright. Accepted:
                // closing this needs the commander to defer its own network
                // commands, which is a separate, larger change.
                let result = self
                    .recovery
                    .reload_driver(&self.paths, &iface.module, &iface.name)
                    .await;
                self.escalation.mark(escalate::Step::Reloaded);
                match result {
                    Ok(()) => log_escalation_step(
                        "reload",
                        "ok",
                        schedule_id,
                        elapsed_s,
                        format_args!("Reloaded the Wi-Fi driver ahead of the Police reboot"),
                    ),
                    Err(err) => log_escalation_step(
                        "reload",
                        "failed",
                        schedule_id,
                        elapsed_s,
                        format_args!("Driver reload failed: {err:#}"),
                    ),
                }
            }
        }
    }

    async fn recover_roam_stall(&mut self, iface: &str, count: u32) {
        let Some(_recovering) = self.net_guard.try_recovery(NETWORK_SETTLE) else {
            self.wait_for_guard("roam_stall");
            return;
        };
        let Some(bssid) = self.recovery.associated_bssid(iface).await else {
            debug!(target: "link_health", "Roam buffer stuck while not associated; checking again later");
            self.roam.defer(Instant::now());
            return;
        };

        info!(
            target: "link_health",
            kind = "roam_stall",
            action = "reconnect",
            bssid = %bssid,
            roam_buf_count = count,
            "Roam buffer stuck; reconnecting Wi-Fi"
        );
        if let Err(err) = self.recovery.reconnect(iface).await {
            warn!(
                target: "link_health",
                kind = "roam_stall",
                action = "reconnect",
                "Wi-Fi reconnect failed: {err:#}"
            );
        }
        // Counted even when nmcli failed, so a broken reconnect still obeys
        // the spacing and the hourly cap.
        self.roam.recovered(Instant::now());
    }

    /// A down interface also reads as a cleared buffer, so the link is
    /// checked before calling it recovered.
    async fn roam_cleared(&self, iface: &str, elapsed: Duration) {
        if self.recovery.associated_bssid(iface).await.is_some() {
            info!(
                target: "link_health",
                kind = "roam_stall",
                outcome = "recovered",
                elapsed_s = elapsed.as_secs(),
                "Roam buffer cleared"
            );
        } else {
            warn!(
                target: "link_health",
                kind = "roam_stall",
                outcome = "cleared_not_connected",
                elapsed_s = elapsed.as_secs(),
                "Roam buffer cleared but Wi-Fi is not connected"
            );
        }
    }

    fn wait_for_guard(&mut self, kind: &'static str) {
        if !self.guard_wait_logged {
            self.guard_wait_logged = true;
            info!(
                target: "link_health",
                kind,
                outcome = "skipped_guard",
                "Network command in progress; recovery deferred"
            );
        }
    }
}

/// Logs one Police-countdown escalation step's outcome on target `link_health`.
fn log_escalation_step(
    step: &'static str,
    outcome: &'static str,
    schedule_id: u32,
    elapsed_s: u64,
    message: std::fmt::Arguments<'_>,
) {
    if outcome == "failed" {
        warn!(
            target: "link_health",
            kind = "api_unreachable",
            step,
            outcome,
            schedule_id,
            elapsed_s,
            "{message}"
        );
    } else {
        info!(
            target: "link_health",
            kind = "api_unreachable",
            step,
            outcome,
            schedule_id,
            elapsed_s,
            "{message}"
        );
    }
}

fn log_lockout_recovered(note: LockoutNote) {
    if let LockoutNote::Recovered { elapsed, attempts } = note {
        info!(
            target: "link_health",
            kind = "no_secrets",
            outcome = "recovered",
            elapsed_s = elapsed.as_secs(),
            attempts,
            "Wi-Fi activated again"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::driver::tests::fake_iface;
    use crate::shutdown::ShutdownHandler;
    use futures::FutureExt;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    fn police_handle() -> PoliceHandle {
        PoliceHandle::new(ShutdownHandler::new().signals())
    }

    /// Police only arms after `ARM_AFTER` of uptime, so the paused clock must pass it first.
    async fn pending_reboot() -> (PoliceHandle, u32) {
        let police = police_handle();
        // A round-trip guarantees the actor has started before the clock
        // advances past its arming interval.
        let _ = police.status().await;
        tokio::time::advance(crate::police::ARM_AFTER + Duration::from_secs(1)).await;
        for _ in 0..100 {
            if let Some(id) = police.report_problem_starting().await {
                return (police, id);
            }
            tokio::task::yield_now().await;
        }
        panic!("police did not arm after ARM_AFTER");
    }

    #[derive(Default)]
    struct Fake {
        associated: AtomicBool,
        association_checks: AtomicU32,
        reconnects: AtomicU32,
        connects: AtomicU32,
        reloads: AtomicU32,
        /// The reconnect leaves the device unassociated, as on 2026-10-28.
        reconnect_drops_link: AtomicBool,
        reload_fails: AtomicBool,
        /// Probed during the reconnect to see whether a settings change could start.
        guard: Option<NetGuard>,
        change_blocked_during_reconnect: AtomicBool,
    }

    impl Fake {
        fn associated() -> Self {
            let fake = Self::default();
            fake.associated.store(true, Ordering::SeqCst);
            fake
        }
    }

    impl Recovery for &Fake {
        async fn associated_bssid(&self, _iface: &str) -> Option<String> {
            self.association_checks.fetch_add(1, Ordering::SeqCst);
            self.associated
                .load(Ordering::SeqCst)
                .then(|| "36:70:4e:4e:60:01".to_owned())
        }
        async fn reconnect(&self, _iface: &str) -> Result<()> {
            self.reconnects.fetch_add(1, Ordering::SeqCst);
            if let Some(guard) = &self.guard {
                let blocked = guard
                    .hold_after_recovery(Duration::ZERO)
                    .now_or_never()
                    .is_none();
                self.change_blocked_during_reconnect
                    .store(blocked, Ordering::SeqCst);
            }
            if self.reconnect_drops_link.load(Ordering::SeqCst) {
                self.associated.store(false, Ordering::SeqCst);
            }
            Ok(())
        }
        async fn connect(&self, _iface: &str) -> Result<()> {
            self.connects.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn reload_driver(
            &self,
            _paths: &DriverPaths,
            _module: &str,
            _iface: &str,
        ) -> Result<()> {
            self.reloads.fetch_add(1, Ordering::SeqCst);
            if self.reload_fails.load(Ordering::SeqCst) {
                bail!("fake reload failure");
            }
            Ok(())
        }
    }

    fn paths(root: &Path) -> DriverPaths {
        DriverPaths {
            net: root.join("net"),
            params: root.join("params"),
            proc: root.join("proc"),
        }
    }

    /// A `LinkHealth` with a fresh `NetGuard` and no reboot ever pending, for
    /// tests that don't need a custom guard or `PoliceHandle`.
    fn lh<'a>(root: &Path, fake: &'a Fake) -> LinkHealth<&'a Fake> {
        LinkHealth::new(NetGuard::new(), fake, paths(root), police_handle())
    }

    fn write_trx(root: &Path, line: &str) {
        let dir = root.join("proc/wlP1p1s0");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("trx_info"),
            format!("free_xmitframe_cnt=236\n{line}\n"),
        )
        .unwrap();
    }

    fn no_secrets() -> DeviceStateEvent {
        DeviceStateEvent {
            new: detect::NM_DEVICE_STATE_FAILED,
            old: 50,
            reason: detect::NM_DEVICE_STATE_REASON_NO_SECRETS,
            seeded: false,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn two_stuck_reads_reconnect_once() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtl88x2ce", VENDOR_MODULE);
        write_trx(root, "TX: roam_buf_pkt=1:20");
        let fake = Fake::associated();
        let mut lh = lh(root, &fake);

        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 0);
        tokio::time::advance(TICK).await;
        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 1);
        tokio::time::advance(TICK).await;
        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_held_guard_defers_the_reconnect() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtl88x2ce", VENDOR_MODULE);
        write_trx(root, "TX: roam_buf_pkt=1:20");
        let fake = Fake::associated();
        let guard = NetGuard::new();
        let hold = guard.hold();
        let mut lh = LinkHealth::new(guard.clone(), &fake, paths(root), police_handle());

        lh.on_tick().await;
        tokio::time::advance(TICK).await;
        lh.on_tick().await;
        assert!(lh.guard_wait_logged);
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 0);

        drop(hold);
        tokio::time::advance(NETWORK_SETTLE).await;
        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 1);
        assert!(!lh.guard_wait_logged);
    }

    #[tokio::test(start_paused = true)]
    async fn a_reconnect_holds_off_settings_changes_while_it_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtl88x2ce", VENDOR_MODULE);
        write_trx(root, "TX: roam_buf_pkt=1:20");
        let guard = NetGuard::new();
        let fake = Fake {
            guard: Some(guard.clone()),
            ..Fake::associated()
        };
        let mut lh = LinkHealth::new(guard, &fake, paths(root), police_handle());

        lh.on_tick().await;
        tokio::time::advance(TICK).await;
        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 1);
        assert!(fake.change_blocked_during_reconnect.load(Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn idles_without_the_vendor_proc_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtl88x2ce", VENDOR_MODULE);
        let fake = Fake::associated();
        let mut lh = lh(root, &fake);
        for _ in 0..4 {
            lh.on_tick().await;
            tokio::time::advance(TICK).await;
        }
        assert!(lh.roam_unavailable_logged);
        assert_eq!(fake.association_checks.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_stall_without_association_rechecks_once_a_minute() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtl88x2ce", VENDOR_MODULE);
        write_trx(root, "TX: roam_buf_pkt=1:20");
        let fake = Fake::default();
        let mut lh = lh(root, &fake);

        // Two reads to declare the stall, then 2 more minutes of ticks.
        for _ in 0..10 {
            lh.on_tick().await;
            tokio::time::advance(TICK).await;
        }
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 0);
        assert_eq!(fake.association_checks.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn no_secrets_lockout_activates_after_two_minutes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtw_8822ce", "rtw88_8822ce");
        let fake = Fake::default();
        let mut lh = lh(root, &fake);

        lh.on_device_state(no_secrets());
        lh.on_tick().await;
        assert_eq!(fake.connects.load(Ordering::SeqCst), 0);
        tokio::time::advance(Duration::from_secs(120)).await;
        lh.on_tick().await;
        assert_eq!(fake.connects.load(Ordering::SeqCst), 1);
        tokio::time::advance(TICK).await;
        lh.on_tick().await;
        assert_eq!(fake.connects.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_device_found_disconnected_at_startup_is_activated() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtw_8822ce", "rtw88_8822ce");
        let fake = Fake::default();
        let mut lh = lh(root, &fake);

        lh.on_device_state(DeviceStateEvent {
            new: detect::NM_DEVICE_STATE_DISCONNECTED,
            old: detect::NM_DEVICE_STATE_DISCONNECTED,
            reason: 0,
            seeded: true,
        });
        tokio::time::advance(Duration::from_secs(120)).await;
        lh.on_tick().await;
        assert_eq!(fake.connects.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_user_disconnect_stops_the_lockout_recovery() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtw_8822ce", "rtw88_8822ce");
        let fake = Fake::default();
        let mut lh = lh(root, &fake);

        lh.on_device_state(no_secrets());
        lh.on_device_state(DeviceStateEvent {
            new: detect::NM_DEVICE_STATE_DISCONNECTED,
            old: detect::NM_DEVICE_STATE_FAILED,
            reason: detect::NM_DEVICE_STATE_REASON_USER_REQUESTED,
            seeded: false,
        });
        tokio::time::advance(Duration::from_secs(60 * 60)).await;
        lh.on_tick().await;
        assert_eq!(fake.connects.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_lockout_that_already_cleared_is_not_reconnected() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtw_8822ce", "rtw88_8822ce");
        let fake = Fake::default();
        let mut lh = lh(root, &fake);

        lh.on_device_state(no_secrets());
        // The ACTIVATED event never arrives, but the link is back.
        fake.associated.store(true, Ordering::SeqCst);
        tokio::time::advance(Duration::from_secs(120)).await;
        lh.on_tick().await;
        tokio::time::advance(Duration::from_secs(30 * 60)).await;
        lh.on_tick().await;
        assert_eq!(fake.connects.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn reads_the_active_profile_uuid() {
        assert_eq!(
            parse_con_uuid("3aaa88cb-19a7-4b0e-9f66-c176c4473df9\n").as_deref(),
            Some("3aaa88cb-19a7-4b0e-9f66-c176c4473df9")
        );
        assert_eq!(parse_con_uuid("--\n"), None);
        assert_eq!(parse_con_uuid(""), None);
    }

    #[test]
    fn reconnects_by_profile_without_a_device_disconnect() {
        let args = connection_up_args("3aaa88cb", "wlP1p1s0");
        assert_eq!(
            args,
            [
                "--wait",
                "45",
                "connection",
                "up",
                "uuid",
                "3aaa88cb",
                "ifname",
                "wlP1p1s0"
            ]
        );
        assert!(!args.contains(&"disconnect"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_cleared_buffer_is_checked_against_the_link() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtl88x2ce", VENDOR_MODULE);
        write_trx(root, "TX: roam_buf_pkt=1:20");
        let fake = Fake::associated();
        fake.reconnect_drops_link.store(true, Ordering::SeqCst);
        let mut lh = lh(root, &fake);

        lh.on_tick().await;
        tokio::time::advance(TICK).await;
        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 1);
        assert_eq!(fake.association_checks.load(Ordering::SeqCst), 1);

        // The interface is down, so the driver reports an empty buffer.
        write_trx(root, "TX: roam_buf_pkt=0:0");
        tokio::time::advance(TICK).await;
        lh.on_tick().await;
        assert_eq!(fake.association_checks.load(Ordering::SeqCst), 2);
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 1);
    }

    /// Platform A (L4T 35, driver v5.9) names the interface `wlan0` and its
    /// `trx_info` has no `roam_buf_pkt` line, so the roam path stays idle.
    #[tokio::test(start_paused = true)]
    async fn platform_a_wlan0_without_a_roam_buf_line_never_reconnects() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlan0", "rtl88x2ce", VENDOR_MODULE);
        let dir = root.join("proc/wlan0");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("trx_info"), "free_xmitframe_cnt=256\n").unwrap();
        let fake = Fake::associated();
        let mut lh = lh(root, &fake);
        for _ in 0..4 {
            lh.on_tick().await;
            tokio::time::advance(TICK).await;
        }
        assert_eq!(lh.iface.as_ref().map(|i| i.name.as_str()), Some("wlan0"));
        assert!(lh.roam_unavailable_logged);
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn reassociation_fires_once_at_five_minutes_on_a_pending_reboot() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtl88x2ce", VENDOR_MODULE);
        let fake = Fake::associated();
        let (police, _id) = pending_reboot().await;
        let mut lh = LinkHealth::new(NetGuard::new(), &fake, paths(root), police);

        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 0);

        tokio::time::advance(escalate::REASSOCIATE_AT).await;
        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 1);

        tokio::time::advance(TICK).await;
        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn reload_fires_once_at_fifteen_minutes_after_reassociation_already_ran() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtl88x2ce", VENDOR_MODULE);
        let fake = Fake::associated();
        let (police, _id) = pending_reboot().await;
        let mut lh = LinkHealth::new(NetGuard::new(), &fake, paths(root), police);

        tokio::time::advance(escalate::REASSOCIATE_AT).await;
        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 1);

        tokio::time::advance(escalate::RELOAD_AT - escalate::REASSOCIATE_AT).await;
        lh.on_tick().await;
        assert_eq!(fake.reloads.load(Ordering::SeqCst), 1);
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_held_guard_defers_the_reassociation_without_refiring_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtl88x2ce", VENDOR_MODULE);
        let fake = Fake::associated();
        let (police, _id) = pending_reboot().await;
        let guard = NetGuard::new();
        let hold = guard.hold();
        let mut lh = LinkHealth::new(guard.clone(), &fake, paths(root), police);

        tokio::time::advance(escalate::REASSOCIATE_AT).await;
        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 0);

        drop(hold);
        tokio::time::advance(NETWORK_SETTLE).await;
        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 1);

        tokio::time::advance(TICK).await;
        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_same_tick_lockout_recovery_satisfies_the_reassociation_step() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtw_8822ce", "rtw88_8822ce");
        let fake = Fake::default();
        let (police, _id) = pending_reboot().await;
        let mut lh = LinkHealth::new(NetGuard::new(), &fake, paths(root), police);

        lh.on_device_state(no_secrets());
        tokio::time::advance(escalate::REASSOCIATE_AT).await;
        lh.on_tick().await;

        assert_eq!(fake.connects.load(Ordering::SeqCst), 1);
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn the_reload_allowlist_covers_the_upstream_rtw88_driver_too() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtw_8822ce", "rtw88_8822ce");
        let fake = Fake::associated();
        let (police, _id) = pending_reboot().await;
        let mut lh = LinkHealth::new(NetGuard::new(), &fake, paths(root), police);

        tokio::time::advance(escalate::REASSOCIATE_AT).await;
        lh.on_tick().await;
        tokio::time::advance(escalate::RELOAD_AT - escalate::REASSOCIATE_AT).await;
        lh.on_tick().await;

        assert_eq!(fake.reloads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_device_on_an_unlisted_driver_skips_the_reload_and_never_retries() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "iwlwifi", "iwlwifi");
        let fake = Fake::associated();
        let (police, _id) = pending_reboot().await;
        let mut lh = LinkHealth::new(NetGuard::new(), &fake, paths(root), police);

        tokio::time::advance(escalate::REASSOCIATE_AT).await;
        lh.on_tick().await;
        tokio::time::advance(escalate::RELOAD_AT - escalate::REASSOCIATE_AT).await;
        lh.on_tick().await;
        assert_eq!(fake.reloads.load(Ordering::SeqCst), 0);

        tokio::time::advance(TICK).await;
        lh.on_tick().await;
        assert_eq!(fake.reloads.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_cancelled_and_rescheduled_reboot_gets_fresh_escalation() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtl88x2ce", VENDOR_MODULE);
        let fake = Fake::associated();
        let (police, id) = pending_reboot().await;
        let mut lh = LinkHealth::new(NetGuard::new(), &fake, paths(root), police.clone());

        tokio::time::advance(escalate::REASSOCIATE_AT).await;
        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 1);

        police.report_problem_solved(id).await;
        tokio::time::advance(TICK).await;
        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 1);

        let new_id = police
            .report_problem_starting()
            .await
            .expect("police stays armed once arming has happened");
        assert_ne!(new_id, id);
        tokio::time::advance(escalate::REASSOCIATE_AT).await;
        lh.on_tick().await;
        assert_eq!(fake.reconnects.load(Ordering::SeqCst), 2);
    }
}
