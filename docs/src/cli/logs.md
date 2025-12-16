# logs

Get logs from devices (runs 'journalctl -r -n 500').

## Usage

```sh
sm logs [DEVICE_SELECTOR] [OPTIONS]
```

## Options

- `--nowait`: Don't wait for result, just queue the command
- `--label KEY=VALUE` (`-l`): Filter by labels
- `--online`: Show only online devices
- `--offline`: Show only offline devices
- `--search` (`-s`): Enable partial matching for device IDs

## Examples

```sh
# Get logs from a device
sm logs ABC123

# Queue logs command for later retrieval
sm logs ABC123 --nowait

# Get logs from devices by label
sm logs -l env=production

# Search and get logs
sm logs rpi -s
```

## Notes

- This command runs `journalctl -r -n 500` on the device, which shows the last 500 log entries in reverse chronological order (newest first)
- The logs are retrieved from the system journal (systemd journal)
- You can use `--nowait` to queue the command and check results later with `sm command <id>`
