use anyhow::Context;
use clap::Parser;
use smith::magic::{MagicHandle, structure::ConfigPackage};
use smith::shutdown::ShutdownHandler;
use smith::updater::PENDING_SMITH_RELEASE_FILE;
use std::path::{Path, PathBuf};
use tokio::time;
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, hide = true)]
    check_pinned_release_support: bool,
}

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
            && let Ok(mtime) = meta.modified()
        {
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
    let args = Args::parse();
    if args.check_pinned_release_support {
        return Ok(());
    }
    tracing_subscriber::fmt::init();
    info!("Smith Updater Starting");

    tokio::time::sleep(time::Duration::from_secs(30)).await;

    info!("Smith Updater Updating");
    let shutdown = ShutdownHandler::new();

    let configuration = MagicHandle::new(shutdown.signals());

    configuration.load(None).await;

    time::sleep(time::Duration::from_secs(5)).await;

    let packages_dir = PathBuf::from("/etc/smith/packages");
    let pending_release = packages_dir.join(PENDING_SMITH_RELEASE_FILE);
    let target_release_id = match tokio::fs::read_to_string(&pending_release).await {
        Ok(release_id) => {
            let release_id = release_id
                .trim()
                .parse::<i32>()
                .with_context(|| format!("Invalid release id in {}", pending_release.display()))?;
            info!(
                release_id,
                pending_release = %pending_release.display(),
                "Using Smith target pinned by the updater transaction"
            );
            release_id
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            warn!("No pinned Smith target found; falling back to the current configured target");
            configuration
                .get_target_release_id()
                .await
                .with_context(|| "Failed to get Target Release ID")?
        }
        Err(err) => {
            let err = anyhow::Error::new(err)
                .context(format!("Failed to read {}", pending_release.display()));
            return Err(err.into());
        }
    };

    let blobs = packages_dir.join("blobs");
    let release_cache = packages_dir
        .join("versions")
        .join(target_release_id.to_string());

    let (package_file, smith_package) = if release_cache.exists() {
        info!("Using versions file: {}", release_cache.display());
        let content = tokio::fs::read(&release_cache).await?;
        let content = std::str::from_utf8(&content)?;

        let packages = ConfigPackage::parse_manifest(content)?;

        let smith_package = packages
            .into_iter()
            .find(|package| package.name == "smith" || package.name == "smith_amd64")
            .with_context(|| "No smith package found in release")?;

        let package_file = smith_package.safe_file_path(&blobs)?;
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
        let status = tokio::process::Command::new("sudo")
            .arg("apt")
            .arg("install")
            .arg(&package_file)
            .arg("-y")
            .arg("--allow-downgrades")
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
            return Err(anyhow::anyhow!("Failed to install Smith package").into());
        }
    } else {
        info!("Package already installed");
    }

    match tokio::fs::remove_file(&pending_release).await {
        Ok(()) => info!(
            pending_release = %pending_release.display(),
            "Cleared completed Smith self-update target"
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => warn!(
            error = ?err,
            pending_release = %pending_release.display(),
            "Failed to clear completed Smith self-update target"
        ),
    }

    info!("Smith Updater Shutting Down");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_manifest_rejects_paths_outside_blob_cache() {
        assert!(ConfigPackage::parse_manifest("smith 1 ../../tmp/smith.deb\n").is_err());
        assert!(ConfigPackage::parse_manifest("smith 1 /tmp/smith.deb\n").is_err());
        assert!(ConfigPackage::parse_manifest("smith 1 smith.deb;reboot\n").is_ok());
    }

    #[test]
    fn safe_package_path_keeps_nested_files_under_base() -> anyhow::Result<()> {
        let base = Path::new("/etc/smith/packages/blobs");
        let package = ConfigPackage {
            name: "smith".to_string(),
            version: "1".to_string(),
            file: "nested/smith.deb".to_string(),
        };
        let path = package.safe_file_path(base)?;

        assert_eq!(path, base.join("nested/smith.deb"));
        Ok(())
    }
}
