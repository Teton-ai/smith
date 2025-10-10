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
    },
    /// Test network speed for a device
    TestNetwork {
        /// Device serial number or ID
        device: String,
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
}
