use std::time::{Duration, Instant};

mod common;
use common::{DaemonWaiter, DeviceConfig, assert_intervals_consistent, assert_timing_within};

/// Test that daemon pings consistently every 20 seconds
/// This is a basic smoke test to verify the core heartbeat mechanism
#[tokio::test]
#[ignore] // Ignored by default as it takes 2+ minutes
async fn daemon_pings_every_20_seconds_consistently() {
    // Note: This test requires the API and device Docker images to be built
    // Run: docker build -f api.Dockerfile -t smith-api:test .
    // Run: docker build -f device.Dockerfile -t smith-device:test .

    let env = match common::SmithTestEnvironment::start().await {
        Ok(env) => env,
        Err(e) => {
            eprintln!("Failed to start test environment: {}", e);
            eprintln!("Make sure Docker images are built:");
            eprintln!("  docker build -f api.Dockerfile -t smith-api:test .");
            eprintln!("  docker build -f device.Dockerfile -t smith-device:test .");
            panic!("Test environment setup failed");
        }
    };

    let config = DeviceConfig::default();
    let device = env
        .spawn_device(config.clone())
        .await
        .expect("Failed to spawn device");

    let waiter = DaemonWaiter::default();
    let api_url = env.api_base_url().await.expect("Failed to get API URL");

    // Wait for device to register
    waiter
        .wait_for_registration(&api_url, &config.serial_number)
        .await
        .expect("Device failed to register");

    // Collect ping timestamps for 2 minutes (6 pings expected)
    let test_duration = Duration::from_secs(120);
    let start = Instant::now();
    let mut ping_times = vec![Instant::now()];

    while start.elapsed() < test_duration {
        match waiter.wait_for_ping(&api_url, &config.serial_number).await {
            Ok(_) => {
                ping_times.push(Instant::now());
            }
            Err(e) => {
                eprintln!("Error waiting for ping: {}", e);
                break;
            }
        }
    }

    // Calculate intervals between pings
    let intervals: Vec<Duration> = ping_times
        .windows(2)
        .map(|w| w[1].duration_since(w[0]))
        .collect();

    // Verify we got at least 5 pings in 2 minutes
    assert!(
        intervals.len() >= 5,
        "Expected at least 5 ping intervals, got {}",
        intervals.len()
    );

    // Verify intervals are consistent (20s ± 1s tolerance for test environment)
    assert_intervals_consistent(&intervals, Duration::from_secs(20), Duration::from_secs(1));

    // Calculate and verify average interval
    let avg_interval_ms: u128 =
        intervals.iter().map(|d| d.as_millis()).sum::<u128>() / intervals.len() as u128;
    assert_timing_within(avg_interval_ms, 20_000, 1_000, "Average ping interval");

    // Cleanup
    device.stop().await.expect("Failed to stop device");
}

/// Test that daemon continues pinging even when API responds slowly
#[tokio::test]
#[ignore]
async fn daemon_handles_slow_api_responses() {
    // This test would use wiremock to simulate slow API responses
    // and verify the daemon doesn't accumulate delays
    todo!("Implement slow API response test");
}

/// Test that daemon recovers if it misses sending a ping
#[tokio::test]
#[ignore]
async fn daemon_recovers_from_missed_ping() {
    // This test would block network temporarily and verify
    // daemon continues pinging after network restores
    todo!("Implement missed ping recovery test");
}

/// Test ping payload contains all required fields
#[tokio::test]
#[ignore]
async fn ping_payload_validation() {
    // Verify ping contains:
    // - correct serial number
    // - system info
    // - command responses
    // - release_id
    todo!("Implement ping payload validation test");
}

/// Test that daemon doesn't drift in timing over longer periods
#[tokio::test]
#[ignore] // This takes 10+ minutes
async fn ping_timing_no_drift_over_10_minutes() {
    // Run for 10 minutes and verify no cumulative drift
    todo!("Implement long-running timing drift test");
}
