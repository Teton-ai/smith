# Integration Testing Quick Start

## TL;DR

```bash
# 1. Build test images
./scripts/build-test-images.sh

# 2. Run all integration tests
cargo test --package smith --test '*' -- --ignored --nocapture --test-threads=1

# 3. Run specific test
cargo test --package smith daemon_pings_every_20_seconds_consistently -- --ignored --nocapture
```

## What Got Implemented

### ✅ Core Tests (Ready to Run)
1. **Ping Consistency** - Verifies daemon pings every 20s
2. **Network Recovery** - Daemon recovers after 30s disconnect
3. **API Downtime** - Daemon handles API unavailability
4. **Boot Sequence** - Fresh boot → register → ping flow
5. **Graceful Shutdown** - SIGTERM/SIGKILL handling
6. **Multi-Device** - 10 devices pinging simultaneously

### 🚧 Skeleton Tests (Need Implementation)
- Slow API responses
- Token expiration & re-registration
- Network latency/packet loss
- Police restart mechanism
- Boot with existing token
- And more... (see individual test files)

## File Structure

```
smithd/tests/
├── README.md              # Full documentation
├── QUICKSTART.md         # This file
├── common/               # Test utilities
│   ├── containers.rs     # Docker env management
│   ├── daemon.rs         # Daemon helpers
│   └── assertions.rs     # Timing assertions
├── integration/          # Integration tests
│   ├── ping_consistency.rs
│   └── network_resilience.rs
└── scenarios/            # E2E scenarios
    ├── boot_sequence.rs
    ├── graceful_shutdown.rs
    └── concurrent_devices.rs
```

## Key Features

### 1. Isolated Test Environments
Each test gets its own:
- PostgreSQL container
- API server container
- Bore tunnel server
- Isolated network

### 2. Custom Assertions
```rust
// Verify timing with tolerance
assert_timing_within(actual_ms, expected_ms, tolerance_ms, "message");

// Verify consistent intervals
assert_intervals_consistent(&durations, expected, tolerance);
```

### 3. Daemon Lifecycle Helpers
```rust
let env = SmithTestEnvironment::start().await?;
let device = env.spawn_device(config).await?;

// Wait for daemon state
waiter.wait_for_registration(&api_url, &serial).await?;
waiter.wait_for_ping(&api_url, &serial).await?;
```

## Common Commands

### Run Test Categories
```bash
# Ping tests only
cargo test --package smith --test ping_consistency -- --ignored --nocapture

# Network tests only
cargo test --package smith --test network_resilience -- --ignored --nocapture

# Scenario tests only
cargo test --package smith --test boot_sequence -- --ignored --nocapture
cargo test --package smith --test graceful_shutdown -- --ignored --nocapture
cargo test --package smith --test concurrent_devices -- --ignored --nocapture
```

### Debug a Test
```bash
RUST_LOG=debug cargo test --package smith your_test_name -- --ignored --nocapture
```

### Check Test Compilation (No Run)
```bash
cargo test --package smith --no-run
```

## Expected Test Duration

| Test | Duration | Resource Usage |
|------|----------|----------------|
| `daemon_pings_every_20_seconds_consistently` | ~2 min | Low |
| `daemon_recovers_after_network_disconnect` | ~1 min | Low |
| `ten_devices_ping_simultaneously` | ~3 min | High |
| `ping_timing_no_drift_over_10_minutes` | ~10 min | Medium |

## Troubleshooting

### Issue: Images not found
```
Error: Failed to start test environment
```
**Fix:**
```bash
./scripts/build-test-images.sh
```

### Issue: Docker not running
```
Error: Cannot connect to the Docker daemon
```
**Fix:**
```bash
sudo systemctl start docker
# or
sudo dockerd
```

### Issue: Port conflicts
```
Error: Address already in use
```
**Fix:**
```bash
docker ps  # Find conflicting containers
docker stop <container_id>
```

### Issue: Test hangs
**Fix:**
1. Check Docker containers: `docker ps`
2. Check container logs: `docker logs <container_id>`
3. Kill hung test: `Ctrl+C`
4. Clean up containers: `docker rm -f $(docker ps -aq)`

## Next Steps

1. **Run the tests** to verify your setup
2. **Implement TODO tests** based on your needs
3. **Add custom scenarios** for your specific use cases
4. **Integrate with CI/CD** (already configured in `.github/workflows/test.yml`)

## Need Help?

- Full docs: `smithd/tests/README.md`
- Implementation details: `smithd/INTEGRATION_TESTING.md`
- Report issues: GitHub Issues

---

**Happy Testing!** 🚀
