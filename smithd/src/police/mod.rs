//! Police actor
//!
//! Others actors will send messages to the police actor when they think
//! that something is wrong. The police actor will then take action to solve
//! the problem. Right now the only action is to restart the agent.
//!
//! It does this by issuing a delayed restart after `RESTART_DELAY`. If the
//! problem is solved before the restart is issued, the restart is cancelled. 🤞
//!
//! The pending restart is readable via [`PoliceHandle::status`], which the
//! local control socket exposes so other services on the device can react
//! before the reboot lands. Those services can also place a *hold* — a lease
//! that defers the reboot while someone is, say, connected to the debug access
//! point. Holds expire unless renewed: a crashed holder must never disarm the
//! watchdog forever.
//!
use crate::shutdown::ShutdownSignals;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, interval, sleep_until};
use tracing::{error, info, warn};

/// How long after a problem is reported before the device is rebooted. Other
/// services poll the control socket within this window to react in time.
const RESTART_DELAY: Duration = Duration::from_secs(10 * 60);

/// Restarts are only armed once the daemon has been up this long, so a device
/// that boots without connectivity does not reboot-loop.
const ARM_AFTER: Duration = Duration::from_secs(15 * 60);

/// Hold lease granted when the caller does not ask for a specific TTL.
const DEFAULT_HOLD_TTL: Duration = Duration::from_secs(10 * 60);

/// Upper bound on a single hold lease. Long-lived holds must be renewed; this
/// caps how long a vanished holder can keep the watchdog silenced.
const MAX_HOLD_TTL: Duration = Duration::from_secs(10 * 60);

/// Whether a reboot is scheduled, how long is left, and any hold on it.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct RebootStatus {
    pub reboot_pending: bool,
    /// Identity of the pending reboot. A new scheduling event gets a new id, so
    /// pollers can tell "still the same reboot" from "cancelled and rescheduled".
    pub schedule_id: Option<u32>,
    pub seconds_remaining: u64,
    /// Seconds since this reboot was scheduled. Can exceed `delay_seconds` when
    /// holds have deferred the deadline.
    pub elapsed_seconds: u64,
    /// The configured delay between scheduling and reboot.
    pub delay_seconds: u64,
    pub held: bool,
    pub hold_seconds_remaining: u64,
}

struct Police {
    shutdown: ShutdownSignals,
    should_restart: bool,
    restart_at: Option<Instant>,
    scheduled_at: Option<Instant>,
    hold_until: Option<Instant>,
    schedule_seq: u32,
    receiver: mpsc::Receiver<PoliceMessage>,
    next_id: u32,
    problems: Vec<u32>,
}
enum PoliceMessage {
    ProblemStarting {
        respond_to: oneshot::Sender<Option<u32>>,
    },
    ProblemSolved {
        id: u32,
    },
    Status {
        respond_to: oneshot::Sender<RebootStatus>,
    },
    Hold {
        ttl_seconds: Option<u64>,
        respond_to: oneshot::Sender<RebootStatus>,
    },
    ReleaseHold {
        respond_to: oneshot::Sender<RebootStatus>,
    },
}

impl Police {
    fn new(shutdown: ShutdownSignals, receiver: mpsc::Receiver<PoliceMessage>) -> Self {
        Police {
            shutdown,
            should_restart: false,
            restart_at: None,
            scheduled_at: None,
            hold_until: None,
            schedule_seq: 0,
            receiver,
            next_id: 0,
            problems: Vec::new(),
        }
    }

    fn status(&self) -> RebootStatus {
        let now = Instant::now();
        let hold_seconds_remaining = self
            .hold_until
            .map(|h| h.saturating_duration_since(now).as_secs())
            .unwrap_or(0);

        RebootStatus {
            reboot_pending: self.restart_at.is_some(),
            schedule_id: self.restart_at.is_some().then_some(self.schedule_seq),
            seconds_remaining: self
                .restart_at
                .map(|at| at.saturating_duration_since(now).as_secs())
                .unwrap_or(0),
            elapsed_seconds: self
                .scheduled_at
                .map(|at| now.saturating_duration_since(at).as_secs())
                .unwrap_or(0),
            delay_seconds: RESTART_DELAY.as_secs(),
            held: hold_seconds_remaining > 0,
            hold_seconds_remaining,
        }
    }

