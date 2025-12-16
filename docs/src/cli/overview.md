# CLI Overview

## Command line tool (sm)

Smith provides a command line tool for communicating with the Smith API to manage your fleet of devices.

This tool is named `sm`.

For configuration, `sm` looks for configuration files in the `$HOME/.config/smith` directory. You can manage multiple profiles and switch between them using the `sm profile` command.

This overview covers `sm` syntax, describes the command operations, and provides common examples. For details about each command, see the individual command reference pages.

## Installation

See the [Installation Guide](./installation.md) for installation instructions.

## Syntax

Use the following syntax to run `sm` commands from your terminal window:

```
sm [command] [TYPE] [NAME] [flags]
```

where `command`, `TYPE`, `NAME`, and `flags` are:

- **command**: Specifies the operation that you want to perform on one or more resources, for example `get`, `status`, `restart`, `logs`.

- **TYPE**: Specifies the resource type. Resource types support aliases and you can specify the full name or abbreviated forms. For example, the following commands produce the same output:
  ```sh
  sm get device ABC123
  sm get devices ABC123
  sm get d ABC123
  ```

- **NAME**: Specifies the name of the resource (device serial number or ID). Names are case-sensitive. If the name is omitted, details for all resources are displayed, for example `sm get device`.

  When performing an operation on multiple resources, you can specify each resource by name or use filters:

  - **To specify multiple resources by name**:
    ```sh
    sm get device ABC123 XYZ789 DEF456
    ```

  - **To filter resources by labels**:
    ```sh
    sm get device --label env=production --label region=us-west
    sm get device -l env=production -l region=us-west
    ```

  - **To filter resources by status**:
    ```sh
    sm get device --online
    sm get device --offline
    ```

  - **To search with partial matching**:
    ```sh
    sm get device rpi nano --search
    sm get device rpi -s
    ```

- **flags**: Specifies optional flags. For example, you can use the `--json` flag to output results in JSON format, or `--nowait` to queue commands asynchronously.

## Common Resource Type Aliases

| Resource Type | Aliases | Example |
|--------------|---------|---------|
| `device` | `devices`, `d` | `sm get d --online` |
| `service` | `services`, `svc` | `sm status svc nginx ABC123` |
| `commands` | `cmds` | `sm get cmds ABC123` |

## Device Selection

Most commands support flexible device selection through a common set of flags:

- **Positional IDs**: Specify one or more device serial numbers or IDs
- `--label KEY=VALUE` (`-l`): Filter by labels (can be used multiple times)
- `--online`: Show only online devices (last seen < 5 minutes)
- `--offline`: Show only offline devices (last seen >= 5 minutes)
- `--search` (`-s`): Enable partial matching for device IDs (matches serial number, hostname, or model)

### Examples

```sh
# Single device by serial number
sm get d ABC123

# Multiple devices
sm get d ABC123 XYZ789

# All devices with a label
sm get d -l env=production

# Search for devices (partial match)
sm get d rpi -s

# Multiple search terms
sm get d rpi nano -s

# Combine filters
sm get d -l env=staging --online
```

## Authentication

Before using the CLI, you need to authenticate:

```sh
sm auth login
```

This will open your browser for authentication. Use `--no-open` to prevent automatic browser launch.

To logout:
```sh
sm auth logout
```

To view your current token:
```sh
sm auth show
```

## Common Workflows

### Checking device status

```sh
# List all devices
sm get d

# Check which devices are online
sm get d --online

# Check smithd status on a device
sm status d ABC123

# Check if updates are available
sm status d -l env=production
```

### Troubleshooting

```sh
# Get device logs
sm logs ABC123

# Check service status
sm status svc smithd ABC123

# Restart a service
sm restart svc smithd ABC123

# Test network connectivity
sm test-network ABC123

# Run custom diagnostic command
sm run ABC123 -w -- dmesg | tail -n 50
```

### Managing labels

```sh
# Add environment label
sm label ABC123 env=production

# Query by label
sm get d -l env=production

# Update multiple devices
sm label -s rpi region=warehouse-1
```

## Tips

- **Use aliases** to save typing: `sm get d` instead of `sm get device`, `sm status svc` instead of `sm status service`
- **Use short flags** where available: `-l` for `--label`, `-s` for `--search`, `-j` for `--json`, `-o` for `--output`
- Use `--search` (`-s`) flag for fuzzy matching when you don't remember the full serial number
- Combine multiple labels to narrow down device selection: `sm get d -l env=prod -l region=us`
- Use `--nowait` for bulk operations to queue commands asynchronously
- Use `sm get cmds` to check the history of commands sent to a device
- Most commands support `--json` (`-j`) flag for machine-readable output
- Use `sm agent-help` to get a comprehensive markdown reference for automation

## Command Reference

- [get](./cli-get.md) - Retrieve information about resources
- [status](./cli-status.md) - Get status information for devices or services
- [restart](./cli-restart.md) - Restart devices or services
- [logs](./cli-logs.md) - Get logs from devices
- [run](./cli-run.md) - Run custom commands on devices
- [label](./cli-label.md) - Set labels on devices
- [Other Commands](./cli-other.md) - Additional utility commands
