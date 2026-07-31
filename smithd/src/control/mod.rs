use crate::police::RebootStatus;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

mod server;
mod status;
mod upload;
pub use server::ControlHandle;
use status::status;

/// Unix socket the daemon serves its local control API on. Root-only (0660).
pub const CONTROL_SOCKET: &str = "/run/smithd/smithd.sock";

// Request/response bodies are shared by the server and the CLI client below so
// the two cannot drift apart.

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckResponse {
    pub updates_available: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TunnelRequest {
    pub port: Option<u16>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TunnelResponse {
    pub public_port: u16,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DownloadRequest {
    pub remote_file: String,
    pub local_file: String,
    pub rate_mb: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HoldRequest {
    /// Lease length in seconds. Omitted → the daemon's default TTL. Values are
    /// capped server-side; renew before expiry to keep a reboot deferred.
    pub ttl_seconds: Option<u64>,
}

/// The one and only agent smith
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Update the local debian files to match the remote release ones
    Update,
    /// Upgrade the local debian files to run the latest version installed
    Upgrade,
    Status,
    /// Report whether the daemon has a reboot scheduled, or place/release a
    /// hold that defers one
    Watchdog {
        #[command(subcommand)]
        action: Option<WatchdogAction>,
    },
    /// Upload a local file or folder to smith assets S3 bucket
    Upload(Upload),
    Tunnel {
        #[arg(help = "Expose a port to the internet", long)]
        port: u16,
    },
}

#[derive(Subcommand, Debug)]
pub enum WatchdogAction {
    /// Defer any scheduled reboot while the hold lease is alive; renew before
    /// the TTL expires to keep deferring
    Hold {
        #[arg(long, help = "Lease length in seconds (daemon default when omitted)")]
        ttl: Option<u64>,
    },
    /// Release the hold; a deferred reboot may then fire
    Release,
}

#[derive(Parser, Debug)]
struct Upload {
    #[arg(help = "Specify the file / folder to upload")]
    file: String,
}

/// What the caller should do once the CLI arguments have been handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// No subcommand was given: hand over to the daemon.
    RunDaemon,
    /// A control command ran successfully.
    Success,
    /// A control command failed; the error has already been logged.
    Failure,
}

pub async fn execute() -> Outcome {
    let args = Args::parse();

    let result = match args.command {
        Some(Commands::Update) => update()
            .await
            .inspect_err(|e| error!("Failed to schedule update: {e:#}")),
        Some(Commands::Upload(upload_args)) => upload::files_upload(&upload_args.file)
            .await
            .inspect_err(|e| error!("Failed to upload {}: {e:#}", upload_args.file)),
        Some(Commands::Upgrade) => upgrade()
            .await
            .inspect_err(|e| error!("Failed to schedule upgrade: {e:#}")),
        Some(Commands::Status) => status()
            .await
            .inspect_err(|e| error!("Failed to get status: {e:#}")),
        Some(Commands::Watchdog { action }) => watchdog(action)
            .await
            .inspect_err(|e| error!("Failed to query watchdog: {e:#}")),
        Some(Commands::Tunnel { port }) => expose_port(port)
            .await
            .inspect_err(|e| error!("Failed to expose port {port}: {e:#}")),
        None => return Outcome::RunDaemon,
    };

    match result {
        Ok(()) => Outcome::Success,
        Err(_) => Outcome::Failure,
    }
}

pub async fn ensure_daemon_mode() -> bool {
    let args = Args::parse();

    match args.command {
        Some(_) => {
            info!("Invalid command. maybe you should try to use smithctl");
            false
        }
        None => {
            info!("No command");
            true
        }
    }
}

/// HTTP client bound to the daemon's control socket. The host in request URLs is
/// ignored — the connection always goes to [`CONTROL_SOCKET`].
fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .unix_socket(CONTROL_SOCKET)
        .build()
        .context("Failed to build control API client")
}

fn control_url(path: &str) -> String {
    format!("http://localhost{path}")
}

pub async fn update() -> Result<()> {
    let response: CheckResponse = client()?
        .post(control_url("/updater/check"))
        .send()
        .await
        .context("Is the smithd daemon running?")?
        .error_for_status()?
        .json()
        .await?;

    info!("Updates available: {}", response.updates_available);
    Ok(())
}

pub async fn upgrade() -> Result<()> {
    let response: MessageResponse = client()?
        .post(control_url("/updater/upgrade"))
        .send()
        .await
        .context("Is the smithd daemon running?")?
        .error_for_status()?
        .json()
        .await?;

    info!(response.message);
    Ok(())
}

pub async fn watchdog(action: Option<WatchdogAction>) -> Result<()> {
    let client = client()?;

    let request = match action {
        None => client.get(control_url("/watchdog")),
        Some(WatchdogAction::Hold { ttl }) => client
            .post(control_url("/watchdog/hold"))
            .json(&HoldRequest { ttl_seconds: ttl }),
        Some(WatchdogAction::Release) => client.delete(control_url("/watchdog/hold")),
    };

    let status: RebootStatus = request
        .send()
        .await
        .context("Is the smithd daemon running?")?
        .error_for_status()?
        .json()
        .await?;

    if status.reboot_pending {
        println!(
            "reboot pending in {}s ({}s elapsed of {}s delay)",
            status.seconds_remaining, status.elapsed_seconds, status.delay_seconds
        );
    } else {
        println!("no reboot scheduled");
    }

    if status.held {
        println!("held for another {}s", status.hold_seconds_remaining);
    }

    Ok(())
}

pub async fn expose_port(port: u16) -> Result<()> {
    let response: TunnelResponse = client()?
        .post(control_url("/tunnel"))
        .json(&TunnelRequest { port: Some(port) })
        .send()
        .await
        .context("Is the smithd daemon running?")?
        .error_for_status()?
        .json()
        .await?;

    println!("{}", response.public_port);
    Ok(())
}
