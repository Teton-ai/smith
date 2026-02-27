use std::time::Duration;
use tokio::time::sleep;

mod common;
use common::{DaemonWaiter, DeviceConfig};

/// Test graceful shutdown with SIGTERM
#[tokio::test]
#[ignore]
async fn graceful_shutdown_sigterm() {
    let env = common::SmithTestEnvironment::start()
        .await
        .expect("Failed to start test environment");

    let config = DeviceConfig::default();
    let device = env
        .spawn_device(config.clone())
        .await
        .expect("Failed to spawn device");

    let waiter = DaemonWaiter::default();
    let api_url = env.api_base_url().await.expect("Failed to get API URL");

    // Wait for device to be running
    waiter
        .wait_for_registration(&api_url, &config.serial_number)
        .await
        .expect("Device failed to register");

    // TODO: testcontainers 0.23 doesn't support exec()
    // For now, we'll just stop the container which simulates shutdown
    // eprintln!("Note: Using container stop instead of exec killall");

    // Wait a moment for shutdown
    sleep(Duration::from_secs(2)).await;

    // Verify logs show graceful shutdown
    let logs = device.logs().await.unwrap_or_default();
    assert!(
        logs.contains("Agent is shutting down"),
        "Expected graceful shutdown message"
    );
    assert!(
        logs.contains("Postman task shut down")
            || logs.contains("Bouncer task shut down")
            || logs.contains("Police task shut down"),
        "Expected actor shutdown messages"
    );

    device.stop().await.expect("Failed to stop device");
}

/// Test shutdown during active ping
#[tokio::test]
#[ignore]
async fn shutdown_during_ping() {
    let env = common::SmithTestEnvironment::start()
        .await
        .expect("Failed to start test environment");

    let config = DeviceConfig::default();
    let device = env
        .spawn_device(config.clone())
        .await
        .expect("Failed to spawn device");

    let waiter = DaemonWaiter::default();
    let api_url = env.api_base_url().await.expect("Failed to get API URL");

    waiter
        .wait_for_registration(&api_url, &config.serial_number)
        .await
        .expect("Device failed to register");

    // Wait for a ping to be in progress, then stop
    sleep(Duration::from_secs(10)).await;

    // TODO: testcontainers 0.23 doesn't support exec()

    sleep(Duration::from_secs(2)).await;

    let logs = device.logs().await.unwrap_or_default();

    // Should not contain panic or error messages related to aborted requests
    assert!(
        !logs.contains("panic"),
        "Daemon should not panic during shutdown"
    );

    device.stop().await.expect("Failed to stop device");
}

/// Test that no state corruption occurs after unclean shutdown
#[tokio::test]
#[ignore]
async fn no_state_corruption_after_sigkill() {
    let env = common::SmithTestEnvironment::start()
        .await
        .expect("Failed to start test environment");

    let config = DeviceConfig::default();
    let device = env
        .spawn_device(config.clone())
        .await
        .expect("Failed to spawn device");

    let waiter = DaemonWaiter::default();
    let api_url = env.api_base_url().await.expect("Failed to get API URL");

    waiter
        .wait_for_registration(&api_url, &config.serial_number)
        .await
        .expect("Device failed to register");

    // TODO: testcontainers 0.23 doesn't support exec()
    // This test needs Docker CLI or testcontainers upgrade
    eprintln!("Test skipped: testcontainers 0.23 doesn't support exec()");

    device.stop().await.expect("Failed to stop device");
}

/// Test shutdown releases all resources properly
#[tokio::test]
#[ignore]
async fn shutdown_releases_resources() {
    // Verify no leaked file descriptors
    // Verify no zombie processes
    // Verify no stale lock files
    todo!("Implement resource cleanup verification test");
}
