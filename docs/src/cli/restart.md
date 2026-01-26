# restart

Restart devices or services.

## restart device

Restart one or more devices.

### Usage

```sh
sm restart device [DEVICE_SELECTOR] [OPTIONS]
sm restart d [DEVICE_SELECTOR] [OPTIONS]        # Using alias
```

### Options

- `--yes` (`-y`): Skip confirmation prompt
- `--nowait`: Don't wait for result, just queue the command
- `--label KEY=VALUE` (`-l`): Filter by labels
- `--online`: Show only online devices
- `--offline`: Show only offline devices
- `--search` (`-s`): Enable partial matching for device IDs

### Examples

```sh
# Restart a device (with confirmation)
sm restart d ABC123

# Restart multiple devices without confirmation
sm restart d -l env=staging -y

# Queue restart for all offline devices
sm restart d --offline --nowait

# Restart specific devices
sm restart d ABC123 XYZ789 -y

# Search and restart
sm restart d rpi -s -y
```

### Confirmation

When restarting devices, the CLI will show a preview of up to 10 devices that will be affected. If there are more than 10 devices, it will show the first 10 and indicate how many more devices will be restarted.

You can skip this confirmation with the `--yes` (`-y`) flag.

## restart service

Restart a systemd service on devices (runs 'systemctl restart <unit>').

### Usage

```sh
sm restart service <UNIT> [DEVICE_SELECTOR] [OPTIONS]
sm restart svc <UNIT> [DEVICE_SELECTOR] [OPTIONS]      # Using alias
```

### Arguments

- `UNIT`: Service unit name (e.g., nginx, smithd, docker)

### Options

- `--yes` (`-y`): Skip confirmation prompt
- `--nowait`: Don't wait for result, just queue the command
- `--label KEY=VALUE` (`-l`): Filter by labels
- `--online`: Show only online devices
- `--offline`: Show only offline devices
- `--search` (`-s`): Enable partial matching for device IDs

### Examples

```sh
# Restart nginx on a device
sm restart svc nginx ABC123

# Restart smithd on all production devices
sm restart svc smithd -l env=production -y

# Queue restart for multiple services
sm restart svc docker ABC123 XYZ789 --nowait

# Search and restart service
sm restart svc nginx rpi -s -y
```
