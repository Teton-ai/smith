use crate::{api::SmithAPI, auth, print::TablePrint};
use anyhow::{Context as _, bail};
use chrono_humanize::HumanTime;
use clap::{Args, Subcommand};
use models::{
    deployment::{DeploymentRequest, DeploymentStatus},
    device::DeviceFilter,
    distribution::NewDistributionRelease,
    release::UpdateRelease,
};
use regex::Regex;

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
    /// Deploy a release
    Deploy {
        release_number: String,
        /// Customize which devices are canary released,
        /// by filtering devices by labels.
        /// e.g. --labels label1=a --labels label2=b
        #[arg(long, visible_alias = "labels")]
        canary_device_labels: Option<Vec<String>>,
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
                    release_number.parse().unwrap(),
                    UpdateRelease {
                        draft: Some(false),
                        yanked: None,
                    },
                )
                .await?;
                println!("Release published successfully!");
            }
            ReleasesCommands::Deploy {
                release_number,
                canary_device_labels,
            } => deploy_release(release_number, canary_device_labels, config).await?,
        };

        Ok(())
    }
}

async fn deploy_release(
    release_number: String,
    canary_device_labels: Option<Vec<String>>,
    config: crate::config::Config,
) -> anyhow::Result<()> {
    let secrets = auth::get_secrets(&config)
        .await
        .with_context(|| "Error getting token")?
        .with_context(|| "No Token found, please Login")?;
    let api = SmithAPI::new(secrets, &config);

    // Start the deployment
    api.deploy_release(
        release_number.clone(),
        canary_device_labels.map(|l| DeploymentRequest {
            canary_device_labels: Some(l),
        }),
    )
    .await?;

    // Set up polling parameters
    let start_time = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(5 * 60); // 5 minutes
    let check_interval = std::time::Duration::from_secs(5); // Check every 5 seconds

    println!("Checking for deployment completion...");

    // Start polling loop
    loop {
        // Check if we've exceeded the timeout
        if start_time.elapsed() > timeout {
            println!("Deployment timed out after 5 minutes");
            return Err(anyhow::anyhow!("Deployment timed out after 5 minutes"));
        }

        // Check deployment status
        let deployment = api
            .deploy_release_check_done(release_number.clone())
            .await?;

        // Check if the deployment is done
        let status = deployment.status;
        println!("Current status: {}", status);

        if status == DeploymentStatus::Done {
            println!("Deployment completed successfully!");
            return Ok(());
        }

        // If status is "failed" or any other terminal state, we can exit early
        if status == DeploymentStatus::Failed {
            return Err(anyhow::anyhow!("Deployment failed"));
        }

        // Wait before the next check
        println!(
            "Waiting for devices to update... (elapsed: {:?})",
            start_time.elapsed()
        );
        tokio::time::sleep(check_interval).await;
    }
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
        if release_number.is_some_and(|_| releases.len() == 1) {
            distribution_id = releases.first().map(|r| r.distribution_id);
        }
        let latest_release_id = if let Some(distribution_id) = distribution_id {
            let latest_distro_release =
                api.get_latest_distribution_release(distribution_id).await?;
            Some(latest_distro_release.id)
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
                meta = "Latest".to_string();
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

    let re = Regex::new(r"^(\d+)\.(\d+)\.(\d+)").unwrap();
    let captures = re.captures(current_version).unwrap();
    let major: i32 = captures.get(1).unwrap().as_str().parse().unwrap();
    let minor: i32 = captures.get(2).unwrap().as_str().parse().unwrap();
    let patch: i32 = captures.get(3).unwrap().as_str().parse().unwrap();

    let new_patch = format!("{major}.{minor}.{}", patch + 1);
    let new_minor = format!("{major}.{}.0", minor + 1);
    let new_major = format!("{}.0.0", major + 1);
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
            },
        )
        .await?;

    spinner.stop(format!("Release created. New release id {new_release_id}"));

    Ok(())
}
