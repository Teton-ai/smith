# label

Set labels on devices.

## Usage

```sh
sm label [DEVICE_SELECTOR] <KEY=VALUE>...
```

## Arguments

- `KEY=VALUE`: Labels to set on the devices (can specify multiple)

## Options

- `--device ID` (`-d`): Specific device serial numbers or IDs to target (can be used multiple times)
- `--label KEY=VALUE` (`-l`): Filter by existing labels
- `--online`: Show only online devices
- `--offline`: Show only offline devices
- `--search` (`-s`): Enable partial matching for device IDs

## Examples

```sh
# Set a label on a device
sm label ABC123 env=production

# Set multiple labels
sm label ABC123 env=production region=us-west

# Set labels on multiple devices
sm label ABC123 XYZ789 env=testing

# Set labels on devices matching a filter
sm label -l region=us-east env=production

# Search and set labels
sm label rpi -s location=warehouse-1

# Set labels on all online devices
sm label --online status=active
```

## Notes

- Labels are key-value pairs that help organize and filter devices
- Setting a label will overwrite any existing value for that key
- You can use labels to filter devices in other commands (e.g., `sm get d -l env=production`)
- Common label use cases:
  - Environment: `env=production`, `env=staging`, `env=development`
  - Region/Location: `region=us-west`, `location=warehouse-1`
  - Device type: `type=gateway`, `type=sensor`
  - Team/Owner: `team=backend`, `owner=john`
