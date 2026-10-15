use crate::{api::SmithAPI, auth, commands::os::OsCommands, print::TablePrint};
use anyhow::{Context as _, bail};
use chrono_humanize::HumanTime;
use clap::{Args, Subcommand};
use models::{
    deployment::DeploymentRequest,
    distribution::NewDistributionRelease,
    release::{UpdateRelease, VersionBump, next_version},
};

#[derive(Args, Debug)]
pub struct ReleasesGet {
    /// Get a specific release, leave out to get all releases
    release_number: Option<String>,
    #[arg(long, visible_alias = "distro")]
    distribution_id: Option<i32>,
    #[arg(long, default_value = "false")]
    json: bool,
}

#[derive(Subcommand, Debug)]
pub enum ReleasesCommands {
    /// Get releases
    Get(ReleasesGet),
    /// Draft a new release from the deployed release of a distribution
    ///
    /// The draft copies the packages of the deployed release (or of `--from`)
    /// and swaps in each `--package`. Choices left out are asked
    /// interactively.
    #[command(after_long_help = "\
Examples:
  sm releases draft
  sm releases draft --distro 6 --version 1.4.0 --package 812 --package 815
  sm releases draft --distro 6 --from 140 --version 1.4.0-rc --rc
")]
    Draft(ReleasesDraft),
    /// List the packages of a release
    Packages {
        release_number: i32,
        #[arg(long)]
        json: bool,
    },
    /// Publish a release that is in draft
    Publish { release_number: String },
    /// Mark a release as LTS, a version we commit to keeping devices on
    Lts {
        release_number: String,
        /// Remove the LTS designation instead of applying it
        #[arg(long)]
        clear: bool,
    },
    /// Manage the base OS image attached to a release
    Os {
        #[clap(subcommand)]
        command: OsCommands,
    },
    /// Deploy a release to its canary devices
    ///
    /// Waits until every canary device runs the release, then shows the
    /// deployment. Roll it out to the fleet with `sm releases confirm`.
    Deploy {
        release_number: i32,
        /// Customize which devices are canary released,
        /// by filtering devices by labels.
        /// e.g. --labels label1=a --labels label2=b
        #[arg(long, visible_alias = "labels")]
        canary_device_labels: Option<Vec<String>>,
        /// Return after the deployment starts, without waiting for the canary devices
        #[arg(long)]
        nowait: bool,
    },
    /// Show the deployment of a release: its status, canary devices and their service health
    Deployment { release_number: i32 },
    /// Roll a release out to the whole fleet after its canary
    ///
    /// Fails while a canary device does not run the release yet.
    Confirm { release_number: i32 },
}

#[derive(Args, Debug)]
pub struct ReleasesDraft {
    /// Distribution of the release
    #[arg(long, visible_alias = "distro")]
    distribution_id: Option<i32>,
    /// Release to copy the packages from. Defaults to the deployed release
    #[arg(long, value_name = "RELEASE_ID")]
    from: Option<i32>,
    /// Package to put in the release. It replaces the package with the same
    /// name, or is added when there is none. Can be used multiple times
    #[arg(long = "package", value_name = "PACKAGE_ID")]
    packages: Vec<i32>,
    /// Version of the release
    #[arg(long)]
    version: Option<String>,
    /// Make the release a release candidate. Asked when `--version` is left out
    #[arg(long)]
    rc: bool,
}

impl ReleasesCommands {
    pub async fn handle(self, config: crate::config::Config) -> anyhow::Result<()> {
        match self {
            ReleasesCommands::Get(get) => handle_releases_get(get, config).await?,
            ReleasesCommands::Draft(draft) => draft_release(draft, config).await?,
            ReleasesCommands::Packages {
                release_number,
                json,
            } => {
                let api = api(&config).await?;
                let packages = api.get_release_packages(release_number).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&packages)?);
                } else {
                    let mut table =
                        TablePrint::new_with_headers(vec!["Id", "Name", "Version", "Created at"]);
                    for package in packages {
                        table.add_row(vec![
                            package.id.to_string(),
                            package.name,
                            package.version,
                            package
                                .created_at
                                .map(|t| HumanTime::from(t).to_string())
                                .unwrap_or_default(),
                        ]);
                    }
                    table.print();
                }
            }
            ReleasesCommands::Deployment { release_number } => {
                let api = api(&config).await?;
                print_deployment(&api, release_number).await?;
            }
            ReleasesCommands::Confirm { release_number } => {
                let api = api(&config).await?;
                let deployment = api.confirm_deployment(release_number).await?;
                println!(
                    "Release {release_number} rolls out to the fleet. Deployment status: {}",
                    deployment.status
                );
            }
            ReleasesCommands::Publish { release_number } => {
                let secrets = auth::get_secrets(&config)
                    .await
                    .with_context(|| "Error getting token")?
                    .with_context(|| "No Token found, please Login")?;
                let api = SmithAPI::new(secrets, &config);
                api.update_release(
                    release_number
                        .parse()
                        .context("Failed to parse release number as i32")?,
                    UpdateRelease {
                        draft: Some(false),
                        yanked: None,
                        yanked_reason: None,
                        lts: None,
                    },
                )
                .await?;
                println!("Release published successfully!");
            }
            ReleasesCommands::Lts {
                release_number,
                clear,
            } => {
                let secrets = auth::get_secrets(&config)
                    .await
                    .with_context(|| "Error getting token")?
                    .with_context(|| "No Token found, please Login")?;
                let api = SmithAPI::new(secrets, &config);
                api.update_release(
                    release_number
                        .parse()
                        .context("Failed to parse release number as i32")?,
                    UpdateRelease {
                        draft: None,
                        yanked: None,
                        yanked_reason: None,
                        lts: Some(!clear),
                    },
                )
                .await?;
                if clear {
                    println!("LTS designation removed.");
                } else {
                    println!("Release marked as LTS.");
                }
            }
            ReleasesCommands::Os { command } => command.handle(config).await?,
            ReleasesCommands::Deploy {
                release_number,
                canary_device_labels,
                nowait,
            } => deploy_release(release_number, canary_device_labels, nowait, config).await?,
        };

        Ok(())
    }
}