    fn handle_message(&mut self, msg: PoliceMessage) {
        match msg {
            PoliceMessage::ProblemStarting { respond_to } => {
                // There is no restart scheduled, so we will do it in RESTART_DELAY
                let response = if self.should_restart {
                    self.next_id += 1;
                    self.problems.push(self.next_id);
                    if self.restart_at.is_none() {
                        self.schedule_seq += 1;
                        self.scheduled_at = Some(Instant::now());
                        self.restart_at = Some(Instant::now() + RESTART_DELAY);
                        warn!(
                            "Restarting in {}s (schedule {})",
                            RESTART_DELAY.as_secs(),
                            self.schedule_seq
                        );
                    } else {
                        warn!("Restart already scheduled");
                    }
                    Some(self.next_id)
                } else {
                    warn!("Restart not to be scheduled yet");
                    None
                };

                _ = respond_to.send(response);
            }
            PoliceMessage::ProblemSolved { id } => {
                // pop id from problems
                self.problems.retain(|&x| x != id);

                // If there are no more problems, cancel the restart
                if self.problems.is_empty() && self.restart_at.take().is_some() {
                    info!("Problem solved, restart aborted");
                    self.scheduled_at = None;
                }
            }
            PoliceMessage::Status { respond_to } => {
                _ = respond_to.send(self.status());
            }
            PoliceMessage::Hold {
                ttl_seconds,
                respond_to,
            } => {
                let ttl = ttl_seconds
                    .map(Duration::from_secs)
                    .unwrap_or(DEFAULT_HOLD_TTL)
                    .min(MAX_HOLD_TTL);
                self.hold_until = Some(Instant::now() + ttl);
                info!("Reboot hold placed for {}s", ttl.as_secs());
                _ = respond_to.send(self.status());
            }
            PoliceMessage::ReleaseHold { respond_to } => {
                if self.hold_until.take().is_some() {
                    info!("Reboot hold released");
                }
                _ = respond_to.send(self.status());
            }
        }
    }

    /// The scheduled deadline has arrived: reboot, unless a live hold defers it
    /// to the hold's expiry (where it is re-checked, so a renewed hold keeps
    /// deferring).
    fn fire_or_defer(&mut self) {
        let now = Instant::now();

        if let Some(hold_until) = self.hold_until.filter(|h| *h > now) {
            warn!(
                "Reboot due but held; deferring {}s",
                hold_until.saturating_duration_since(now).as_secs()
            );
            self.restart_at = Some(hold_until);
            return;
        }

        error!("Restarting now!");
        self.restart_at = None;
        self.scheduled_at = None;

        // Tests drive the clock across this deadline; a test binary must record
        // the firing instead of executing a real reboot.
        #[cfg(test)]
        {
            tests::REBOOT_FIRED.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        #[cfg(not(test))]
        {
            if let Err(e) = tokio::process::Command::new("reboot").arg("now").spawn() {
                error!("Failed to spawn reboot command: {e}");
            }
        }
    }

    async fn run(&mut self) {
        info!("Police runnning");

        let mut enable_by_default = interval(ARM_AFTER);

        // the first tick is immediate
        enable_by_default.tick().await;

        loop {
            // Inert placeholder when nothing is scheduled; the `if` guard keeps
            // the branch from firing anyway.
            let deadline = self
                .restart_at
                .unwrap_or_else(|| Instant::now() + ARM_AFTER);

            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    self.handle_message(msg);
                }
                _ = enable_by_default.tick() => {
                    info!("Enabling police restarts by default");
                    self.should_restart = true;
                }
                _ = sleep_until(deadline), if self.restart_at.is_some() => {
                    self.fire_or_defer();
                }
                _ = self.shutdown.token.cancelled() => {
                    break;
                }
            }
        }

        info!("Bouncer task shut down");
    }
}

#[derive(Clone)]
pub struct PoliceHandle {
    sender: mpsc::Sender<PoliceMessage>,
}

impl PoliceHandle {
    pub fn new(shutdown: ShutdownSignals) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let mut actor = Police::new(shutdown, receiver);
        tokio::spawn(async move { actor.run().await });

