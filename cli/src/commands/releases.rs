use crate::{api::SmithAPI, auth, commands::os::OsCommands, print::TablePrint};
use anyhow::Context as _;
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
    /// Draft a new release
    Draft,
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
    /// Deploy a release. Canaries are picked automatically. A stable release
    /// then rolls out to the fleet; a release candidate needs a pilot group,
    /// which it rolls out to instead. A pilot group under 10 devices is
    /// deployed to directly, without a canary.
    #[command(after_long_help = "\
Examples:
  sm releases deploy 1520
  sm releases deploy 1521 --pilot-label env=staging
  sm releases deploy 1521 --pilot-device 12 --pilot-device 15
")]
    Deploy {
        release_number: String,
        /// A release candidate's pilot group: devices with any of these labels.
        /// Repeat for several.
        #[arg(long = "pilot-label", conflicts_with = "pilot_device_ids")]
        pilot_device_labels: Option<Vec<String>>,
        /// A release candidate's pilot group: these device IDs. Repeat for
        /// several.
        #[arg(long = "pilot-device")]
        pilot_device_ids: Option<Vec<i32>>,
    },
}

impl ReleasesCommands {
    pub async fn handle(self, config: crate::config::Config) -> anyhow::Result<()> {
        match self {
            ReleasesCommands::Get(get) => handle_releases_get(get, config).await?,
            ReleasesCommands::Draft => draft_release(config).await?,
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
                pilot_device_labels,
                pilot_device_ids,
            } => {
                deploy_release(
                    release_number,
                    DeploymentRequest {
                        pilot_device_labels,
                        pilot_device_ids,
                    },
                    config,
                )
                .await?
            }
        };

        Ok(())
    }
}

async fn deploy_release(
    release_number: String,
    request: DeploymentRequest,
    config: crate::config::Config,
) -> anyhow::Result<()> {
    let secrets = auth::get_secrets(&config)
        .await
        .with_context(|| "Error getting token")?
        .with_context(|| "No Token found, please Login")?;
    let api = SmithAPI::new(secrets, &config);

    let deployment = api.deploy_release(release_number, Some(request)).await?;
    let detail = api.get_deployment(deployment.id).await?;
    if detail.summary.kind == "direct" {
        println!(
            "Deployment {} started: {} pilot devices update directly.",
            deployment.id, detail.summary.counts.total
        );
    } else {
        println!(
            "Deployment {} started: {} canary devices.",
            deployment.id, detail.summary.canary.total
        );
        println!(
            "Once they are healthy, confirm with: sm deployments confirm {}",
            deployment.id
        );
    }
    println!("Follow it with: sm deployments get {}", deployment.id);
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

async fn draft_release(config: crate::config::Config) -> anyhow::Result<()> {
    let secrets = auth::get_secrets(&config)
        .await
        .with_context(|| "Error getting token")?
        .with_context(|| "No Token found, please Login")?;
    let api = SmithAPI::new(secrets, &config);

    cliclack::intro("Draft New Release")?;
    let spinner = cliclack::spinner();

    spinner.start("Getting distributions");
    let distros = api.get_distributions().await?;
    spinner.stop("Distributions fetched");

    let mut distro_selecter = cliclack::select("Choose distro");
    for distro in distros.iter() {
        distro_selecter = distro_selecter.item(
            distro.id,
            format!("{} ({})", distro.name, distro.architecture),
            distro.description.clone().unwrap_or_default(),
        );
    }
    let distro_id = distro_selecter.interact()?;

    let distro = distros
        .iter()
        .find(|distro| distro.id == distro_id)
        .unwrap();

    let spinner = cliclack::spinner();
    spinner.start("Fetching previous releases");
    let releases = api.get_distribution_releases(distro_id).await?;
    let latest_release = releases.first().context("Could not find latest release")?;
    spinner.stop("Previous releases fetched");
    let current_version = &latest_release.version;

    let bump = |kind| {
        next_version(current_version, kind)
            .with_context(|| format!("Latest release version {current_version} is not semver"))
    };
    let new_patch = bump(VersionBump::Patch)?;
    let new_minor = bump(VersionBump::Minor)?;
    let new_major = bump(VersionBump::Major)?;
    let mut version = cliclack::select(format!("Choose version. Current: {current_version}"))
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
        version = format!("{version}-rc");
    };

    let spinner = cliclack::spinner();
    spinner.start("Creating release");
    let packages = api.get_release_packages(latest_release.id).await?;

    let new_release_id = api
        .create_distribution_release(
            distro.id,
            NewDistributionRelease {
                version,
                packages: packages.into_iter().map(|p| p.id).collect(),
                release_candidate: as_rc,
            },
        )
        .await?;

    spinner.stop(format!("Release created. New release id {new_release_id}"));

    Ok(())
}
