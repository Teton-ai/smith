# Other Commands

Additional utility commands for managing your Smith CLI and devices.

## approve

Approve devices so they can register and join the fleet. A device that has
registered but has not been approved shows up as pending and receives no token.

### Usage

```sh
sm approve [DEVICE_SELECTOR] [OPTIONS]
```

### Options

- `-y`, `--yes`: Skip the confirmation prompt

### Examples

```sh
# Approve one device
sm approve ABC123

# Approve every pending device carrying a label, without prompting
sm approve -l site=oslo -y
```

## revoke

Withdraw a device's approval. It stops receiving commands and updates, but the
server keeps its token, so approving it again later can leave it unable to
register. Use `unregister` when you want the device to pair again from scratch.

### Usage

```sh
sm revoke [DEVICE_SELECTOR] [OPTIONS]
```

### Options

- `-y`, `--yes`: Skip the confirmation prompt

### Examples

```sh
# Revoke one device
sm revoke ABC123
```

## unregister

Reset a device's enrollment, sending it back through approval as if it had never
been seen. Clears the approval, the token and the release target together, which
is the only combination that lets the device pair again from scratch.

### Usage

```sh
sm unregister [DEVICE_SELECTOR] [OPTIONS]
```

### Options

- `-y`, `--yes`: Skip the confirmation prompt

### Examples

```sh
# Unregister a re-imaged device
sm unregister ABC123

# Unregister several at once without prompting
sm unregister ABC123 DEF456 -y
```

### Notes

- The device drops offline within a ping or two, clears its local token by
  itself, and reappears under pending approval. Approve it again — with a
  release — to bring it back.
- Nothing is deleted. Command history, responses, the ledger, labels, variables
  and notes all stay on the device page.
- The reset is recorded in the device's ledger together with who ran it.
- This resets the device's record in Smith only. It does not touch credentials
  that other Teton services issue to the same machine.

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
