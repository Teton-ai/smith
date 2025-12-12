# status

Get status information for devices or services.

## status device

Get smithd status for devices (runs 'smithd status' command).

Shows comprehensive update status including:
- Update/upgrade status (whether the system is up-to-date)
- Installed package versions (currently running on the device)
- Target package versions (versions that should be running)
- Update status flag for each package

### Usage

```sh
sm status device [DEVICE_SELECTOR] [OPTIONS]
sm status d [DEVICE_SELECTOR] [OPTIONS]        # Using alias
```

### Options

- `--nowait`: Don't wait for result, just queue the command (faster, use `sm command <id>` to check results later)
- `--label KEY=VALUE` (`-l`): Filter by labels
- `--online`: Show only online devices
- `--offline`: Show only offline devices
- `--search` (`-s`): Enable partial matching for device IDs

### Examples

```sh
# Get status for a device
sm status d ABC123

# Get status for multiple devices (async)
sm status d -l env=production --nowait

# Check status for all online devices
sm status d --online

# Search and check status
sm status d rpi -s
```

## status service

Get systemd service status on devices (runs 'systemctl status <unit>').

### Usage

```sh
sm status service <UNIT> [DEVICE_SELECTOR] [OPTIONS]
sm status svc <UNIT> [DEVICE_SELECTOR] [OPTIONS]      # Using alias
```

### Arguments

- `UNIT`: Service unit name (e.g., nginx, smithd, docker)

### Options

- `--nowait`: Don't wait for result, just queue the command
- `--label KEY=VALUE` (`-l`): Filter by labels
- `--online`: Show only online devices
- `--offline`: Show only offline devices
- `--search` (`-s`): Enable partial matching for device IDs

### Examples

```sh
# Check nginx status on a device
sm status svc nginx ABC123

# Check smithd status on all production devices
sm status svc smithd -l env=production

# Check docker on multiple devices
sm status svc docker ABC123 XYZ789

# Search and check service status
sm status svc nginx rpi -s
```