        Self { sender }
    }

    pub async fn report_problem_starting(&self) -> Option<u32> {
        let (send, recv) = oneshot::channel();
        let msg = PoliceMessage::ProblemStarting { respond_to: send };
        _ = self.sender.send(msg).await;
        recv.await.unwrap_or(None)
    }

    pub async fn report_problem_solved(&self, id: u32) {
        let msg = PoliceMessage::ProblemSolved { id };
        _ = self.sender.send(msg).await;
    }

    /// Whether a reboot is currently scheduled, the time left, and any hold.
    pub async fn status(&self) -> RebootStatus {
        let (send, recv) = oneshot::channel();
        let msg = PoliceMessage::Status { respond_to: send };
        _ = self.sender.send(msg).await;
        recv.await.unwrap_or_default()
    }

    /// Place or renew a hold that defers any scheduled reboot until the lease
    /// expires. `None` uses the default TTL; TTLs are capped at the maximum.
    pub async fn hold(&self, ttl_seconds: Option<u64>) -> RebootStatus {
        let (send, recv) = oneshot::channel();
        let msg = PoliceMessage::Hold {
            ttl_seconds,
            respond_to: send,
        };
        _ = self.sender.send(msg).await;
        recv.await.unwrap_or_default()
    }

    /// Release the hold. A deferred reboot fires at its next deadline check.
    pub async fn release_hold(&self) -> RebootStatus {
        let (send, recv) = oneshot::channel();
        let msg = PoliceMessage::ReleaseHold { respond_to: send };
        _ = self.sender.send(msg).await;
        recv.await.unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shutdown::ShutdownHandler;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Set by `fire_or_defer` in test builds instead of spawning `reboot`.
    pub(super) static REBOOT_FIRED: AtomicBool = AtomicBool::new(false);

    /// Timer branches race message branches inside the actor's select, so after
    /// advancing the clock, let the actor drain its ready timer events before
    /// querying state.
    async fn settle() {
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
    }

    /// The arming tick and a report race inside the actor's select, so retry
    /// until the report lands after arming.
    async fn report_until_armed(police: &PoliceHandle) -> u32 {
        for _ in 0..100 {
            if let Some(id) = police.report_problem_starting().await {
                return id;
            }
            tokio::task::yield_now().await;
        }
        panic!("police did not arm after ARM_AFTER");
    }

    #[tokio::test(start_paused = true)]
    async fn restarts_are_not_armed_before_the_arm_window() {
        let shutdown = ShutdownHandler::new();
        let police = PoliceHandle::new(shutdown.signals());

        assert!(police.report_problem_starting().await.is_none());

        let status = police.status().await;
        assert!(!status.reboot_pending);
        assert_eq!(status.schedule_id, None);
    }

    #[tokio::test(start_paused = true)]
    async fn hold_defers_a_scheduled_reboot_until_released() {
        let shutdown = ShutdownHandler::new();
        let police = PoliceHandle::new(shutdown.signals());

        // A round-trip guarantees the actor has started (and created its arming
        // interval) before the clock is advanced past it.
        let _ = police.status().await;

        tokio::time::advance(ARM_AFTER + Duration::from_secs(1)).await;
        report_until_armed(&police).await;

        let status = police.status().await;
        assert!(status.reboot_pending);
        assert_eq!(status.schedule_id, Some(1));
        assert_eq!(status.delay_seconds, RESTART_DELAY.as_secs());
        assert!(status.seconds_remaining <= RESTART_DELAY.as_secs());

        // Two minutes in: elapsed is visible to pollers (plex keys off this).
        tokio::time::advance(Duration::from_secs(120)).await;
        let status = police.status().await;
        assert!(status.elapsed_seconds >= 120);
        assert!(!status.held);

        // A hold outliving the deadline must defer the reboot past it. An
        // oversized request is clamped to MAX_HOLD_TTL rather than rejected.
        let status = police.hold(Some(3600)).await;
        assert!(status.held);
        assert_eq!(status.hold_seconds_remaining, MAX_HOLD_TTL.as_secs());

        // Reach the original deadline with the hold still live.
        tokio::time::advance(RESTART_DELAY - Duration::from_secs(120)).await;
        settle().await;
        let status = police.status().await;
        assert!(
            !REBOOT_FIRED.load(Ordering::SeqCst),
            "reboot fired despite a live hold"
        );
        assert!(
            status.reboot_pending,
            "deferral must keep the reboot pending"
        );

        // Released: the deferred deadline is allowed to fire.
        let status = police.release_hold().await;
        assert!(!status.held);

        tokio::time::advance(Duration::from_secs(3600)).await;
        settle().await;
        let status = police.status().await;
        assert!(REBOOT_FIRED.load(Ordering::SeqCst));
        assert!(!status.reboot_pending);
    }
}