async fn api(config: &crate::config::Config) -> anyhow::Result<SmithAPI> {
    let secrets = auth::get_secrets(config)
        .await
        .with_context(|| "Error getting token")?
        .with_context(|| "No Token found, please Login")?;
    Ok(SmithAPI::new(secrets, config))
}

async fn deploy_release(
    release_number: i32,
    canary_device_labels: Option<Vec<String>>,
    nowait: bool,
    config: crate::config::Config,
) -> anyhow::Result<()> {
    let api = api(&config).await?;

    api.deploy_release(
        release_number.to_string(),
        canary_device_labels.map(|l| DeploymentRequest {
            canary_device_labels: Some(l),
            canary_device_ids: None,
        }),
    )
    .await?;
    println!("Deployment of release {release_number} started.");

    if !nowait {
        // Devices pick up a new target release on their next update cycle.
        let timeout = std::time::Duration::from_secs(15 * 60);
        let start = std::time::Instant::now();
        let mut last_updated = None;
        loop {
            let devices = api.get_deployment_devices(release_number).await?;
            let updated = devices
                .iter()
                .filter(|d| d.release_id == Some(release_number))
                .count();
            if last_updated != Some(updated) {
                println!("{updated}/{} canary devices run the release", devices.len());
                last_updated = Some(updated);
            }
            if updated == devices.len() {
                break;
            }
            if start.elapsed() > timeout {
                println!(
                    "Not all canary devices updated within {} minutes.",
                    timeout.as_secs() / 60
                );
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
        println!();
        print_deployment(&api, release_number).await?;
    }

    println!(
        "\nCheck the canary with `sm releases deployment {release_number}`, then roll out with `sm releases confirm {release_number}`."
    );
    Ok(())
}

async fn print_deployment(api: &SmithAPI, release_number: i32) -> anyhow::Result<()> {
    let deployment = api.get_deployment(release_number).await?;
    let devices = api.get_deployment_devices(release_number).await?;
    let health = api.get_deployment_service_health(release_number).await?;

    println!(
        "Deployment {} of release {release_number}: {}",
        deployment.id, deployment.status
    );

    let mut table = TablePrint::new_with_headers(vec![
        "Serial number",
        "Release",
        "Target release",
        "Services",
    ]);
    for device in devices {
        let services = health
            .iter()
            .filter(|h| h.serial_number == device.serial_number)
            .map(|h| {
                format!(
                    "{} {} ({} restarts)",
                    h.service_name, h.active_state, h.n_restarts
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        table.add_row(vec![
            device.serial_number,
            device.release_id.map(|r| r.to_string()).unwrap_or_default(),
            device
                .target_release_id
                .map(|r| r.to_string())
                .unwrap_or_default(),
            services,
        ]);
    }
    table.print();
    Ok(())
}

async fn handle_releases_get(
    get: ReleasesGet,
    config: crate::config::Config,
) -> anyhow::Result<()> {
    let ReleasesGet {
        release_number,
        distribution_id,
        json,
    } = get;
    let secrets = auth::get_secrets(&config)
        .await
        .with_context(|| "Error getting token")?
        .with_context(|| "No Token found, please Login")?;
    let api = SmithAPI::new(secrets, &config);
    let releases = match &release_number {
        Some(release_number) => {
            let release = api.get_release_info(release_number.to_string()).await?;
            vec![release]
        }
        None => {
            if let Some(distribution_id) = distribution_id {
                api.get_distribution_releases(distribution_id).await?
            } else {
                api.get_releases().await?
            }
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&releases)?);
    } else {
        let mut distribution_id = distribution_id;
        if release_number.is_some() {
            distribution_id = releases.first().map(|r| r.distribution_id);
        }
        let latest_release_id = if let Some(distribution_id) = distribution_id {
            api.get_latest_distribution_release(distribution_id)
                .await
                .ok()
                .map(|r| r.id)
        } else {
            None
        };
        let mut table = TablePrint::new_with_headers(vec![
            "Id",
            "Distribution name",
            "Version",
            "Info",
            "Created at",
        ]);
        for release in releases {
            let mut meta = String::new();
            if release.draft {
                meta = "Draft".to_string();
            }
            if Some(release.id) == latest_release_id {
                meta = "Deployed".to_string();
            }
            if release.lts {
                meta = if meta.is_empty() {
                    "LTS".to_string()
                } else {
                    format!("{meta}, LTS")
                };
            }
            table.add_row(vec![
                release.id.to_string(),
                release.distribution_name,
                release.version,
                meta,
                HumanTime::from(release.created_at).to_string(),
            ]);
        }
        table.print();
    }

    Ok(())
}

async fn draft_release(draft: ReleasesDraft, config: crate::config::Config) -> anyhow::Result<()> {
    let api = api(&config).await?;

    let distro_id = match draft.distribution_id {
        Some(id) => id,
        None => {
            let distros = api.get_distributions().await?;
            let mut distro_selecter = cliclack::select("Choose distro");
            for distro in distros.iter() {
                distro_selecter = distro_selecter.item(
                    distro.id,
                    format!("{} ({})", distro.name, distro.architecture),
                    distro.description.clone().unwrap_or_default(),
                );
            }
            distro_selecter.interact()?
        }
    };

    let base = match draft.from {
        Some(id) => api.get_release_info(id.to_string()).await?,
        None => api
            .get_latest_distribution_release(distro_id)
            .await
            .context("No deployed release in this distribution, choose one with --from")?,
    };
    if base.distribution_id != distro_id {
        bail!(
            "Release {} belongs to distribution {}, not {distro_id}",
            base.id,
            base.distribution_id
        );
    }

    let (version, release_candidate) = match draft.version {
        Some(version) => (version, draft.rc),
        None => choose_version(&base.version)?,
    };

    let mut packages = api.get_release_packages(base.id).await?;
    let mut changes = Vec::new();
    if !draft.packages.is_empty() {
        let mut all_packages = api.get_packages().await?;
        for package_id in draft.packages {
            let index = all_packages
                .iter()
                .position(|p| p.id == package_id)
                .with_context(|| format!("Package {package_id} not found"))?;
            let new = all_packages.swap_remove(index);
            let (name, new_version) = (new.name.clone(), new.version.clone());
            let old_version = match packages.iter_mut().find(|p| p.name == new.name) {
                Some(slot) => Some(std::mem::replace(slot, new).version),
                None => {
                    packages.push(new);
                    None
                }
            };
            changes.push((name, old_version, new_version));
        }
    }

    let new_release_id = api
        .create_distribution_release(
            distro_id,
            NewDistributionRelease {
                version: version.clone(),
                packages: packages.iter().map(|p| p.id).collect(),
                release_candidate,
            },
        )
        .await?;

    println!(
        "Draft release {new_release_id} ({version}) created from release {} ({}).",
        base.id, base.version
    );
    if changes.is_empty() {
        println!("Packages are the same as in release {}.", base.id);
    } else {
        let mut table = TablePrint::new_with_headers(vec!["Package", "From", "To"]);
        for (name, old_version, new_version) in changes {
            table.add_row(vec![
                name,
                old_version.unwrap_or_else(|| "(added)".to_string()),
                new_version,
            ]);
        }
        table.print();
    }
    println!("Publish it with `sm releases publish {new_release_id}`.");

    Ok(())
}

/// Asks for the semver bump from `current` and whether the release is an RC.
fn choose_version(current: &str) -> anyhow::Result<(String, bool)> {
    let bump = |kind| {
        next_version(current, kind)
            .with_context(|| format!("Release version {current} is not semver"))
    };
    let new_patch = bump(VersionBump::Patch)?;
    let new_minor = bump(VersionBump::Minor)?;
    let new_major = bump(VersionBump::Major)?;
    let version = cliclack::select(format!("Choose version. Current: {current}"))
        .item(
            new_patch.to_string(),
            format!("{new_patch} (PATCH)"),
            "Bug fixes and small changes",
        )
        .item(
            new_minor.to_string(),
            format!("{new_minor} (MINOR)"),
            "New features, backwards compatible",
        )
        .item(
            new_major.to_string(),
            format!("{new_major} (MAJOR)"),
            "Significant new features, may include breaking changes",
        )
        .interact()?;

    let as_rc = cliclack::select(format!("Create as Release Candidate (RC): {version}-rc"))
        .item(false, "No", "")
        .item(true, "Yes", "")
        .interact()?;
    if as_rc {
        Ok((format!("{version}-rc"), true))
    } else {
        Ok((version, false))
    }
}
