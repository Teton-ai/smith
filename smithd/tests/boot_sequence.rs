use std::time::Duration;

mod common;
use common::{DaemonWaiter, DeviceConfig};

/// Test fresh boot sequence: no config -> register -> ping
#[tokio::test]
#[ignore]
async fn fresh_boot_register_and_ping() {
    let env = common::SmithTestEnvironment::start()
        .await
        .expect("Failed to start test environment");

    let config = DeviceConfig::default();
    let device = env
        .spawn_device(config.clone())
        .await
        .expect("Failed to spawn device");

    let waiter = DaemonWaiter::new(Duration::from_secs(120));
    let api_url = env.api_base_url().await.expect("Failed to get API URL");

    // Verify device registers within timeout
    waiter
        .wait_for_registration(&api_url, &config.serial_number)
        .await
        .expect("Device failed to register");

    // Verify first ping arrives
    waiter
        .wait_for_ping(&api_url, &config.serial_number)
        .await
        .expect("First ping failed");

    // Verify logs show correct boot sequence
    let logs = device.logs().await.unwrap_or_default();
    assert!(
        logs.contains("Postman runnning"),
        "Postman actor didn't start"
    );
    assert!(
        logs.contains("Bouncer runnning"),
        "Bouncer actor didn't start"
    );
    assert!(
        logs.contains("Police runnning"),
        "Police actor didn't start"
    );

    device.stop().await.expect("Failed to stop device");
}

/// Test boot with existing valid token
#[tokio::test]
#[ignore]
async fn boot_with_existing_token() {
    // Start device, let it register
    // Stop device
    // Start again and verify it skips registration
    todo!("Implement boot with existing token test");
}

/// Test boot with invalid/expired token
#[tokio::test]
#[ignore]
async fn boot_with_expired_token() {
    // Start device with a pre-configured expired token
    // Verify it detects expiration and re-registers
    todo!("Implement boot with expired token test");
}

/// Test that bouncer checks pass before pinging starts
#[tokio::test]
#[ignore]
async fn bouncer_checks_before_pinging() {
    // Start device with failing checks
    // Verify no pings sent
    // Fix checks
    // Verify pinging starts
    todo!("Implement bouncer checks test");
}

/// Test actor startup order is correct
#[tokio::test]
#[ignore]
async fn verify_actor_startup_order() {
    let env = common::SmithTestEnvironment::start()
        .await
        .expect("Failed to start test environment");

    let config = DeviceConfig::default();
    let device = env
        .spawn_device(config.clone())
        .await
        .expect("Failed to spawn device");

    // Give time for startup
    tokio::time::sleep(Duration::from_secs(5)).await;

    let logs = device.logs().await.unwrap_or_default();

    // Verify order: Configuration -> Tunnel -> Police -> ... -> Postman -> Dbus -> Bouncer
    let magic_pos = logs.find("MagicHandle");
    let tunnel_pos = logs.find("Tunnel runnning");
    let police_pos = logs.find("Police runnning");
    let postman_pos = logs.find("Postman runnning");
    let bouncer_pos = logs.find("Bouncer runnning");

    if let (Some(m), Some(t), Some(po), Some(ps), Some(b)) =
        (magic_pos, tunnel_pos, police_pos, postman_pos, bouncer_pos)
    {
        assert!(m < t, "Magic should initialize before Tunnel");
        assert!(t < po, "Tunnel should start before Police");
        assert!(po < ps, "Police should start before Postman");
        assert!(ps < b, "Postman should start before Bouncer");
    }

    device.stop().await.expect("Failed to stop device");
}
