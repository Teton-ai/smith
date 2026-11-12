//! Delay-based download rate control: the rate backs off when probe round
//! trips grow (a congested link) and creeps back up when they stay low.

use std::time::{Duration, Instant};
use tracing::info;

const MIN_RATE: u64 = 250_000;
const RATE_STEP: u64 = 250_000;
const SLOW_MARGIN: Duration = Duration::from_millis(100);
const SLOW_SAMPLES_TO_BACK_OFF: u32 = 2;
const LOST_PROBES_TO_PAUSE: u32 = 3;
const PAUSE: Duration = Duration::from_secs(30);
const RECOVERY_INTERVAL: Duration = Duration::from_secs(30);

pub(super) enum Probe {
    Rtt(Duration),
    Lost,
}

enum Mode {
    Running {
        rate: u64,
        slow_streak: u32,
        clean_since: Instant,
    },
    Paused {
        until: Instant,
    },
}

pub(super) struct Pacer {
    cap: u64,
    base_rtt: Option<Duration>,
    lost_streak: u32,
    mode: Mode,
}

impl Pacer {
    pub(super) fn new(cap: u64, now: Instant) -> Self {
        let cap = cap.max(1);
        Self {
            cap,
            base_rtt: None,
            lost_streak: 0,
            mode: Mode::Running {
                rate: cap,
                slow_streak: 0,
                clean_since: now,
            },
        }
    }

    /// A cap below `MIN_RATE` is its own floor.
    fn floor(&self) -> u64 {
        MIN_RATE.min(self.cap)
    }

    pub(super) fn observe(&mut self, probe: Probe, now: Instant) {
        let sample = match probe {
            Probe::Rtt(rtt) => {
                self.base_rtt = Some(self.base_rtt.map_or(rtt, |base| base.min(rtt)));
                Some(rtt)
            }
            // Until one probe gets through, a loss may be a firewall or proxy
            // that blocks the probe and not the download.
            Probe::Lost if self.base_rtt.is_none() => return,
            Probe::Lost => None,
        };
        let slow = match (sample, self.base_rtt) {
            (Some(rtt), Some(base)) => rtt > base + SLOW_MARGIN,
            _ => true,
        };
        let base_rtt = self.base_rtt;
        let floor = self.floor();

        let Mode::Running {
            rate,
            slow_streak,
            clean_since,
        } = &mut self.mode
        else {
            return;
        };

        if !slow {
            *slow_streak = 0;
            self.lost_streak = 0;
            return;
        }
        *slow_streak += 1;
        if sample.is_none() {
            self.lost_streak += 1;
        } else {
            self.lost_streak = 0;
        }

        if self.lost_streak >= LOST_PROBES_TO_PAUSE {
            info!(
                old_rate = *rate,
                ?base_rtt,
                ?sample,
                "download paused, probes lost"
            );
            self.lost_streak = 0;
            self.mode = Mode::Paused { until: now + PAUSE };
        } else if *slow_streak >= SLOW_SAMPLES_TO_BACK_OFF {
            let new_rate = (*rate / 2).max(floor);
            if new_rate != *rate {
                info!(
                    old_rate = *rate,
                    new_rate,
                    ?base_rtt,
                    ?sample,
                    "download rate lowered"
                );
            }
            *rate = new_rate;
            *slow_streak = 0;
            *clean_since = now;
        }
    }

