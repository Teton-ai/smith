use crate::{
    api::{DeploymentDetail, DeploymentSummary, DeviceCounts, SmithAPI},
    auth,
    print::TablePrint,
};
use anyhow::Context as _;
use chrono_humanize::HumanTime;
use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum DeploymentsCommands {
    /// List deployments, newest first
    #[command(after_long_help = "\
Examples:
  sm deployments list
  sm deployments list --release 1516
  sm deployments list --distribution 14 --json
")]
    List {
        /// Only deployments of this release
        #[arg(long)]
        release: Option<i32>,
        /// Only deployments in this distribution
        #[arg(long, visible_alias = "distro")]
        distribution: Option<i32>,
        /// At most this many, up to 200
        #[arg(long, default_value_t = 20)]
        limit: i64,
        #[arg(long)]
        json: bool,
    },
    /// Show a deployment: where it is, its waves, what blocks confirming it,
    /// and its activity
    #[command(after_long_help = "\
Examples:
  sm deployments get 42
")]
    Get {
        deployment_id: i32,
        #[arg(long)]
        json: bool,
    },
    /// Confirm a stable release's rollout once the canary is healthy. The fleet
    /// then updates in staggered waves. Release candidates cannot be confirmed.
    #[command(after_long_help = "\
Examples:
  sm deployments confirm 42
")]
    Confirm { deployment_id: i32 },
}

impl DeploymentsCommands {
    pub async fn handle(self, config: crate::config::Config) -> anyhow::Result<()> {
        let secrets = auth::get_secrets(&config)
            .await
            .with_context(|| "Error getting token")?
            .with_context(|| "No Token found, please Login")?;
        let api = SmithAPI::new(secrets, &config);

        match self {
            DeploymentsCommands::List {
                release,
                distribution,
                limit,
                json,
            } => {
                let deployments = api.list_deployments(release, distribution, limit).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&deployments)?);
                    return Ok(());
                }
                let mut table = TablePrint::new_with_headers(vec![
                    "Id",
                    "Release",
                    "Distribution",
                    "State",
                    "Updated",
                    "Started",
                ]);
                for deployment in &deployments {
                    table.add_row(vec![
                        deployment.id.to_string(),
                        release_label(deployment),
                        deployment.distribution_name.clone(),
                        state_label(&deployment.state).to_string(),
                        format!("{}/{}", deployment.counts.updated, deployment.counts.total),
                        HumanTime::from(deployment.created_at).to_string(),
                    ]);
                }
                table.print();
            }
            DeploymentsCommands::Get {
                deployment_id,
                json,
            } => {
                let detail = api.get_deployment(deployment_id).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&detail)?);
                } else {
                    print_detail(&detail);
                }
            }
            DeploymentsCommands::Confirm { deployment_id } => {
                api.control_deployment(deployment_id, "promote", None)
                    .await?;
                let detail = api.get_deployment(deployment_id).await?;
                println!(
                    "Rollout confirmed: {} devices update in {} waves.",
                    detail.summary.counts.total - detail.summary.canary.total,
                    detail.waves.iter().filter(|w| w.wave > 0).count()
                );
            }
        }
        Ok(())
    }
}

fn release_label(deployment: &DeploymentSummary) -> String {
    let mut label = format!("v{}", deployment.release_version);
    if deployment.release_candidate {
        label.push_str(" (RC)");
    }
    if deployment.release_yanked {
        label.push_str(" (yanked)");
    }
    if let Some(from) = &deployment.rolled_back_from_version {
        label = format!("v{from} → {label}, rollback");
    }
    label
}

fn state_label(state: &str) -> &str {
    match state {
        "canary" => "Canary",
        "rolling_out" => "Rolling out",
        "paused" => "Paused",
        "completed" => "Deployed",
        "canceled" => "Canceled",
        "failed" => "Failed",
        other => other,
    }
}

/// Counts worth calling out, skipping the zeros.
fn counts_label(counts: &DeviceCounts) -> String {
    let mut parts = vec![format!("{}/{} updated", counts.updated, counts.total)];
    for (count, label) in [
        (counts.updating, "pending"),
        (counts.scheduled, "waiting"),
        (counts.held, "held"),
        (counts.offline, "offline"),
        (counts.timed_out, "timed out"),
        (counts.unhealthy, "unhealthy"),
        (counts.superseded, "moved elsewhere"),
        (counts.canceled, "canceled"),
    ] {
        if count > 0 {
            parts.push(format!("{count} {label}"));
        }
    }
    parts.join(", ")
}

fn print_detail(detail: &DeploymentDetail) {
    let summary = &detail.summary;
    let audience = match (summary.kind.as_str(), summary.release_candidate) {
        ("rollback", _) => "every device on the yanked release, directly",
        ("direct", false) => "selected devices",
        (_, true) => "canary devices only",
        (_, false) => "fleet",
    };
    println!(
        "Deployment {} · {} · {} → {}",
        summary.id,
        release_label(summary),
        summary.distribution_name,
        audience
    );
    println!(
        "State: {} · started {} by {}",
        state_label(&summary.state),
        HumanTime::from(summary.created_at),
        summary.created_by.as_deref().unwrap_or("unknown")
    );
    println!("Devices: {}", counts_label(&summary.counts));
    if let Some(next) = summary.next_wave_at {
        println!("Next wave opens {}", HumanTime::from(next));
    }
    if let Some(deadline) = summary.times_out_at {
        println!(
            "Devices not updated {} time out.",
            HumanTime::from(deadline)
        );
    }

    if !detail.waves.is_empty() {
        println!();
        let mut table = TablePrint::new_with_headers(vec!["Wave", "Opens", "Devices"]);
        for wave in &detail.waves {
            table.add_row(vec![
                if wave.wave == 0 {
                    "Canary".to_string()
                } else {
                    wave.wave.to_string()
                },
                HumanTime::from(wave.opens_at).to_string(),
                counts_label(&wave.counts),
            ]);
        }
        table.print();
    }

    if summary.state == "canary" {
        println!();
        if detail.actions.can_promote {
            println!("Ready to confirm: sm deployments confirm {}", summary.id);
        } else {
            for blocker in &detail.actions.promote_blockers {
                println!("Not ready: {blocker}");
            }
            if let Some(reason) = &detail.actions.denied_reason {
                println!("{reason}");
            }
        }
    }

    if !detail.events.is_empty() {
        println!();
        for event in detail.events.iter().take(5) {
            println!(
                "{} · {}{}{}",
                HumanTime::from(event.created_at),
                event.kind,
                event
                    .user_email
                    .as_deref()
                    .map(|email| format!(" by {email}"))
                    .unwrap_or_default(),
                event
                    .detail
                    .as_deref()
                    .map(|detail| format!(": {detail}"))
                    .unwrap_or_default()
            );
        }
    }
}
