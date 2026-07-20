use crate::dbus::SmithDbusProxy;
use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;
use zbus::Connection;

mod status;
mod upload;
use status::status;

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
    Mode {
        #[arg(help = "Set the mode of the agent", long)]
        mode: String,
    },
    /// Upload a local file or folder to smith assets S3 bucket
    Upload(Upload),
    Tunnel {
        #[arg(help = "Expose a port to the internet", long)]
        port: u16,
    },
}

#[derive(Parser, Debug)]
struct Upload {
    #[arg(help = "Specify the file / folder to upload")]
    file: String,
}

pub async fn execute() -> bool {
    let mut daemon_should_run = false;
    let args = Args::parse();

    match args.command {
        Some(Commands::Update) => {
            _ = update().await;
        }
        Some(Commands::Upload(upload_args)) => {
            upload::files_upload(&upload_args.file).await.unwrap();
        }
        Some(Commands::Upgrade) => {
            _ = upgrade().await;
        }
        Some(Commands::Status) => {
            _ = status().await;
        }
        Some(Commands::Mode { mode }) => {
            _ = change_to_mode(&mode).await;
        }
        Some(Commands::Tunnel { port }) => {
            _ = expose_port(port).await;
        }
        None => daemon_should_run = true,
    }

    daemon_should_run
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

pub async fn update() -> Result<()> {
    let connection = Connection::system().await?;

    let proxy = SmithDbusProxy::new(&connection).await?;

    let reply = proxy.update_packages().await?;

    info!(reply);
    Ok(())
}

pub async fn upgrade() -> Result<()> {
    let connection = Connection::system().await?;

    let proxy = SmithDbusProxy::new(&connection).await?;

    let reply = proxy.upgrade_packages().await?;

    info!(reply);
    Ok(())
}

pub async fn change_to_mode(mode: &str) -> Result<()> {
    let connection = Connection::system().await?;

    let proxy = SmithDbusProxy::new(&connection).await?;

    let reply = if mode == "app" {
        proxy.schedule_services().await?
    } else {
        proxy.unschedule_services().await?
    };

    info!(reply);
    Ok(())
}

pub async fn expose_port(port: u16) -> Result<()> {
    let connection = Connection::system().await?;

    let proxy = SmithDbusProxy::new(&connection).await?;

    let reply = proxy.expose_port(port).await?;

    println!("{}", reply);
    Ok(())
}
