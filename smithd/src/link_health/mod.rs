//! Link health actor: brings back two Wi-Fi states a device never leaves by
//! itself, before Police has to reboot it.
//!
//! - **Roam buffer stall** (rtl8822ce vendor driver): after a driver-initiated
//!   reassociation, `roam_buf_pkt` can stay set, parking every outgoing frame
//!   while the link still looks connected. Only a disconnect flushes it.
//! - **No-secrets lockout**: when an AP refuses the association, NM can fail
//!   the device with `NO_SECRETS` and then block autoconnect for good.
//!
//! Recovery goes through nmcli and stays out of the way while the commander
//! changes networks (see [`NetGuard`]). Everything is logged on target
//! `link_health`; nothing is reported to the api.

mod detect;

use crate::commander::network::{associated_bssid, execute_nmcli_command};
use crate::settings::driver::{DriverPaths, VENDOR_MODULE, WifiInterface, wifi_interface};
use crate::shutdown::ShutdownSignals;
use crate::utils::net_guard::NetGuard;
use anyhow::{Context, Result, bail};
use detect::{LockoutNote, LockoutTracker, RoamDetector, RoamEvent, RoamLimit, parse_trx_info};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant, MissedTickBehavior, interval, sleep, timeout};
use tracing::{debug, info, warn};

const TICK: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// NM keeps moving for a while after the commander's nmcli calls return.
const NETWORK_SETTLE: Duration = Duration::from_secs(60);
/// Between `nmcli device disconnect` and `connect`, so NM sees a clean stop.
const RECONNECT_PAUSE: Duration = Duration::from_secs(2);
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

    /// `nmcli device disconnect` also turns autoconnect off for the device,
    /// so it is turned back on before anything else can fail.
    async fn reconnect(&self, iface: &str) -> Result<()> {
        let disconnected = nmcli(&["device", "disconnect", iface]).await;
        enable_autoconnect(iface).await;
        disconnected.context("disconnect")?;
        sleep(RECONNECT_PAUSE).await;
        nmcli(&["device", "connect", iface])
            .await
            .context("connect")
    }

    async fn connect(&self, iface: &str) -> Result<()> {
        enable_autoconnect(iface).await;
        nmcli(&["device", "connect", iface]).await
    }
}

pub struct LinkHealthHandle {
    device_events: mpsc::Sender<DeviceStateEvent>,
}

impl LinkHealthHandle {
    pub fn new(shutdown: ShutdownSignals, net_guard: NetGuard) -> Self {
        let (device_events, receiver) = mpsc::channel(EVENT_QUEUE);
        let mut actor = LinkHealth::new(net_guard, Nmcli, DriverPaths::default());
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
    fn new(net_guard: NetGuard, recovery: R, paths: DriverPaths) -> Self {
        Self {
            net_guard,
            recovery,
            paths,
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
        let quiet = self.net_guard.is_quiet(NETWORK_SETTLE);
        if quiet {
            self.guard_wait_logged = false;
        }

        if self.lockout.due(Instant::now()) {
            self.recover_lockout(&iface.name, quiet).await;
        }

        let read = if iface.module == VENDOR_MODULE {
            self.read_roam_buf(&iface.name).await
        } else {
            None
        };
        match self.roam.observe(read, Instant::now()) {
            RoamEvent::Idle => {}
            RoamEvent::Stall { count } => self.recover_roam_stall(&iface.name, count, quiet).await,
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
            RoamEvent::Recovered { elapsed } => info!(
                target: "link_health",
                kind = "roam_stall",
                outcome = "recovered",
                elapsed_s = elapsed.as_secs(),
                "Roam buffer cleared"
            ),
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

    async fn recover_lockout(&mut self, iface: &str, quiet: bool) {
        if !quiet {
            self.wait_for_guard("no_secrets");
            return;
        }
        // The ACTIVATED event that ends an incident can be dropped when the
        // queue is full; never reconnect a link that is already up.
        if self.recovery.associated_bssid(iface).await.is_some() {
            log_lockout_recovered(self.lockout.resolved(Instant::now()));
            return;
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
    }

    async fn recover_roam_stall(&mut self, iface: &str, count: u32, quiet: bool) {
        if !quiet {
            self.wait_for_guard("roam_stall");
            return;
        }
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
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    #[derive(Default)]
    struct Fake {
        associated: AtomicBool,
        association_checks: AtomicU32,
        reconnects: AtomicU32,
        connects: AtomicU32,
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
            Ok(())
        }
        async fn connect(&self, _iface: &str) -> Result<()> {
            self.connects.fetch_add(1, Ordering::SeqCst);
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
        let mut lh = LinkHealth::new(NetGuard::new(), &fake, paths(root));

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
        let mut lh = LinkHealth::new(guard.clone(), &fake, paths(root));

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
    async fn idles_without_the_vendor_proc_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtl88x2ce", VENDOR_MODULE);
        let fake = Fake::associated();
        let mut lh = LinkHealth::new(NetGuard::new(), &fake, paths(root));
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
        let mut lh = LinkHealth::new(NetGuard::new(), &fake, paths(root));

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
        let mut lh = LinkHealth::new(NetGuard::new(), &fake, paths(root));

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
        let mut lh = LinkHealth::new(NetGuard::new(), &fake, paths(root));

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
    async fn a_lockout_that_already_cleared_is_not_reconnected() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fake_iface(root, "wlP1p1s0", "rtw_8822ce", "rtw88_8822ce");
        let fake = Fake::default();
        let mut lh = LinkHealth::new(NetGuard::new(), &fake, paths(root));

        lh.on_device_state(no_secrets());
        // The ACTIVATED event never arrives, but the link is back.
        fake.associated.store(true, Ordering::SeqCst);
        tokio::time::advance(Duration::from_secs(120)).await;
        lh.on_tick().await;
        tokio::time::advance(Duration::from_secs(30 * 60)).await;
        lh.on_tick().await;
        assert_eq!(fake.connects.load(Ordering::SeqCst), 0);
    }
}
