//! Pure decision logic for link health: no I/O, the caller passes the time so
//! every rule can be tested without a device.

use std::collections::VecDeque;
use tokio::time::{Duration, Instant};

/// Two reads 15 s apart with the flag up mean it has been stuck for at least
/// 15 s; a healthy roam clears it within milliseconds of the key install.
const STALL_READS: u8 = 2;
/// Stalls in the field recurred every 3 to 10 min; 2 min never blocks that.
const MIN_RECOVERY_GAP: Duration = Duration::from_secs(2 * 60);
const MAX_RECOVERIES_PER_HOUR: usize = 10;
const HOUR: Duration = Duration::from_secs(60 * 60);
/// Time a reconnect gets to clear the flag before we stop trying.
const RECOVERY_CHECK: Duration = Duration::from_secs(60);
/// Stuck while not associated: NM is reconnecting anyway, so ask again later
/// instead of running `iw` on every read.
const NOT_ASSOCIATED_RECHECK: Duration = Duration::from_secs(60);

/// NetworkManager `NMDeviceState` and `NMDeviceStateReason` values.
pub(crate) const NM_DEVICE_STATE_DISCONNECTED: u32 = 30;
pub(crate) const NM_DEVICE_STATE_ACTIVATED: u32 = 100;
pub(crate) const NM_DEVICE_STATE_FAILED: u32 = 120;
pub(crate) const NM_DEVICE_STATE_REASON_NO_SECRETS: u32 = 7;
/// NM blocks autoconnect after a no-secrets failure; give it time to recover
/// on its own before forcing an activation.
const LOCKOUT_FIRST_ATTEMPT: Duration = Duration::from_secs(2 * 60);
/// Waits after each attempt; the last one repeats.
const LOCKOUT_BACKOFF: [Duration; 4] = [
    Duration::from_secs(4 * 60),
    Duration::from_secs(8 * 60),
    Duration::from_secs(16 * 60),
    Duration::from_secs(30 * 60),
];

/// The `TX: roam_buf_pkt=<flag>:<count>` line of the vendor driver's
/// `trx_info`: whether TX is parked in the roam buffer, and how many frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RoamBuf {
    pub flag: u8,
    pub count: u32,
}

