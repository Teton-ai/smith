use anyhow::Context;
use clap::Parser;
use smith::magic::{MagicHandle, structure::ConfigPackage};
use smith::shutdown::ShutdownHandler;
use std::path::PathBuf;
use tokio::time;
use tracing::{error, info};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Args::parse();
    tracing_subscriber::fmt::init();
    info!("Smith Updater Starting");

    tokio::time::sleep(time::Duration::from_secs(30)).await;

    info!("Smith Updater Updating");
    let shutdown = ShutdownHandler::new();

    let configuration = MagicHandle::new(shutdown.signals());

    configuration.load(None).await;

    time::sleep(time::Duration::from_secs(5)).await;

    let target_release_id = configuration
        .get_target_release_id()
        .await
        .with_context(|| "Failed to get Target Release ID")?;

    let packages_dir = PathBuf::from("/etc/smith/packages");
    let blobs = packages_dir.join("blobs");
    let release_cache = packages_dir
        .join("versions")
        .join(target_release_id.to_string());

    // read the file from release cache
    let content = tokio::fs::read(&release_cache).await?;
    let content = std::str::from_utf8(&content)?;

    let packages: Vec<ConfigPackage> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.splitn(3, ' ');
            Ok::<_, anyhow::Error>(ConfigPackage {
                name: parts
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("missing name"))?
                    .to_string(),
                version: parts
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("missing version"))?
                    .to_string(),
                file: parts
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("missing file"))?
                    .to_string(),
            })
        })
        .collect::<Result<_, _>>()?;

    let smith_package = packages
        .iter()
        .find(|package| package.name == "smith" || package.name == "smith_amd64")
        .with_context(|| "No smith package found in release")?;

    let package_file = blobs.join(&smith_package.file);
    let package_version = &smith_package.version;

    let installed_version = smith_package.get_system_version().await;
    let package_installed = matches!(installed_version, Ok(ref v) if v == package_version);

    if !package_installed {
        info!("Installing package: smith");
        let install_command = format!(
            "sudo apt install {} -y --allow-downgrades",
            package_file.display()
        );
        let status = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&install_command)
            .output()
            .await
            .map_err(|e| {
                error!("Failed to run install command for smith: {}", e);
                e
            })?;

        if status.status.success() {
            info!("Smith installed! Restarting");
        } else {
            let stderr = String::from_utf8_lossy(&status.stderr);
            let stdout = String::from_utf8_lossy(&status.stdout);
            error!(
                "Failed to install smith:\nstderr: {}\nstdout: {}",
                stderr, stdout
            );
        }
    } else {
        info!("Package already installed");
    }

    info!("Smith Updater Shutting Down");

    Ok(())
}
