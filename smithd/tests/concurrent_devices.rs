use std::time::Duration;
use tokio::time::sleep;

mod common;
use common::{DaemonWaiter, DeviceConfig};

/// Test that multiple devices can ping simultaneously without issues
#[tokio::test]
#[ignore] // This test takes several minutes and requires significant resources
async fn ten_devices_ping_simultaneously() {
    let env = common::SmithTestEnvironment::start()
        .await
        .expect("Failed to start test environment");

    let device_count = 10;
    let mut devices = Vec::new();

    // Spawn 10 devices
    for i in 0..device_count {
        let mut config = DeviceConfig::default();
        config.serial_number = format!("TEST-DEVICE-{:03}", i);
        config.wifi_mac = format!("00:11:22:33:44:{:02x}", i);

        let device = env
            .spawn_device(config)
            .await
            .expect(&format!("Failed to spawn device {}", i));

        devices.push(device);
    }

    let waiter = DaemonWaiter::new(Duration::from_secs(120));
    let api_url = env.api_base_url().await.expect("Failed to get API URL");

    // Wait for all devices to register
    for device in &devices {
        waiter
            .wait_for_registration(&api_url, &device.serial_number)
            .await
            .expect(&format!(
                "Device {} failed to register",
                device.serial_number
            ));
    }

    // Wait for all devices to send at least one ping
    for device in &devices {
        waiter
            .wait_for_ping(&api_url, &device.serial_number)
            .await
            .expect(&format!("Device {} failed to ping", device.serial_number));
    }

    // Let them run for 1 minute to verify stability
    sleep(Duration::from_secs(60)).await;

    // Verify all devices are still responsive
    for device in &devices {
        let ping = waiter.wait_for_ping(&api_url, &device.serial_number).await;
        assert!(
            ping.is_ok(),
            "Device {} stopped responding",
            device.serial_number
        );
    }

    // Cleanup
    for device in devices {
        device.stop().await.expect("Failed to stop device");
    }
}

/// Test API handles burst of pings at startup
#[tokio::test]
#[ignore]
async fn api_handles_startup_ping_burst() {
    // Start 20 devices simultaneously
    // All will try to register and ping at roughly the same time
    // Verify no rate limiting errors
    // Verify all succeed
    todo!("Implement startup burst test");
}

/// Test devices maintain timing even under API load
#[tokio::test]
#[ignore]
async fn ping_timing_consistent_under_load() {
    // Start 10 devices
    // Let them run for 5 minutes
    // Verify each device maintains ~20s intervals
    // Even though API is handling 10x the requests
    todo!("Implement timing under load test");
}

/// Test that one failing device doesn't affect others
#[tokio::test]
#[ignore]
async fn device_failure_isolation() {
    // Start 5 devices
    // Kill one device abruptly
    // Verify other 4 continue operating normally
    todo!("Implement failure isolation test");
}
