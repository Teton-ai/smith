# Smith Daemon Integration Tests

This directory contains integration tests for the `smithd` daemon. These tests verify the daemon's behavior in realistic scenarios, including network resilience, timing consistency, and multi-device coordination.

## Test Categories

### Integration Tests (`integration/`)
- **ping_consistency.rs**: Tests that the daemon pings the API every 20 seconds consistently
- **network_resilience.rs**: Tests daemon behavior under network failures, API downtime, and latency

### Scenario Tests (`scenarios/`)
- **boot_sequence.rs**: Tests various boot scenarios (fresh boot, existing token, expired token)
- **graceful_shutdown.rs**: Tests shutdown behavior (SIGTERM, SIGKILL, during ping)
- **concurrent_devices.rs**: Tests multiple devices operating simultaneously

## Prerequisites

### 1. Docker
All integration tests require Docker to be running. They use `testcontainers` to spin up isolated test environments.

### 2. Build Test Images
Before running tests, you must build the Docker images:

```bash
./scripts/build-test-images.sh
```

This builds:
- `smith-api:test` - The API server
- `smith-device:test` - The device container with smithd

## Running Tests

### Run All Integration Tests (Ignored by Default)
```bash
cargo test --package smith --test '*' -- --ignored --nocapture
```

### Run Specific Test Suite
```bash
# Ping consistency tests
cargo test --package smith --test ping_consistency -- --ignored --nocapture

# Network resilience tests
cargo test --package smith --test network_resilience -- --ignored --nocapture

# Boot sequence tests
cargo test --package smith --test boot_sequence -- --ignored --nocapture
```

### Run Individual Test
```bash
cargo test --package smith daemon_pings_every_20_seconds_consistently -- --ignored --nocapture
```

### Run Tests in Parallel (Careful!)
By default, tests run serially (`--test-threads=1` is recommended) because they use Docker resources:

```bash
cargo test --package smith --test '*' -- --ignored --nocapture --test-threads=1
```

## Test Structure

```
tests/
├── common/                      # Shared test utilities
│   ├── mod.rs
│   ├── containers.rs            # Docker container management
│   ├── daemon.rs                # Daemon lifecycle helpers
│   └── assertions.rs            # Custom assertions for timing
├── integration/                 # Integration tests
│   ├── ping_consistency.rs
│   └── network_resilience.rs
└── scenarios/                   # End-to-end scenario tests
    ├── boot_sequence.rs
    ├── graceful_shutdown.rs
    └── concurrent_devices.rs
```

## Writing New Tests

### Example Test

```rust
use std::time::Duration;

mod common;
use common::{DaemonWaiter, DeviceConfig};

#[tokio::test]
#[ignore]  // Mark as ignored so it doesn't run with `cargo test`
async fn my_new_test() {
    // Start test environment
    let env = common::SmithTestEnvironment::start()
        .await
        .expect("Failed to start test environment");

    // Spawn a device
    let config = DeviceConfig::default();
    let device = env.spawn_device(config.clone()).await.expect("Failed to spawn device");

    // Test logic here
    let waiter = DaemonWaiter::default();
    let api_url = env.api_base_url().await.expect("Failed to get API URL");

    waiter.wait_for_registration(&api_url, &config.serial_number)
        .await
        .expect("Device failed to register");

    // Cleanup
    device.stop().await.expect("Failed to stop device");
}
```

## Test Configuration

Tests use the following environment variables (optional):
- `RUST_LOG`: Set logging level (default: `info,smithd=debug`)
- `TEST_TIMEOUT`: Override default test timeout in seconds

## Common Issues

### Docker Images Not Found
```
Error: Failed to start test environment
```
**Solution**: Run `./scripts/build-test-images.sh`

### Port Conflicts
```
Error: Address already in use
```
**Solution**: Stop other containers or services using the same ports

### Tests Timeout
```
Error: Timeout waiting for device registration
```
**Solution**: 
- Check Docker is running
- Increase timeout in test
- Check API logs for errors

## CI/CD Integration

Integration tests are run in CI on pull requests. See `.github/workflows/test.yml` for configuration.

Long-running tests (5+ minutes) are marked with `#[ignore]` and only run nightly.

## Metrics Tracked

Professional IoT projects track these metrics. Our tests verify:

| Metric | Target | Test Coverage |
|--------|--------|---------------|
| Ping success rate | >99.9% | ✓ ping_consistency |
| Ping timing jitter | <500ms | ✓ ping_consistency |
| Recovery time after network loss | <30s | ✓ network_resilience |
| Memory stability (24h) | No leaks | ⚠️ TODO |
| CPU usage during idle | <5% | ⚠️ TODO |

## Future Improvements

- [ ] Add chaos engineering tests (random failures)
- [ ] Add property-based testing with proptest
- [ ] Add hardware-specific tests (ARM, Jetson)
- [ ] Add performance benchmarks
- [ ] Add memory leak detection
- [ ] Add long-running stability tests (24h+)

## References

- [Testcontainers Documentation](https://docs.rs/testcontainers/)
- [Smith Architecture Docs](../../docs/)
- Professional IoT daemon testing examples:
  - Eclipse Mosquitto
  - AWS IoT Greengrass
  - Zephyr RTOS
