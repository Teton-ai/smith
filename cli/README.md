# Smith CLI (`sm`)

Command-line interface for managing your Smith fleet.

## Installation

### Quick Install

```bash
curl -fsSL https://docs.smith.teton.ai/install.sh | sh
```

### Manual Install

Download the latest release from the [releases page](https://github.com/teton-ai/smith/releases) or build from source:

```bash
cargo build --release --bin sm
```

The binary will be available at `target/release/sm`.

## Configuration

On first run, `sm` will prompt you to create a configuration file. The CLI stores its configuration in your system's config directory:

- Linux: `~/.config/smith/config.toml`
- macOS: `~/Library/Application Support/smith/config.toml`
- Windows: `%APPDATA%\smith\config.toml`

## Authentication

Before using the CLI, you need to authenticate:

```bash
sm auth login
```

This will open your browser for authentication. Use `--no-open` to get a URL to paste manually.

To view your current authentication status:

```bash
sm auth show
```

To log out:

```bash
sm auth logout
```

## Commands

### Devices

List all devices in your fleet:

```bash
sm devices ls
```

Add `--json` flag for JSON output.

### Distributions

List available distributions:

```bash
sm distributions ls
# or use the alias
sm distro ls
```

Add `--json` flag for JSON output.

### Releases

View release information:

```bash
sm release <release_number>
```

Deploy a release to your fleet:

```bash
sm release <release_number> --deploy
```

The CLI will poll the deployment status and report when complete (or timeout after 5 minutes).

### Tunnel

Open an SSH tunnel to a device:

```bash
sm tunnel <serial_number>
```

This creates a secure SSH connection through the Smith infrastructure to access your device remotely.

### Profiles

Switch between different configuration profiles:

```bash
sm profile <profile_name>
```

View current profile:

```bash
sm profile
```

### Updates

The CLI automatically checks for updates every 24 hours. To manually update:

```bash
sm update
```

To check for updates without installing:

```bash
sm update --check
```

### Shell Completion

Generate shell completion scripts:

```bash
# Bash
sm completion bash > /etc/bash_completion.d/sm

# Zsh
sm completion zsh > ~/.zsh/completion/_sm

# Fish
sm completion fish > ~/.config/fish/completions/sm.fish

# PowerShell
sm completion powershell > sm.ps1
```

## Examples

```bash
# List all devices with their online status
sm devices ls

# Deploy a release and wait for completion
sm release v1.2.3 --deploy

# Open an SSH session to a specific device
sm tunnel ABC123456

# Get device list as JSON for scripting
sm devices ls --json | jq '.[] | select(.last_seen > "2025-01-01")'
```

## License

Apache License 2.0
