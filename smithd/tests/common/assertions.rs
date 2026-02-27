use std::time::Duration;

/// Assert that a timing measurement is within acceptable bounds
pub fn assert_timing_within(actual_ms: u128, expected_ms: u128, tolerance_ms: u128, message: &str) {
    let diff = (actual_ms as i128 - expected_ms as i128).abs();
    assert!(
        diff <= tolerance_ms as i128,
        "{}: expected ~{}ms, got {}ms (diff: {}ms, tolerance: {}ms)",
        message,
        expected_ms,
        actual_ms,
        diff,
        tolerance_ms
    );
}

/// Assert that intervals between events are consistent
pub fn assert_intervals_consistent(
    intervals: &[Duration],
    expected: Duration,
    tolerance: Duration,
) {
    for (idx, interval) in intervals.iter().enumerate() {
        let diff = if *interval > expected {
            *interval - expected
        } else {
            expected - *interval
        };

        assert!(
            diff <= tolerance,
            "Interval {} inconsistent: expected {:?}, got {:?} (diff: {:?}, tolerance: {:?})",
            idx,
            expected,
            interval,
            diff,
            tolerance
        );
    }
}

/// Calculate average duration from a slice
pub fn average_duration(durations: &[Duration]) -> Duration {
    if durations.is_empty() {
        return Duration::ZERO;
    }
    let total_ms: u128 = durations.iter().map(|d| d.as_millis()).sum();
    Duration::from_millis((total_ms / durations.len() as u128) as u64)
}

/// Calculate standard deviation of durations
pub fn std_dev_duration(durations: &[Duration]) -> f64 {
    if durations.is_empty() {
        return 0.0;
    }

    let mean = average_duration(durations).as_millis() as f64;
    let variance: f64 = durations
        .iter()
        .map(|d| {
            let diff = d.as_millis() as f64 - mean;
            diff * diff
        })
        .sum::<f64>()
        / durations.len() as f64;

    variance.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assert_timing_within() {
        assert_timing_within(20_050, 20_000, 500, "test timing");
    }

    #[test]
    #[should_panic]
    fn test_assert_timing_outside_tolerance() {
        assert_timing_within(21_000, 20_000, 500, "test timing");
    }

    #[test]
    fn test_average_duration() {
        let durations = vec![
            Duration::from_millis(100),
            Duration::from_millis(200),
            Duration::from_millis(150),
        ];
        let avg = average_duration(&durations);
        assert_eq!(avg.as_millis(), 150);
    }
}
