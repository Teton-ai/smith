//! Delay-based download rate control. The rate starts low and creeps up while
//! the round trip to the api stays near its baseline, and backs off when it
//! grows (a congested link).

use std::time::{Duration, Instant};
use tracing::info;

const MIN_RATE: u64 = 250_000;
const RATE_STEP: u64 = 250_000;
const SLOW_MARGIN: Duration = Duration::from_millis(100);
const SLOW_SAMPLES_TO_BACK_OFF: u32 = 2;
const LOST_PROBES_TO_PAUSE: u32 = 3;
const PAUSE: Duration = Duration::from_secs(30);
/// Spreads the resumes of devices that paused together.
const PAUSE_JITTER: Duration = Duration::from_secs(15);
const RECOVERY_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone, Copy)]
pub(super) enum Probe {
    Delay {
        baseline: Duration,
        current: Duration,
    },
    Lost,
}

/// What the pacer saw during one download.
#[derive(Clone, Debug, Default)]
pub(super) struct PacerStats {
    pub delay_samples: u32,
    pub lost: u32,
    pub lowest_rate: u64,
    pub back_offs: u32,
    pub pauses: u32,
    pub last_delay: Option<(Duration, Duration)>,
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
    remote: String,
    lost_streak: u32,
    mode: Mode,
    stats: PacerStats,
}

impl Pacer {
    pub(super) fn new(cap: u64, remote: &str, now: Instant) -> Self {
        let cap = cap.max(1);
        let floor = MIN_RATE.min(cap);
        Self {
            cap,
            remote: remote.to_owned(),
            lost_streak: 0,
            mode: Mode::Running {
                rate: floor,
                slow_streak: 0,
                clean_since: now,
            },
            stats: PacerStats {
                lowest_rate: floor,
                ..PacerStats::default()
            },
        }
    }

    pub(super) fn stats(&self) -> &PacerStats {
        &self.stats
    }

    /// A cap below `MIN_RATE` is its own floor.
    fn floor(&self) -> u64 {
        MIN_RATE.min(self.cap)
    }

    /// `jitter` in `0..=1` stretches a pause.
    pub(super) fn observe(&mut self, probe: Probe, now: Instant, jitter: f64) {
        let lost = matches!(probe, Probe::Lost);
        let slow = match probe {
            Probe::Delay { baseline, current } => {
                self.stats.delay_samples += 1;
                self.stats.last_delay = Some((baseline, current));
                current > baseline + SLOW_MARGIN
            }
            Probe::Lost => {
                self.stats.lost += 1;
                // Until one sample arrives, a loss may be an api that reports no
                // timing and not a congested link.
                if self.stats.delay_samples == 0 {
                    return;
                }
                true
            }
        };
        let floor = self.floor();
        let last_delay = self.stats.last_delay;

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
        if lost {
            self.lost_streak += 1;
        } else {
            self.lost_streak = 0;
        }

        if self.lost_streak >= LOST_PROBES_TO_PAUSE {
            info!(
                remote = %self.remote,
                old_rate = *rate,
                ?last_delay,
                "download paused, no link delay samples"
            );
            self.lost_streak = 0;
            self.stats.pauses += 1;
            let pause = PAUSE + PAUSE_JITTER.mul_f64(jitter.clamp(0.0, 1.0));
            self.mode = Mode::Paused { until: now + pause };
        } else if *slow_streak >= SLOW_SAMPLES_TO_BACK_OFF {
            let new_rate = (*rate / 2).max(floor);
            if new_rate != *rate {
                info!(
                    remote = %self.remote,
                    old_rate = *rate,
                    new_rate,
                    ?last_delay,
                    "download rate lowered"
                );
                self.stats.back_offs += 1;
            }
            *rate = new_rate;
            *slow_streak = 0;
            *clean_since = now;
            self.stats.lowest_rate = self.stats.lowest_rate.min(new_rate);
        }
    }

