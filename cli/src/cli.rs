use clap::{Args, Parser, Subcommand, value_parser};
use clap_complete::Shell;

use crate::commands::releases::ReleasesCommands;

#[derive(Parser)]
#[command(name = "sm", version, about = "Smith CLI - Fleet management tool", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Common device selection arguments
#[derive(Args, Debug, Clone)]
pub struct DeviceSelector {
    /// Device serial numbers or IDs. If omitted, shows all devices.
    pub ids: Vec<String>,
    /// Filter by labels (format: key=value). Can be used multiple times.
    #[arg(short, long = "label", value_name = "KEY=VALUE")]
    pub labels: Vec<String>,
    /// Show only online devices (last seen < 5 minutes)
    #[arg(long, conflicts_with = "offline")]
    pub online: bool,
    /// Show only offline devices (last seen >= 5 minutes)
    #[arg(long, conflicts_with = "online")]
    pub offline: bool,
    /// Use partial matching for device IDs (matches serial number, hostname, or model)
    #[arg(short, long)]
    pub search: bool,
}

#[derive(Subcommand)]
pub enum StatusResourceType {
    /// Get smithd status for a device (runs 'smithd status' command)
    ///
    /// Shows comprehensive update status including:
    /// - Update/upgrade status (whether the system is up-to-date)
    /// - Installed package versions (currently running on the device)
    /// - Target package versions (versions that should be running)
    /// - Update status flag (true/false for each package indicating if it's updated)
    #[command(verbatim_doc_comment)]
    #[command(after_long_help = "\
Examples:
  sm status d ABC123
  sm status d --online
  # Queue it on every production device and collect results later
  sm status d -l env=production --nowait
")]
    #[command(visible_alias = "devices")]
    #[command(visible_alias = "d")]
    Device {
        #[command(flatten)]
        selector: DeviceSelector,
        /// Don't wait for result, just queue the command and return immediately (faster, recommended for agents - use 'sm command <id>' to check results later)
        #[arg(long, default_value = "false")]
        nowait: bool,
    },
    /// Get systemd service status on a device (runs 'systemctl status <unit>')
    #[command(after_long_help = "\
Examples:
  sm status svc nginx ABC123
  sm status svc smithd -l env=production
  sm status svc docker ABC123 XYZ789
")]
    #[command(visible_alias = "services")]
    #[command(visible_alias = "svc")]
    Service {
        /// Service unit name (e.g., nginx, smithd, docker)
        unit: String,
        #[command(flatten)]
        selector: DeviceSelector,
        /// Don't wait for result, just queue the command and return immediately (faster, recommended for agents - use 'sm command <id>' to check results later)
        #[arg(long, default_value = "false")]
        nowait: bool,
    },
}

#[derive(Subcommand)]
pub enum AuthCommands {
    /// Login to Smith API. If a browser cannot be opened, use the displayed URL manually.
    #[command(after_long_help = "\
Examples:
  sm auth login
  # On a machine without a browser
  sm auth login --no-open
")]
    Login {
        /// Do not try to open a browser automatically
        #[arg(long, default_value = "false")]
        no_open: bool,
    },
    /// logs out the current section
    Logout,
    /// Shows the current token being used
    Show,
}

#[derive(Subcommand, Debug)]
pub enum DistroCommands {
    /// List the current distributions
    Ls {
        #[arg(short, long, default_value = "false")]
        json: bool,
    },
    /// List the current distribution releases
    Releases,
}