    /// The current rate in bytes per second, `None` while paused.
    pub(super) fn rate(&mut self, now: Instant) -> Option<u64> {
        match &mut self.mode {
            Mode::Paused { until } => {
                if now < *until {
                    return None;
                }
                let rate = self.cap.min(MIN_RATE);
                info!(new_rate = rate, base_rtt = ?self.base_rtt, "download resumed");
                self.mode = Mode::Running {
                    rate,
                    slow_streak: 0,
                    clean_since: now,
                };
                Some(rate)
            }
            Mode::Running {
                rate, clean_since, ..
            } => {
                if now.duration_since(*clean_since) >= RECOVERY_INTERVAL && *rate < self.cap {
                    let new_rate = (*rate + RATE_STEP).min(self.cap);
                    info!(old_rate = *rate, new_rate, base_rtt = ?self.base_rtt, "download rate raised");
                    *rate = new_rate;
                    *clean_since = now;
                }
                Some(*rate)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: u64 = 2_000_000;
    const FAST: Probe = Probe::Rtt(Duration::from_millis(20));
    const SLOW: Probe = Probe::Rtt(Duration::from_millis(300));

    fn pacer_with_base(now: Instant) -> Pacer {
        let mut pacer = Pacer::new(CAP, now);
        pacer.observe(FAST, now);
        pacer
    }

    #[test]
    fn halves_after_two_slow_samples() {
        let now = Instant::now();
        let mut pacer = pacer_with_base(now);
        pacer.observe(SLOW, now);
        pacer.observe(SLOW, now);
        assert_eq!(pacer.rate(now), Some(1_000_000));
    }

    #[test]
    fn one_slow_sample_keeps_the_rate() {
        let now = Instant::now();
        let mut pacer = pacer_with_base(now);
        pacer.observe(SLOW, now);
        assert_eq!(pacer.rate(now), Some(CAP));
    }

    #[test]
    fn fast_sample_between_slow_ones_resets_the_streak() {
        let now = Instant::now();
        let mut pacer = pacer_with_base(now);
        pacer.observe(SLOW, now);
        pacer.observe(FAST, now);
        pacer.observe(SLOW, now);
        assert_eq!(pacer.rate(now), Some(CAP));
    }

    #[test]
    fn rate_floors_at_min_rate() {
        let now = Instant::now();
        let mut pacer = pacer_with_base(now);
        for _ in 0..10 {
            pacer.observe(SLOW, now);
            pacer.observe(SLOW, now);
        }
        assert_eq!(pacer.rate(now), Some(250_000));
    }

    #[test]
    fn recovers_a_step_per_clean_interval_up_to_cap() {
        let now = Instant::now();
        let mut pacer = pacer_with_base(now);
        for _ in 0..4 {
            pacer.observe(SLOW, now);
        }
        assert_eq!(pacer.rate(now), Some(500_000));

        assert_eq!(pacer.rate(now + Duration::from_secs(29)), Some(500_000));
        assert_eq!(pacer.rate(now + Duration::from_secs(30)), Some(750_000));
        assert_eq!(pacer.rate(now + Duration::from_secs(59)), Some(750_000));
        assert_eq!(pacer.rate(now + Duration::from_secs(60)), Some(1_000_000));
        let mut later = now + Duration::from_secs(60);
        for _ in 0..10 {
            later += Duration::from_secs(30);
            pacer.rate(later);
        }
        assert_eq!(pacer.rate(later), Some(CAP));
    }

    #[test]
    fn three_lost_probes_pause_then_resume_at_min_rate() {
        let now = Instant::now();
        let mut pacer = pacer_with_base(now);
        for _ in 0..3 {
            pacer.observe(Probe::Lost, now);
        }
        assert_eq!(pacer.rate(now), None);
        assert_eq!(pacer.rate(now + Duration::from_secs(29)), None);
        assert_eq!(pacer.rate(now + Duration::from_secs(30)), Some(250_000));
    }

    #[test]
    fn lost_probes_before_any_round_trip_do_not_pause() {
        let now = Instant::now();
        let mut pacer = Pacer::new(CAP, now);
        for _ in 0..10 {
            pacer.observe(Probe::Lost, now);
        }
        assert_eq!(pacer.rate(now), Some(CAP));
        pacer.observe(FAST, now);
        for _ in 0..3 {
            pacer.observe(Probe::Lost, now);
        }
        assert_eq!(pacer.rate(now), None);
    }

    #[test]
    fn two_lost_probes_do_not_pause() {
        let now = Instant::now();
        let mut pacer = pacer_with_base(now);
        pacer.observe(Probe::Lost, now);
        pacer.observe(Probe::Lost, now);
        pacer.observe(FAST, now);
        pacer.observe(Probe::Lost, now);
        assert_eq!(pacer.rate(now), Some(1_000_000));
    }

    #[test]
    fn cap_below_min_rate_is_the_floor() {
        let now = Instant::now();
        let mut pacer = Pacer::new(100_000, now);
        pacer.observe(FAST, now);
        pacer.observe(SLOW, now);
        pacer.observe(SLOW, now);
        assert_eq!(pacer.rate(now), Some(100_000));

        for _ in 0..3 {
            pacer.observe(Probe::Lost, now);
        }
        assert_eq!(pacer.rate(now + Duration::from_secs(30)), Some(100_000));
    }
}
