use clap::{Parser, Subcommand, value_parser};
use clap_complete::Shell;

#[derive(Parser)]
#[command(version, about, long_about = None)]
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
pub enum DevicesCommands {
    /// List the current distributions
    Ls {
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
    },
    /// Test network speed for a device
    TestNetwork {
        /// Device serial number or ID
        device: String,
    },
    /// Get logs for a specific device
    Logs {
        /// Device serial number
        serial_number: String,
        /// Don't wait for result, just queue the command and return immediately (faster, recommended for agents - use 'sm command <id>' to check results later)
        #[arg(long, default_value = "false")]
        nowait: bool,
    },
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
pub enum Commands {
    /// Commands to handle current profile to use
    Profile { profile: Option<String> },

    /// Sets up the authentication to connect to Smith API
    Auth {
        /// lists test values
        #[clap(subcommand)]
        command: AuthCommands,
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
    #[command(alias = "distro")]
    Distributions {
        #[clap(subcommand)]
        command: DistroCommands,
    },

    // Interact with a specific Release
    Release {
        release_number: String,

        #[arg(short, long, default_value = "false")]
        deploy: bool,
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
}
