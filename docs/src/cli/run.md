# run

Run custom commands on devices.

## Usage

```sh
sm run [DEVICE_SELECTOR] [OPTIONS] -- <COMMAND>
```

## Options

- `--device ID` (`-d`): Specific device serial numbers or IDs to target (can be used multiple times)
- `--wait` (`-w`): Wait for command results (polls until completion)
- `--label KEY=VALUE` (`-l`): Filter by labels
- `--online`: Show only online devices
- `--offline`: Show only offline devices
- `--search` (`-s`): Enable partial matching for device IDs

## Examples

```sh
# Run a command on a device (async by default)
sm run ABC123 -- uptime

# Run and wait for results
sm run ABC123 -w -- df -h

# Run on multiple devices
sm run -l env=production -- systemctl status smithd

# Run on specific devices using --device flag
sm run -d ABC123 -d XYZ789 -- free -h

# Search and run
sm run rpi -s -- cat /proc/cpuinfo

# Complex commands with pipes
sm run ABC123 -w -- "dmesg | grep -i error | tail -n 20"
```

## Notes

- By default, commands are queued asynchronously and return immediately
- Use `--wait` (`-w`) to poll for results until the command completes
- Use `--` to separate the device selector from the command to run
- Commands are executed via `smithd` on the target devices
- You can check command results later using `sm command <device_id>:<command_id>`
- For commands with pipes or special shell characters, wrap the entire command in quotes
