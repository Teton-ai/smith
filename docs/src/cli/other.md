# Other Commands

Additional utility commands for managing your Smith CLI and devices.

## test-network

Test network speed for devices (downloads 20MB test file).

### Usage

```sh
sm test-network [DEVICE_SELECTOR]
```

### Examples

```sh
# Test network for a device
sm test-network ABC123

# Test network for all online devices
sm test-network --online

# Test network for devices by label
sm test-network -l env=production
```

## command

Check command results by ID.

### Usage

```sh
sm command <DEVICE_ID:COMMAND_ID>...
```

### Examples

```sh
# Check a specific command result
sm command 123:456

# Check multiple commands
sm command 123:456 789:012
```

### Notes

- Command IDs are returned when you use `--nowait` flag
- You can also get command IDs from `sm get cmds <device>`

## tunnel

Tunnel into a device.

### Usage

```sh
sm tunnel <SERIAL_NUMBER> [OPTIONS]
```

### Options

- `--overview-debug`: Setup for overview debug

### Examples

```sh
sm tunnel ABC123
sm tunnel ABC123 --overview-debug
```

## profile

Manage CLI profiles.

### Usage

```sh
sm profile [PROFILE_NAME]
```

### Examples

```sh
# Show current profile
sm profile

# Switch to a different profile
sm profile production
```

## distributions

List distributions and releases.

### Usage

```sh
# List distributions
sm distributions ls
sm distributions ls --json
sm distros ls               # Using alias

# List distribution releases
sm distributions releases
sm distros releases         # Using alias
```

## releases

Commands related to releases.

### Usage

```sh
sm releases <subcommand>
```

See `sm releases --help` for available subcommands.

## completion

Generate shell completion scripts.

### Usage

```sh
sm completion <SHELL>
```

### Supported shells

- bash
- zsh
- fish
- powershell
- elvish

### Examples

```sh
# Generate bash completion
sm completion bash > /usr/local/etc/bash_completion.d/sm

# Generate zsh completion
sm completion zsh > ~/.zsh/completion/_sm

# Generate fish completion
sm completion fish > ~/.config/fish/completions/sm.fish
```

## update

Update the CLI.

### Usage

```sh
sm update [OPTIONS]
```

### Options

- `--check`: Check for updates without installing

### Examples

```sh
# Check for updates
sm update --check

# Update the CLI
sm update
```

## agent-help

Print all available commands in markdown format (useful for AI agents).

### Usage

```sh
sm agent-help
```

This command outputs comprehensive documentation in markdown format, including all commands, flags, and examples. It's designed to be consumed by AI agents or automated systems.
