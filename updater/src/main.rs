use anyhow::Context;
use clap::Parser;
use smith::magic::{MagicHandle, structure::ConfigPackage};
use smith::shutdown::ShutdownHandler;
use std::path::{Path, PathBuf};
use tokio::time;
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args;

async fn find_latest_smith_deb(packages_dir: &Path) -> anyhow::Result<(PathBuf, ConfigPackage)> {
    let mut entries = tokio::fs::read_dir(packages_dir)
        .await
        .with_context(|| format!("Failed to read {}", packages_dir.display()))?;

    let mut candidates: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("deb") {
            continue;
        }
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if !filename.starts_with("smith_") {
            continue;
        }
        if let Ok(meta) = entry.metadata().await
            && let Ok(mtime) = meta.modified() {
                candidates.push((path, mtime));
            }
    }

    candidates.sort_by_key(|(_, mtime)| *mtime);
    let (path, _) = candidates
        .into_iter()
        .last()
        .with_context(|| format!("No smith .deb found in {}", packages_dir.display()))?;

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .with_context(|| "Invalid .deb filename")?;

    // Debian filenames follow <name>_<version>_<arch>.deb
    let version = filename
        .split('_')
        .nth(1)
        .with_context(|| format!("Could not parse version from filename: {}", filename))?
        .to_string();

    let package = ConfigPackage {
        name: "smith".to_string(),
        version,
        file: filename.to_string(),
    };

    Ok((path, package))
}

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

    let (package_file, smith_package) = if release_cache.exists() {
        info!("Using versions file: {}", release_cache.display());
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
            .into_iter()
            .find(|package| package.name == "smith" || package.name == "smith_amd64")
            .with_context(|| "No smith package found in release")?;

        let package_file = blobs.join(&smith_package.file);
        info!(
            "Found smith package: version={} file={}",
            smith_package.version,
            package_file.display()
        );
        (package_file, smith_package)
    } else {
        warn!(
            "Versions file not found at {} — last resort: scanning {} for smith .deb",
            release_cache.display(),
            packages_dir.display()
        );
        let (file, package) = find_latest_smith_deb(&packages_dir).await?;
        warn!(
            "Last resort selected: version={} file={}",
            package.version,
            file.display()
        );
        (file, package)
    };
    let package_version = &smith_package.version;

    let installed_version = smith_package.get_system_version().await;
    let package_installed = matches!(installed_version, Ok(ref v) if v == package_version);
    info!(
        "Installed version: {:?}, target: {}, up to date: {}",
        installed_version, package_version, package_installed
    );

    if !package_installed {
        info!("Installing smith {}", package_version);
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