pub(crate) fn parse_trx_info(text: &str) -> Option<RoamBuf> {
    let value = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("TX: roam_buf_pkt="))?;
    let (flag, count) = value.trim().split_once(':')?;
    Some(RoamBuf {
        flag: flag.parse().ok()?,
        count: count.parse().ok()?,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoamLimit {
    Spacing,
    HourlyCap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoamEvent {
    Idle,
    /// Stuck long enough: reconnect, then call [`RoamDetector::recovered`].
    Stall {
        count: u32,
    },
    /// Stuck, but a limit holds the recovery back. Reported once per incident.
    Limited(RoamLimit),
    Recovered {
        elapsed: Duration,
    },
    /// The reconnect did not clear the flag; nothing more until it clears.
    StillStuck {
        count: u32,
    },
}

#[derive(Debug, Default)]
pub(crate) struct RoamDetector {
    stuck_reads: u8,
    /// When the last reconnect returned, until its outcome is known.
    checking_since: Option<Instant>,
    gave_up: bool,
    limit_reported: bool,
    deferred_until: Option<Instant>,
    recoveries: VecDeque<Instant>,
}

impl RoamDetector {
    /// Feeds one `trx_info` read. `None` means the file or the line was
    /// missing, which is also what a driver reload looks like.
    pub fn observe(&mut self, read: Option<RoamBuf>, now: Instant) -> RoamEvent {
        let Some(read) = read else {
            self.stuck_reads = 0;
            return RoamEvent::Idle;
        };

        if read.flag == 0 {
            self.stuck_reads = 0;
            self.gave_up = false;
            self.limit_reported = false;
            self.deferred_until = None;
            return match self.checking_since.take() {
                Some(since) => RoamEvent::Recovered {
                    elapsed: now.saturating_duration_since(since),
                },
                None => RoamEvent::Idle,
            };
        }

        self.stuck_reads = self.stuck_reads.saturating_add(1);

        if let Some(since) = self.checking_since {
            if now.saturating_duration_since(since) < RECOVERY_CHECK {
                return RoamEvent::Idle;
            }
            self.checking_since = None;
            self.gave_up = true;
            return RoamEvent::StillStuck { count: read.count };
        }

        if self.gave_up
            || self.stuck_reads < STALL_READS
            || self.deferred_until.is_some_and(|until| now < until)
        {
            return RoamEvent::Idle;
        }

        while self
            .recoveries
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= HOUR)
        {
            self.recoveries.pop_front();
        }
        let limit = if self
            .recoveries
            .back()
            .is_some_and(|at| now.saturating_duration_since(*at) < MIN_RECOVERY_GAP)
        {
            Some(RoamLimit::Spacing)
        } else if self.recoveries.len() >= MAX_RECOVERIES_PER_HOUR {
            Some(RoamLimit::HourlyCap)
        } else {
            None
        };

        match limit {
            Some(_) if self.limit_reported => RoamEvent::Idle,
            Some(limit) => {
                self.limit_reported = true;
                RoamEvent::Limited(limit)
            }
            None => RoamEvent::Stall { count: read.count },
        }
    }

    /// A [`RoamEvent::Stall`] could not be acted on yet (not associated).
    pub fn defer(&mut self, now: Instant) {
        self.deferred_until = Some(now + NOT_ASSOCIATED_RECHECK);
    }

    /// The reconnect for a [`RoamEvent::Stall`] has returned, at `now`.
    pub fn recovered(&mut self, now: Instant) {
        self.recoveries.push_back(now);
        self.checking_since = Some(now);
        self.stuck_reads = 0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockoutNote {
    None,
    Started,
    Recovered {
        elapsed: Duration,
        attempts: u32,
    },
    /// Any other Wi-Fi failure, logged to learn whether it strands devices too.
    OtherFailure {
        reason: u32,
    },
}

#[derive(Debug)]
struct Incident {
    started: Instant,
    attempts: u32,
    due: Instant,
}

#[derive(Debug, Default)]
pub(crate) struct LockoutTracker {
    incident: Option<Incident>,
}

impl LockoutTracker {
    pub fn on_state_changed(&mut self, new: u32, reason: u32, now: Instant) -> LockoutNote {
        if new == NM_DEVICE_STATE_ACTIVATED {
            return match self.incident.take() {
                Some(incident) => LockoutNote::Recovered {
                    elapsed: now.saturating_duration_since(incident.started),
                    attempts: incident.attempts,
                },
                None => LockoutNote::None,
            };
        }
        if reason == NM_DEVICE_STATE_REASON_NO_SECRETS {
            if self.incident.is_some() {
                return LockoutNote::None;
            }
            self.incident = Some(Incident {
                started: now,
                attempts: 0,
                due: now + LOCKOUT_FIRST_ATTEMPT,
            });
            return LockoutNote::Started;
        }
        if new == NM_DEVICE_STATE_FAILED {
            return LockoutNote::OtherFailure { reason };
        }
        LockoutNote::None
    }

    /// The device's state when we started watching it. NM moves a no-secrets
    /// failure on to DISCONNECTED with reason NONE and keeps the autoconnect
    /// block on the profile, where D-Bus does not expose it, so a device found
    /// disconnected is treated as a possible lockout. The wait and the
    /// association check before acting keep a false alarm cheap.
    pub fn on_initial_state(&mut self, state: u32, reason: u32, now: Instant) -> LockoutNote {
        let stranded = matches!(state, NM_DEVICE_STATE_DISCONNECTED | NM_DEVICE_STATE_FAILED);
        if stranded && self.incident.is_none() {
            return self.on_state_changed(state, NM_DEVICE_STATE_REASON_NO_SECRETS, now);
        }
        self.on_state_changed(state, reason, now)
    }

    /// Ends the incident without an event, when the link is seen up anyway.
    pub fn resolved(&mut self, now: Instant) -> LockoutNote {
        self.on_state_changed(NM_DEVICE_STATE_ACTIVATED, 0, now)
    }

    pub fn due(&self, now: Instant) -> bool {
        self.incident.as_ref().is_some_and(|i| now >= i.due)
    }

    /// An activation was attempted at `now`; schedules the next one.
    pub fn attempted(&mut self, now: Instant) {
        if let Some(incident) = self.incident.as_mut() {
            let step = (incident.attempts as usize).min(LOCKOUT_BACKOFF.len() - 1);
            incident.attempts += 1;
            incident.due = now + LOCKOUT_BACKOFF[step];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: Duration = Duration::from_secs(15);

    fn stuck(count: u32) -> Option<RoamBuf> {
        Some(RoamBuf { flag: 1, count })
    }

    fn clear() -> Option<RoamBuf> {
        Some(RoamBuf { flag: 0, count: 0 })
    }

    #[test]
    fn parses_trx_info() {
        let text = "free_xmitbuf_cnt=256, free_xmitframe_cnt=236\n\
                    Tx: available page num: HPQ: 64, LPQ: 64, NPQ: 64, EPQ: 64, PUBQ: 1689\n\
                    TX: roam_buf_pkt=1:20\n\
                    RX: Count of Packets dropped by Driver: 0\n";
        assert_eq!(parse_trx_info(text), Some(RoamBuf { flag: 1, count: 20 }));
        assert_eq!(
            parse_trx_info("TX: roam_buf_pkt=0:0\n"),
            Some(RoamBuf { flag: 0, count: 0 })
        );
        assert_eq!(parse_trx_info("free_xmitframe_cnt=256\n"), None);
        assert_eq!(parse_trx_info("TX: roam_buf_pkt=garbage\n"), None);
    }

    #[test]
    fn stall_needs_two_consecutive_stuck_reads() {
        let t0 = Instant::now();
        let mut d = RoamDetector::default();
        assert_eq!(d.observe(stuck(5), t0), RoamEvent::Idle);
        assert_eq!(d.observe(clear(), t0 + TICK), RoamEvent::Idle);
        assert_eq!(d.observe(stuck(5), t0 + TICK * 2), RoamEvent::Idle);
        assert_eq!(
            d.observe(stuck(20), t0 + TICK * 3),
            RoamEvent::Stall { count: 20 }
        );
    }

    #[test]
    fn missing_reads_reset_the_count() {
        let t0 = Instant::now();
        let mut d = RoamDetector::default();
        d.observe(stuck(5), t0);
        assert_eq!(d.observe(None, t0 + TICK), RoamEvent::Idle);
        assert_eq!(d.observe(stuck(5), t0 + TICK * 2), RoamEvent::Idle);
    }

    #[test]
    fn reports_recovered_or_still_stuck_after_a_reconnect() {
        let t0 = Instant::now();
        let mut d = RoamDetector::default();
        d.observe(stuck(5), t0);
        d.observe(stuck(20), t0 + TICK);
        d.recovered(t0 + TICK * 2);
        assert_eq!(
            d.observe(clear(), t0 + TICK * 4),
            RoamEvent::Recovered { elapsed: TICK * 2 }
        );

        let t1 = t0 + Duration::from_secs(600);
        d.observe(stuck(5), t1);
        d.observe(stuck(20), t1 + TICK);
        d.recovered(t1 + TICK);
        assert_eq!(d.observe(stuck(40), t1 + TICK * 2), RoamEvent::Idle);
        assert_eq!(
            d.observe(stuck(80), t1 + TICK + RECOVERY_CHECK),
            RoamEvent::StillStuck { count: 80 }
        );
        assert_eq!(d.observe(stuck(90), t1 + TICK * 7), RoamEvent::Idle);
        d.observe(clear(), t1 + TICK * 8);
        d.observe(stuck(5), t1 + TICK * 9);
        assert_eq!(
            d.observe(stuck(10), t1 + TICK * 10),
            RoamEvent::Stall { count: 10 }
        );
    }

    #[test]
    fn spacing_holds_back_a_quick_restall_once() {
        let t0 = Instant::now();
        let mut d = RoamDetector::default();
        d.observe(stuck(5), t0);
        d.observe(stuck(5), t0 + TICK);
        d.recovered(t0 + TICK);
        d.observe(clear(), t0 + TICK * 2);
        d.observe(stuck(5), t0 + TICK * 3);
        assert_eq!(
            d.observe(stuck(5), t0 + TICK * 4),
            RoamEvent::Limited(RoamLimit::Spacing)
        );
        assert_eq!(d.observe(stuck(5), t0 + TICK * 5), RoamEvent::Idle);
        assert_eq!(
            d.observe(stuck(5), t0 + TICK + MIN_RECOVERY_GAP),
            RoamEvent::Stall { count: 5 }
        );
    }

    /// A field device's ten reconnects in 72 min.
    #[test]
    fn field_restall_sequence_is_never_blocked_then_the_cap_holds() {
        let minutes = [0u64, 9, 32, 41, 47, 51, 54, 57, 60, 72];
        let t0 = Instant::now();
        let mut d = RoamDetector::default();
        for m in minutes {
            let at = t0 + Duration::from_secs(m * 60);
            d.observe(stuck(5), at);
            assert_eq!(
                d.observe(stuck(5), at + TICK),
                RoamEvent::Stall { count: 5 },
                "minute {m}"
            );
            d.recovered(at + TICK);
            d.observe(clear(), at + TICK * 2);
        }

        // The eleventh within an hour of the last ten trips the cap.
        let mut d = RoamDetector::default();
        for i in 0..10u64 {
            let at = t0 + Duration::from_secs(i * 180);
            d.observe(stuck(5), at);
            d.observe(stuck(5), at + TICK);
            d.recovered(at + TICK);
            d.observe(clear(), at + TICK * 2);
        }
        let at = t0 + Duration::from_secs(10 * 180);
        d.observe(stuck(5), at);
        assert_eq!(
            d.observe(stuck(5), at + TICK),
            RoamEvent::Limited(RoamLimit::HourlyCap)
        );
    }

    #[test]
    fn deferred_stall_waits_a_minute() {
        let t0 = Instant::now();
        let mut d = RoamDetector::default();
        d.observe(stuck(5), t0);
        assert_eq!(
            d.observe(stuck(5), t0 + TICK),
            RoamEvent::Stall { count: 5 }
        );
        d.defer(t0 + TICK);
        assert_eq!(d.observe(stuck(5), t0 + TICK * 2), RoamEvent::Idle);
        assert_eq!(
            d.observe(stuck(5), t0 + TICK + NOT_ASSOCIATED_RECHECK),
            RoamEvent::Stall { count: 5 }
        );
    }

    #[test]
    fn lockout_waits_two_minutes_then_backs_off() {
        let t0 = Instant::now();
        let mut l = LockoutTracker::default();
        assert_eq!(
            l.on_state_changed(
                NM_DEVICE_STATE_FAILED,
                NM_DEVICE_STATE_REASON_NO_SECRETS,
                t0
            ),
            LockoutNote::Started
        );
        assert!(!l.due(t0 + LOCKOUT_FIRST_ATTEMPT - TICK));
        assert!(l.due(t0 + LOCKOUT_FIRST_ATTEMPT));

        let mut at = t0 + LOCKOUT_FIRST_ATTEMPT;
        for wait in [4u64, 8, 16, 30, 30] {
            l.attempted(at);
            let next = at + Duration::from_secs(wait * 60);
            assert!(!l.due(next - TICK), "wait {wait}");
            assert!(l.due(next), "wait {wait}");
            at = next;
        }
    }

    #[test]
    fn activation_ends_the_lockout() {
        let t0 = Instant::now();
        let mut l = LockoutTracker::default();
        l.on_state_changed(30, NM_DEVICE_STATE_REASON_NO_SECRETS, t0);
        assert_eq!(
            l.on_state_changed(30, NM_DEVICE_STATE_REASON_NO_SECRETS, t0 + TICK),
            LockoutNote::None
        );
        l.attempted(t0 + LOCKOUT_FIRST_ATTEMPT);
        assert_eq!(
            l.on_state_changed(
                NM_DEVICE_STATE_ACTIVATED,
                0,
                t0 + LOCKOUT_FIRST_ATTEMPT + TICK
            ),
            LockoutNote::Recovered {
                elapsed: LOCKOUT_FIRST_ATTEMPT + TICK,
                attempts: 1
            }
        );
        assert!(!l.due(t0 + HOUR));
    }

    #[test]
    fn a_disconnected_device_at_startup_is_a_possible_lockout() {
        let t0 = Instant::now();
        let mut l = LockoutTracker::default();
        assert_eq!(
            l.on_initial_state(NM_DEVICE_STATE_DISCONNECTED, 0, t0),
            LockoutNote::Started
        );
        assert!(l.due(t0 + LOCKOUT_FIRST_ATTEMPT));

        let mut l = LockoutTracker::default();
        assert_eq!(
            l.on_initial_state(NM_DEVICE_STATE_ACTIVATED, 0, t0),
            LockoutNote::None
        );
        // Unavailable (20): radio off or no firmware, nothing to activate.
        assert_eq!(l.on_initial_state(20, 0, t0), LockoutNote::None);
        assert!(!l.due(t0 + HOUR));
    }

    #[test]
    fn other_failures_are_only_noted() {
        let t0 = Instant::now();
        let mut l = LockoutTracker::default();
        assert_eq!(
            l.on_state_changed(NM_DEVICE_STATE_FAILED, 11, t0),
            LockoutNote::OtherFailure { reason: 11 }
        );
        assert_eq!(l.on_state_changed(30, 39, t0), LockoutNote::None);
        assert!(!l.due(t0 + HOUR));
    }
}