#[derive(Subcommand)]
pub enum GetResourceType {
    /// Get device information
    ///
    /// Without IDs or filters, lists every device in the fleet.
    #[command(after_long_help = "\
Examples:
  sm get d
  sm get d ABC123 XYZ789
  sm get d --online
  # Partial match on serial number, hostname or model
  sm get d rpi -s
  sm get d -l env=production -l region=us-west
  # Only the serial numbers, one per line
  sm get d -o serial_number
")]
    #[command(visible_alias = "devices")]
    #[command(visible_alias = "d")]
    Device {
        #[command(flatten)]
        selector: DeviceSelector,
        #[arg(short, long, default_value = "false")]
        json: bool,
        /// Output format: wide, json, or custom field (e.g., serial_number, id, ip_address)
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Get recent commands for device(s)
    ///
    /// Shows each command's ID, when it was issued, its type (Restart, FreeForm,
    /// UpdateVariables, ...) and its status (Pending, Fetched, Completed,
    /// Cancelled). FreeForm commands show the command text.
    #[command(after_long_help = "\
Examples:
  sm get cmds ABC123
  sm get cmds ABC123 --limit 50
  sm get cmds -l env=production
")]
    #[command(visible_alias = "cmds")]
    Commands {
        #[command(flatten)]
        selector: DeviceSelector,
        /// Number of commands to show per device
        #[arg(long, default_value = "10")]
        limit: u32,
        #[arg(short, long, default_value = "false")]
        json: bool,
    },
    /// Get saved command recipes
    #[command(visible_alias = "recipes")]
    Recipe {
        /// Recipe name or id to show. Omit to list all recipes.
        name: Option<String>,
        #[arg(short, long, default_value = "false")]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum TestNetworkCommands {
    /// Run a quick network test on device(s) (downloads 20MB test file)
    #[command(after_long_help = "\
Examples:
  sm test-network quick ABC123
  sm test-network quick --online
")]
    Quick {
        #[command(flatten)]
        selector: DeviceSelector,
    },
    /// Run an extended network test on devices matching labels
    #[command(after_long_help = "\
Examples:
  sm test-network extended -l env=staging
  # Run for 5 minutes and wait for the results
  sm test-network extended -l env=staging -d 5 -w
")]
    Extended {
        /// Filter by labels (format: key=value). Can be used multiple times.
        #[arg(short, long = "label", value_name = "KEY=VALUE", required = true)]
        labels: Vec<String>,
        /// Duration in minutes (3-8)
        #[arg(short, long, default_value = "3", value_parser = clap::value_parser!(u32).range(3..=8))]
        duration: u32,
        /// Poll for results (wait until completion)
        #[arg(short, long)]
        wait: bool,
        /// Poll interval in seconds (default: 30)
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..))]
        poll_interval: u64,
    },
    /// Check status of an extended network test session
    Status {
        /// Session ID (UUID) returned from extended test
        session_id: String,
    },
}

#[derive(Subcommand)]
pub enum RestartResourceType {
    /// Restart devices
    ///
    /// Asks for confirmation first, listing up to 10 of the devices that will
    /// restart. Pass `--yes` to skip it.
    #[command(after_long_help = "\
Examples:
  sm restart d ABC123
  sm restart d -l env=staging -y
  sm restart d --offline --nowait
")]
    #[command(visible_alias = "devices")]
    #[command(visible_alias = "d")]
    Device {
        #[command(flatten)]
        selector: DeviceSelector,
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
        /// Don't wait for result, just queue the command and return immediately
        #[arg(long, default_value = "false")]
        nowait: bool,
    },
    /// Restart a systemd service on device(s) (runs 'systemctl restart <unit>')
    #[command(after_long_help = "\
Examples:
  sm restart svc nginx ABC123
  sm restart svc smithd -l env=production -y
")]
    #[command(visible_alias = "services")]
    #[command(visible_alias = "svc")]
    Service {
        /// Service unit name (e.g., nginx, smithd, docker)
        unit: String,
        #[command(flatten)]
        selector: DeviceSelector,
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
        /// Don't wait for result, just queue the command and return immediately
        #[arg(long, default_value = "false")]
        nowait: bool,
    },
}

#[derive(Subcommand)]
pub enum Commands {
    /// Commands to handle current profile to use
    #[command(after_long_help = "\
Examples:
  # Show the current profile
  sm profile
  # Switch to another profile
  sm profile production
")]
    Profile { profile: Option<String> },

    /// Sets up the authentication to connect to Smith API
    Auth {
        /// lists test values
        #[clap(subcommand)]
        command: AuthCommands,
    },

    /// Get detailed information about a resource
    Get {
        #[clap(subcommand)]
        resource: GetResourceType,
    },

    /// Restart devices
    Restart {
        #[clap(subcommand)]
        resource: RestartResourceType,
    },

    /// Get status information for a resource (device or service)
    Status {
        #[clap(subcommand)]
        resource: StatusResourceType,
    },

    /// Get logs for a device (runs 'journalctl -r' on the device)
    ///
    /// Entries come from the systemd journal, newest first.
    #[command(after_long_help = "\
Examples:
  sm logs ABC123
  sm logs ABC123 --unit smithd --since \"1h ago\"
  sm logs ABC123 --grep \"error|panic\"
  sm logs -l env=production --nowait
")]
    Logs {
        #[command(flatten)]
        selector: DeviceSelector,
        /// Filter logs by systemd unit (e.g. smithd or smithd.service). Passed to journalctl -u
        #[arg(long)]
        unit: Option<String>,
        /// Show entries not older than this date. Passed to journalctl --since (e.g. "1h ago", "2026-06-17 10:00:00")
        #[arg(long, value_name = "TIMESTAMP", allow_hyphen_values = true)]
        since: Option<String>,
        /// Show entries not newer than this date. Passed to journalctl --until
        #[arg(long, value_name = "TIMESTAMP", allow_hyphen_values = true)]
        until: Option<String>,
        /// Filter log lines by pattern. Passed to journalctl --grep (supports ERE regex)
        #[arg(long, value_name = "PATTERN", allow_hyphen_values = true)]
        grep: Option<String>,
        /// Don't wait for result, just queue the command and return immediately (faster, recommended for agents - use 'sm command <id>' to check results later)
        #[arg(long, default_value = "false")]
        nowait: bool,
    },

