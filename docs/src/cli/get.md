# get

The `get` command retrieves information about resources in your fleet. Use it to inspect device details, view command history, and filter resources by various criteria.

**Available Resources:**
- `device` (alias: `d`, `devices`) - View device information and metadata
- `commands` (alias: `cmds`) - View command history for devices

## Quick Examples

```sh
# List all devices
sm get d

# Get a specific device
sm get d ABC123

# Get only online devices
sm get d --online

# Search for devices by partial match
sm get d rpi -s

# View command history for a device
sm get cmds ABC123

# Filter devices by label
sm get d -l env=production
```

---

## get device

Display information about one or more devices in your fleet. Without any arguments, lists all devices. You can filter by specific device IDs, labels, online status, or use partial matching with the search flag.

### Usage

```sh
sm get device [IDS...] [OPTIONS]
sm get d [IDS...] [OPTIONS]        # Using alias
```

### Options

- `--json` (`-j`): Output as JSON
- `--output FORMAT` (`-o`): Output format (wide, json, or field name like serial_number, id, ip_address)
- `--label KEY=VALUE` (`-l`): Filter by labels (can be used multiple times)
- `--online`: Show only online devices (last seen < 5 minutes)
- `--offline`: Show only offline devices (last seen >= 5 minutes)
- `--search` (`-s`): Enable partial matching for device IDs

### Examples

```sh
# List all devices
sm get d

# Get specific device
sm get d ABC123

# Get devices with custom output
sm get d -o wide

# Get only serial numbers
sm get d -o serial_number

# JSON output
sm get d -j

# Get all online devices
sm get d --online

# Search for devices by partial match
sm get d rpi -s

# Filter by multiple labels
sm get d -l env=production -l region=us-west
```

---

## get commands

View the history of commands sent to devices. This shows recent operations like restarts, status checks, custom commands, and their current execution status. Useful for tracking what actions have been performed on your devices and checking if commands have completed.

### Usage

```sh
sm get commands [DEVICE_SELECTOR] [OPTIONS]
sm get cmds [DEVICE_SELECTOR] [OPTIONS]    # Using alias
```

### Options

- `--limit N`: Number of commands to show per device (default: 10)
- `--json` (`-j`): Output as JSON
- `--label KEY=VALUE` (`-l`): Filter by labels
- `--online`: Show only online devices
- `--offline`: Show only offline devices
- `--search` (`-s`): Enable partial matching for device IDs

### Examples

```sh
# Get last 10 commands for a device
sm get cmds ABC123

# Get last 50 commands
sm get cmds ABC123 --limit 50

# Get commands for multiple devices
sm get cmds -l env=production

# Search and get commands
sm get cmds rpi -s --limit 20
```

### Command Output

The output shows:
- **Command ID**: Unique identifier for the command
- **Issued At**: When the command was sent
- **Command Type**: What kind of command (Restart, FreeForm, UpdateVariables, etc.)
- **Status**: Current status (Pending, Fetched, Completed, Cancelled)

For FreeForm commands, the actual command text is displayed (e.g., `systemctl status smithd`).
