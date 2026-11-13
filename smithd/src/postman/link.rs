use reqwest::{StatusCode, header::HeaderMap};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const BUCKET: Duration = Duration::from_secs(60 * 60);
const WINDOW: Duration = Duration::from_secs(6 * 60 * 60);
const RECENT_SAMPLES: usize = 3;
// Larger posts measure the upload, not the round trip.
const MAX_SAMPLED_BODY: usize = 16 * 1024;

/// Round trip to the API as the device measures it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkDelay {
    /// Lowest round trip over the last hours.
    pub baseline_ms: u32,
    /// Lowest of the latest few round trips.
    pub current_ms: u32,
}

/// Round trip to the API: request time minus the time the API reports spending.
#[derive(Debug, Default)]
pub struct LinkMonitor {
    /// Minimum per hour, oldest first.
    buckets: VecDeque<(Instant, Duration)>,
    recent: VecDeque<Duration>,
}

impl LinkMonitor {
    pub fn observe(&mut self, sample: Duration, now: Instant) {
        self.drop_old(now);
        match self.buckets.back_mut() {
            Some((start, min)) if now.saturating_duration_since(*start) < BUCKET => {
                *min = (*min).min(sample);
            }
            _ => self.buckets.push_back((now, sample)),
        }
        if self.recent.len() == RECENT_SAMPLES {
            self.recent.pop_front();
        }
        self.recent.push_back(sample);
    }

    /// Lowest round trip over the last six hours, so it follows route changes.
    pub fn baseline(&self, now: Instant) -> Option<Duration> {
        self.buckets
            .iter()
            .filter(|(start, _)| now.saturating_duration_since(*start) < WINDOW)
            .map(|(_, min)| *min)
            .min()
    }

    /// Lowest of the latest samples, which drops a single slow one.
    pub fn current(&self) -> Option<Duration> {
        self.recent.iter().copied().min()
    }

    /// Readers see a new value only after a successful poll, so they judge its age.
    pub fn snapshot(&self, now: Instant) -> Option<LinkDelay> {
        Some(LinkDelay {
            baseline_ms: millis(self.baseline(now)?),
            current_ms: millis(self.current()?),
        })
    }

    fn drop_old(&mut self, now: Instant) {
        while self
            .buckets
            .front()
            .is_some_and(|(start, _)| now.saturating_duration_since(*start) >= WINDOW)
        {
            self.buckets.pop_front();
        }
    }
}

fn millis(duration: Duration) -> u32 {
    (duration.as_secs_f64() * 1000.0).round() as u32
}

/// The `dur` of the `app` metric in a `Server-Timing` header.
fn app_duration(header: &str) -> Option<Duration> {
    let metric = header.split(',').find(|metric| {
        metric
            .split(';')
            .next()
            .is_some_and(|name| name.trim() == "app")
    })?;
    let millis: f64 = metric
        .split(';')
        .skip(1)
        .find_map(|param| param.trim().strip_prefix("dur="))?
        .trim_matches('"')
        .parse()
        .ok()?;
    Duration::try_from_secs_f64(millis / 1000.0).ok()
}