    /// Test network speed for device(s)
    TestNetwork {
        #[clap(subcommand)]
        command: TestNetworkCommands,
    },

    /// Check command results by ID (format: device_id:command_id)
    ///
    /// IDs are printed by any command run with `--nowait`, and listed by
    /// `sm get cmds`.
    #[command(after_long_help = "\
Examples:
  sm command 123:456
  sm command 123:456 789:012
")]
    Command {
        /// Command IDs to check in format device_id:command_id
        ids: Vec<String>,
    },

    /// Lists distributions and information
    #[command(visible_alias = "distros")]
    Distributions {
        #[clap(subcommand)]
        command: DistroCommands,
    },

    /// Commands related to releases
    Releases {
        #[clap(subcommand)]
        command: ReleasesCommands,
    },

    /// Tunneling options into a device
    #[command(after_long_help = "\
Examples:
  sm tunnel ABC123
")]
    Tunnel {
        /// Device serial number to tunnel into
        serial_number: String,

        /// Change the default user for the tunnel
        #[arg(long)]
        override_user: Option<String>,
    },

    /// Generate shell completion scripts
    #[command(after_long_help = "\
Examples:
  sm completion zsh > ~/.zsh/completion/_sm
  sm completion bash > /usr/local/etc/bash_completion.d/sm
  sm completion fish > ~/.config/fish/completions/sm.fish
")]
    Completion {
        // Shell type to generate completion script for
        #[arg(value_parser = value_parser!(Shell))]
        shell: Shell,
    },

    /// Update the CLI
    #[command(after_long_help = "\
Examples:
  sm update --check
  sm update
")]
    Update {
        /// Check for updates without installing
        #[arg(long)]
        check: bool,
    },

    /// Print all available commands in markdown format (useful for agents)
    #[command(name = "agent-help")]
    AgentHelp,

    /// Run commands on devices with filters (async by default, use --wait to poll for results)
    ///
    /// The command goes after `--`, or on stdin. Wrap anything with pipes or
    /// other shell syntax in quotes. Without `--wait`, check results later with
    /// `sm command <device_id>:<command_id>`.
    #[command(after_long_help = "\
Examples:
  sm run ABC123 -- uptime
  sm run ABC123 -w -- df -h
  sm run -l env=production -- systemctl status smithd
  sm run ABC123 -w -- \"dmesg | grep -i error | tail -n 20\"
  # A saved recipe instead of a free-form command
  sm run ABC123 -r disk-usage
")]
    Run {
        #[command(flatten)]
        selector: DeviceSelector,
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
        /// Wait for command results (polls until completion)
        #[arg(short, long, default_value = "false")]
        wait: bool,
        /// Run a saved recipe by name or id instead of a free-form command
        #[arg(short, long, conflicts_with = "command")]
        recipe: Option<String>,
        /// Command to execute on the devices (provide after -- or via stdin)
        #[arg(last = true)]
        command: Vec<String>,
    },

    /// Set labels on devices with filters
    ///
    /// Setting a label overwrites any existing value for that key. Filter on
    /// labels in other commands with `-l`, e.g. `sm get d -l env=production`.
    #[command(after_long_help = "\
Examples:
  sm label -d ABC123 env=production
  sm label -d ABC123 env=production region=us-west
  sm label -l region=us-east env=production
  sm label -d rpi -s location=warehouse-1
")]
    Label {
        // Not `DeviceSelector`: its variadic positional IDs would swallow the
        // labels below, so devices are named with `--device` instead.
        /// Filter by labels (format: key=value). Can be used multiple times.
        #[arg(short, long = "label", value_name = "KEY=VALUE")]
        labels: Vec<String>,
        /// Only online devices (last seen < 5 minutes)
        #[arg(long, conflicts_with = "offline")]
        online: bool,
        /// Only offline devices (last seen >= 5 minutes)
        #[arg(long, conflicts_with = "online")]
        offline: bool,
        /// Use partial matching for `--device` (matches serial number, hostname, or model)
        #[arg(short, long)]
        search: bool,
        /// Specific device serial numbers or IDs to target
        #[arg(short, long = "device")]
        devices: Vec<String>,
        /// Labels to set on the devices (format: key=value). Can be used multiple times.
        #[arg(required = true, value_name = "KEY=VALUE")]
        set_labels: Vec<String>,
    },

    /// Approve devices for fleet management
    Approve {
        #[command(flatten)]
        selector: DeviceSelector,
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Revoke device approval
    Revoke {
        #[command(flatten)]
        selector: DeviceSelector,
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
}
