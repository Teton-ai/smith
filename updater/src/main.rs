use clap::Parser;
use smith::magic::MagicHandle;
use smith::shutdown::ShutdownHandler;
use std::io::Write;
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

    let packages = configuration.get_packages().await;

    let smith_package = packages
        .iter()
        .filter(|package| package.name == "smith" || package.name == "smith_amd64")
        .collect::<Vec<_>>();

    assert_eq!(smith_package.len(), 1);

    // check current version of smith
    let output = tokio::process::Command::new("dpkg")
        .arg("-l")
        .arg("smith")
        .output()
        .await
        .expect("Failed to execute dpkg command");

    let package_file = smith_package[0].file.clone();
    let package_version = smith_package[0].version.clone();
    let mut package_installed = false;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();

        // Assuming the package version is in the second column of the first line
        if let Some(package_info) = lines.get(5) {
            let fields: Vec<&str> = package_info.split_whitespace().collect();
            if let Some(version) = fields.get(2) {
                info!(
                    "Package installed -> {version} | {} <- Magic Version",
                    &package_version
                );
                package_installed = version == &package_version
            }
        }
    };

    if !package_installed {
        info!("Package must already be available");
        let package_location = format!("packages/{}", package_file);
        info!("Installing package: smith");
        let install_command = format!(
            "sudo apt install ./{} -y --allow-downgrades",
            package_location
        );
        let status = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(install_command)
            .output()
            .await
            .map_err(|e| {
                error!("Failed to run command to install smith: {}", e);
                e
            })?;

        if status.status.success() {
            info!("Smith installed! Restarting");
        } else {
            error!("Failed to install smith");
            info!("Output of install command:");
            let _ = std::io::stderr()
                .write_all(&output.stderr)
                .inspect_err(|e| error!("Failed to write output of installation to stderr. {e}"));
        }
    } else {
        info!("Package already installed");
    }

    info!("Smith Updater Shutting Down");

    Ok(())
}