    /// The current rate in bytes per second, `None` while paused.
    pub(super) fn rate(&mut self, now: Instant) -> Option<u64> {
        match &mut self.mode {
            Mode::Paused { until } => {
                if now < *until {
                    return None;
                }
                let rate = MIN_RATE.min(self.cap);
                info!(remote = %self.remote, new_rate = rate, "download resumed");
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
                    info!(remote = %self.remote, old_rate = *rate, new_rate, "download rate raised");
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
    const FAST: Probe = delay(20, 20);
    const SLOW: Probe = delay(20, 300);

    const fn delay(baseline_ms: u64, current_ms: u64) -> Probe {
        Probe::Delay {
            baseline: Duration::from_millis(baseline_ms),
            current: Duration::from_millis(current_ms),
        }
    }

    fn secs(value: u64) -> Duration {
        Duration::from_secs(value)
    }

    /// A pacer that already ramped to the cap.
    fn pacer_at_cap(now: Instant) -> (Pacer, Instant) {
        let mut pacer = Pacer::new(CAP, "pkg", now);
        let later = now + secs(15 * 7);
        for step in 1..=7 {
            pacer.rate(now + secs(15 * step));
        }
        assert_eq!(pacer.rate(later), Some(CAP));
        pacer.observe(FAST, later, 0.0);
        (pacer, later)
    }

    #[test]
    fn starts_at_the_floor_and_ramps_a_step_per_clean_interval() {
        let now = Instant::now();
        let mut pacer = Pacer::new(CAP, "pkg", now);
        assert_eq!(pacer.rate(now), Some(250_000));
        assert_eq!(pacer.rate(now + secs(14)), Some(250_000));
        assert_eq!(pacer.rate(now + secs(15)), Some(500_000));
        assert_eq!(pacer.rate(now + secs(29)), Some(500_000));
        assert_eq!(pacer.rate(now + secs(30)), Some(750_000));
        assert_eq!(pacer.rate(now + secs(105)), Some(1_000_000));
        let mut later = now + secs(105);
        for _ in 0..4 {
            later += secs(15);
            pacer.rate(later);
        }
        assert_eq!(pacer.rate(later), Some(CAP));
        assert_eq!(pacer.rate(later + secs(60)), Some(CAP));
    }

    #[test]
    fn halves_after_two_slow_samples() {
        let now = Instant::now();
        let (mut pacer, now) = pacer_at_cap(now);
        pacer.observe(SLOW, now, 0.0);
        assert_eq!(pacer.rate(now), Some(CAP));
        pacer.observe(SLOW, now, 0.0);
        assert_eq!(pacer.rate(now), Some(1_000_000));
        assert_eq!(pacer.stats().back_offs, 1);
        assert_eq!(pacer.stats().lowest_rate, 250_000);
    }

    #[test]
    fn a_clean_sample_between_slow_ones_resets_the_streak() {
        let (mut pacer, now) = pacer_at_cap(Instant::now());
        pacer.observe(SLOW, now, 0.0);
        pacer.observe(FAST, now, 0.0);
        pacer.observe(SLOW, now, 0.0);
        assert_eq!(pacer.rate(now), Some(CAP));
    }

    #[test]
    fn slow_is_judged_against_the_reported_baseline() {
        let (mut pacer, now) = pacer_at_cap(Instant::now());
        for _ in 0..5 {
            pacer.observe(delay(400, 500), now, 0.0);
        }
        assert_eq!(pacer.rate(now), Some(CAP));
        for _ in 0..2 {
            pacer.observe(delay(400, 501), now, 0.0);
        }
        assert_eq!(pacer.rate(now), Some(1_000_000));
    }

    #[test]
    fn the_rate_floors_at_min_rate() {
        let (mut pacer, now) = pacer_at_cap(Instant::now());
        for _ in 0..20 {
            pacer.observe(SLOW, now, 0.0);
        }
        assert_eq!(pacer.rate(now), Some(250_000));
        assert_eq!(pacer.stats().back_offs, 3);
    }

    #[test]
    fn a_back_off_restarts_the_clean_interval() {
        let (mut pacer, now) = pacer_at_cap(Instant::now());
        pacer.observe(SLOW, now, 0.0);
        pacer.observe(SLOW, now, 0.0);
        assert_eq!(pacer.rate(now + secs(14)), Some(1_000_000));
        assert_eq!(pacer.rate(now + secs(15)), Some(1_250_000));
    }

    #[test]
    fn losses_before_any_delay_sample_do_not_pause() {
        let now = Instant::now();
        let mut pacer = Pacer::new(CAP, "pkg", now);
        for _ in 0..10 {
            pacer.observe(Probe::Lost, now, 0.0);
        }
        assert_eq!(pacer.rate(now), Some(250_000));
        assert_eq!(pacer.stats().lost, 10);
        pacer.observe(FAST, now, 0.0);
        for _ in 0..3 {
            pacer.observe(Probe::Lost, now, 0.0);
        }
        assert_eq!(pacer.rate(now), None);
    }

    #[test]
    fn three_losses_pause_for_30_s_plus_jitter_then_resume_at_the_floor() {
        let (mut pacer, now) = pacer_at_cap(Instant::now());
        for _ in 0..3 {
            pacer.observe(Probe::Lost, now, 0.5);
        }
        assert_eq!(pacer.rate(now), None);
        assert_eq!(pacer.rate(now + Duration::from_millis(37_499)), None);
        assert_eq!(
            pacer.rate(now + Duration::from_millis(37_500)),
            Some(250_000)
        );
        assert_eq!(pacer.stats().pauses, 1);
    }

    #[test]
    fn the_pause_spans_30_to_45_s() {
        for (jitter, until) in [(0.0, 30), (1.0, 45), (7.0, 45), (-1.0, 30)] {
            let (mut pacer, now) = pacer_at_cap(Instant::now());
            for _ in 0..3 {
                pacer.observe(Probe::Lost, now, jitter);
            }
            assert_eq!(pacer.rate(now + secs(until - 1)), None, "{jitter}");
            assert_eq!(pacer.rate(now + secs(until)), Some(250_000), "{jitter}");
        }
    }

    #[test]
    fn two_losses_do_not_pause() {
        let (mut pacer, now) = pacer_at_cap(Instant::now());
        pacer.observe(Probe::Lost, now, 0.0);
        pacer.observe(Probe::Lost, now, 0.0);
        pacer.observe(FAST, now, 0.0);
        pacer.observe(Probe::Lost, now, 0.0);
        assert_eq!(pacer.rate(now), Some(1_000_000));
    }

    #[test]
    fn counts_samples_losses_and_the_last_delay() {
        let (mut pacer, now) = pacer_at_cap(Instant::now());
        pacer.observe(FAST, now, 0.0);
        pacer.observe(delay(20, 35), now, 0.0);
        pacer.observe(Probe::Lost, now, 0.0);
        let stats = pacer.stats();
        assert_eq!(stats.delay_samples, 3);
        assert_eq!(stats.lost, 1);
        assert_eq!(stats.pauses, 0);
        assert_eq!(stats.back_offs, 0);
        assert_eq!(
            stats.last_delay,
            Some((Duration::from_millis(20), Duration::from_millis(35)))
        );
    }

    #[test]
    fn a_cap_below_min_rate_is_the_floor() {
        let now = Instant::now();
        let mut pacer = Pacer::new(100_000, "pkg", now);
        assert_eq!(pacer.rate(now + secs(60)), Some(100_000));
        pacer.observe(FAST, now, 0.0);
        pacer.observe(SLOW, now, 0.0);
        pacer.observe(SLOW, now, 0.0);
        assert_eq!(pacer.rate(now), Some(100_000));
        assert_eq!(pacer.stats().back_offs, 0);

        for _ in 0..3 {
            pacer.observe(Probe::Lost, now, 0.0);
        }
        assert_eq!(pacer.rate(now + secs(30)), Some(100_000));
    }
}