/// The round trip of one post, when it says something about the link.
pub fn round_trip(
    elapsed: Duration,
    body_len: usize,
    status: StatusCode,
    headers: &HeaderMap,
) -> Option<Duration> {
    if !status.is_success() || body_len > MAX_SAMPLED_BODY {
        return None;
    }
    let server = app_duration(headers.get("server-timing")?.to_str().ok()?)?;
    elapsed.checked_sub(server)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    fn hours(value: u64) -> Duration {
        Duration::from_secs(value * 60 * 60)
    }

    #[test]
    fn an_empty_monitor_has_nothing_to_report() {
        let monitor = LinkMonitor::default();
        let now = Instant::now();
        assert_eq!(monitor.baseline(now), None);
        assert_eq!(monitor.current(), None);
        assert_eq!(monitor.snapshot(now), None);
    }

    #[test]
    fn current_is_the_minimum_of_the_last_three_samples() {
        let mut monitor = LinkMonitor::default();
        let t0 = Instant::now();
        for (i, sample) in [10, 90, 80, 70].into_iter().enumerate() {
            monitor.observe(ms(sample), t0 + Duration::from_secs(i as u64 * 20));
        }
        assert_eq!(monitor.current(), Some(ms(70)));
        monitor.observe(ms(95), t0 + Duration::from_secs(100));
        assert_eq!(monitor.current(), Some(ms(70)));
        monitor.observe(ms(96), t0 + Duration::from_secs(120));
        monitor.observe(ms(97), t0 + Duration::from_secs(140));
        assert_eq!(monitor.current(), Some(ms(95)));
        assert_eq!(
            monitor.baseline(t0 + Duration::from_secs(140)),
            Some(ms(10))
        );
    }

    #[test]
    fn the_baseline_ages_out_after_six_hours() {
        let mut monitor = LinkMonitor::default();
        let t0 = Instant::now();
        monitor.observe(ms(10), t0);
        monitor.observe(ms(50), t0 + hours(3));
        assert_eq!(monitor.baseline(t0 + hours(5)), Some(ms(10)));
        assert_eq!(monitor.baseline(t0 + hours(6)), Some(ms(50)));
        monitor.observe(ms(60), t0 + hours(7));
        assert_eq!(monitor.baseline(t0 + hours(7)), Some(ms(50)));
        assert_eq!(monitor.buckets.len(), 2);
        assert_eq!(monitor.baseline(t0 + hours(9)), Some(ms(60)));
        assert_eq!(monitor.baseline(t0 + hours(14)), None);
    }

    #[test]
    fn a_bucket_keeps_the_minimum_of_its_hour() {
        let mut monitor = LinkMonitor::default();
        let t0 = Instant::now();
        monitor.observe(ms(40), t0);
        monitor.observe(ms(30), t0 + Duration::from_secs(1800));
        monitor.observe(ms(35), t0 + Duration::from_secs(3599));
        assert_eq!(monitor.buckets.len(), 1);
        monitor.observe(ms(35), t0 + hours(1));
        assert_eq!(monitor.buckets.len(), 2);
        assert_eq!(monitor.baseline(t0 + hours(1)), Some(ms(30)));
    }

    #[test]
    fn the_snapshot_rounds_to_milliseconds() {
        let mut monitor = LinkMonitor::default();
        let t0 = Instant::now();
        monitor.observe(Duration::from_micros(12_400), t0);
        monitor.observe(Duration::from_micros(18_600), t0 + Duration::from_secs(20));
        assert_eq!(
            monitor.snapshot(t0 + Duration::from_secs(20)),
            Some(LinkDelay {
                baseline_ms: 12,
                current_ms: 12
            })
        );
        monitor.observe(Duration::from_micros(18_600), t0 + Duration::from_secs(40));
        monitor.observe(Duration::from_micros(18_500), t0 + Duration::from_secs(60));
        assert_eq!(
            monitor.snapshot(t0 + Duration::from_secs(60)),
            Some(LinkDelay {
                baseline_ms: 12,
                current_ms: 19
            })
        );
    }

    #[test]
    fn app_duration_reads_the_app_metric() {
        let cases = [
            ("app;dur=12.4", Some(Duration::from_micros(12_400))),
            ("db;dur=3, app;dur=7", Some(ms(7))),
            ("app;desc=\"x\";dur=1.5", Some(Duration::from_micros(1_500))),
            ("app;dur=\"2\"", Some(ms(2))),
            ("db;dur=3", None),
            ("app", None),
            ("app;dur=abc", None),
            ("app;dur=-1", None),
            ("app;dur=NaN", None),
            ("", None),
            ("garbage;;,,", None),
        ];
        for (header, want) in cases {
            assert_eq!(app_duration(header), want, "{header:?}");
        }
    }

    #[test]
    fn round_trip_needs_a_successful_small_post_with_a_timing_header() {
        let mut headers = HeaderMap::new();
        headers.insert("server-timing", "app;dur=10.0".parse().unwrap());
        let ok = StatusCode::OK;
        assert_eq!(round_trip(ms(35), 500, ok, &headers), Some(ms(25)));
        assert_eq!(round_trip(ms(35), 16 * 1024, ok, &headers), Some(ms(25)));
        assert_eq!(round_trip(ms(35), 16 * 1024 + 1, ok, &headers), None);
        assert_eq!(
            round_trip(ms(35), 500, StatusCode::SERVICE_UNAVAILABLE, &headers),
            None
        );
        assert_eq!(round_trip(ms(5), 500, ok, &headers), None);
        assert_eq!(round_trip(ms(35), 500, ok, &HeaderMap::new()), None);
    }
}
