use clap::{Parser, Subcommand, value_parser};
use clap_complete::Shell;

use crate::commands::{devices::DevicesCommands, releases::ReleasesCommands};

#[derive(Parser)]
#[command(name = "sm", version, about = "Smith CLI - Fleet management tool", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum AuthCommands {
    /// login to Smith API
    Login {
        /// does not open the browser by default
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

#[derive(Subcommand, Debug)]
pub enum ServiceCommands {
    /// Get status of a systemd service
    Status {
        /// Service unit name
        #[arg(short, long)]
        unit: String,
        /// Device serial number
        serial_number: String,
        /// Don't wait for result, just queue the command and return immediately (faster, recommended for agents - use 'sm command <id>' to check results later)
        #[arg(long, default_value = "false")]
        nowait: bool,
    },
}

#[derive(Subcommand)]
pub enum GetResourceType {
    /// Get device information
    #[command(visible_alias = "devices")]
    #[command(visible_alias = "d")]
    Device {
        /// Device serial numbers or IDs. If omitted, shows all devices.
        ids: Vec<String>,
        #[arg(short, long, default_value = "false")]
        json: bool,
        /// Filter by labels (format: key=value). Can be used multiple times.
        #[arg(short, long = "label", value_name = "KEY=VALUE")]
        labels: Vec<String>,
        /// Show only online devices (last seen < 5 minutes)
        #[arg(long, conflicts_with = "offline")]
        online: bool,
        /// Show only offline devices (last seen >= 5 minutes)
        #[arg(long, conflicts_with = "online")]
        offline: bool,
        /// Output format: wide, json, or custom field (e.g., serial_number, id, ip_address)
        #[arg(short, long)]
        output: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum RestartResourceType {
    /// Restart devices
    #[command(visible_alias = "devices")]
    #[command(visible_alias = "d")]
    Device {
        /// Device serial numbers or IDs to restart
        ids: Vec<String>,
        /// Filter by labels (format: key=value). Can be used multiple times.
        #[arg(short, long = "label", value_name = "KEY=VALUE")]
        labels: Vec<String>,
        /// Show only online devices (last seen < 5 minutes)
        #[arg(long, conflicts_with = "offline")]
        online: bool,
        /// Show only offline devices (last seen >= 5 minutes)
        #[arg(long, conflicts_with = "online")]
        offline: bool,
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

    /// Lists devices and information
    Devices {
        #[clap(subcommand)]
        command: DevicesCommands,
    },

    /// Service management commands
    Service {
        #[clap(subcommand)]
        command: ServiceCommands,
    },

    /// Get smithd status for a device (runs 'smithd status' command)
    ///
    /// Shows comprehensive update status including:
    /// - Update/upgrade status (whether the system is up-to-date)
    /// - Installed package versions (currently running on the device)
    /// - Target package versions (versions that should be running)
    /// - Update status flag (true/false for each package indicating if it's updated)
    Status {
        /// Device serial number
        serial_number: String,
        /// Don't wait for result, just queue the command and return immediately (faster, recommended for agents - use 'sm command <id>' to check results later)
        #[arg(long, default_value = "false")]
        nowait: bool,
    },

    /// Check command results by ID (format: device_id:command_id)
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
    Tunnel {
        /// Device serial number to tunnel into
        serial_number: String,

        /// Setup for overview debug
        #[arg(long)]
        overview_debug: bool,
    },

    /// Generate shell completion scripts
    Completion {
        // Shell type to generate completion script for
        #[arg(value_parser = value_parser!(Shell))]
        shell: Shell,
    },

    /// Update the CLI
    Update {
        /// Check for updates without installing
        #[arg(long)]
        check: bool,
    },

    /// Print all available commands in markdown format (useful for agents)
    #[command(name = "agent-help")]
    AgentHelp,

    /// Run commands on devices with filters (async by default, use --wait to poll for results)
    Run {
        /// Filter by labels (format: key=value). Can be used multiple times.
        #[arg(short, long = "label", value_name = "KEY=VALUE")]
        labels: Vec<String>,
        /// Show only online devices (last seen < 5 minutes)
        #[arg(long, conflicts_with = "offline")]
        online: bool,
        /// Show only offline devices (last seen >= 5 minutes)
        #[arg(long, conflicts_with = "online")]
        offline: bool,
        /// Specific device serial numbers or IDs to target
        #[arg(short, long = "device")]
        devices: Vec<String>,
        /// Wait for command results (polls until completion)
        #[arg(short, long, default_value = "false")]
        wait: bool,
        /// Command to execute on the devices
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,
    },

    /// Set labels on devices with filters
    Label {
        /// Filter by labels (format: key=value). Can be used multiple times.
        #[arg(short, long = "label", value_name = "KEY=VALUE")]
        labels: Vec<String>,
        /// Show only online devices (last seen < 5 minutes)
        #[arg(long, conflicts_with = "offline")]
        online: bool,
        /// Show only offline devices (last seen >= 5 minutes)
        #[arg(long, conflicts_with = "online")]
        offline: bool,
        /// Specific device serial numbers or IDs to target
        #[arg(short, long = "device")]
        devices: Vec<String>,
        /// Labels to set on the devices (format: key=value). Can be used multiple times.
        #[arg(required = true, value_name = "KEY=VALUE")]
        set_labels: Vec<String>,
    },
}
