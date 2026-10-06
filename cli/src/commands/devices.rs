use chrono::{DateTime, Utc};
use colored::Colorize;

pub fn get_online_colored(serial_number: &str, last_seen: &Option<DateTime<Utc>>) -> String {
    use chrono_humanize::HumanTime;
    let now = chrono::Utc::now();

    match last_seen {
        Some(parsed_time) => {
            let duration = now.signed_duration_since(parsed_time.with_timezone(&chrono::Utc));

            if duration.num_minutes() < 5 {
                serial_number.bright_green().to_string()
            } else {
                let human_time = HumanTime::from(parsed_time.with_timezone(&chrono::Utc));
                format!("{} ({})", serial_number, human_time)
                    .red()
                    .to_string()
            }
        }
        None => format!("{} (Unknown)", serial_number).yellow().to_string(),
    }
}

/// Pins or unpins the selected devices. With `release`, the devices are first
/// retargeted to it, so they move to that release and stay there.
pub async fn set_pinned(
    api: &crate::api::SmithAPI,
    selector: &crate::cli::DeviceSelector,
    pin: bool,
    release: Option<i32>,
    yes: bool,
) -> anyhow::Result<()> {
    use std::collections::HashSet;
    use std::io::{self, Write};

    let verb = if pin { "pin" } else { "unpin" };
    if !selector.has_filters() {
        anyhow::bail!(
            "No device IDs or filters specified. Example: sm {verb} <device-serial>... or sm {verb} -l key=value"
        );
    }

    let target_release = match release {
        Some(id) => {
            let release = api.get_release_info(id.to_string()).await?;
            if release.draft || release.yanked {
                anyhow::bail!(
                    "Release {} ({}) is a draft or yanked and cannot be targeted.",
                    release.id,
                    release.version
                );
            }
            Some(release)
        }
        None => None,
    };

    let mut seen = HashSet::new();
    let devices: Vec<_> = crate::resolve_devices_from_selector(api, selector)
        .await?
        .into_iter()
        .filter(|d| seen.insert(d.id))
        // Without a release to set, devices already in the wanted state need no change.
        .filter(|d| target_release.is_some() || d.follow_latest == pin)
        .collect();

    if devices.is_empty() {
        println!("No matching devices need to change.");
        return Ok(());
    }

    let action = match &target_release {
        Some(r) => format!("Pinning to release {} ({})", r.id, r.version),
        None if pin => "Pinning".to_string(),
        None => "Unpinning".to_string(),
    };
    println!("{} {} device(s):", action.bold(), devices.len());
    for d in devices.iter().take(10) {
        // Pinning keeps the target, so the target is what the device stays on.
        let target = target_release
            .as_ref()
            .or(d.target_release.as_ref())
            .map(|r| format!("{} ({})", r.id, r.version))
            .unwrap_or_else(|| "-".to_string());
        let mut line = format!("  - {}  target {}", d.serial_number, target);
        if let (Some(target), Some(current)) = (&target_release, &d.release)
            && target.distribution_id != current.distribution_id
        {
            line.push_str(
                &format!(
                    "  different distribution: {} → {}",
                    current.distribution_name, target.distribution_name
                )
                .yellow()
                .to_string(),
            );
        }
        println!("{line}");
    }
    if devices.len() > 10 {
        println!(
            "  {} ({} more devices...)",
            "...".dimmed(),
            devices.len() - 10
        );
    }
    if !pin {
        println!(
            "\n{}",
            "Unpinned devices receive the next full rollout of their distribution.".yellow()
        );
    }

    if !yes {
        print!("\n{} [y/N]: ", "Proceed?".bold());
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if input.trim().to_lowercase() != "y" {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let ids: Vec<i32> = devices.iter().map(|d| d.id).collect();
    // Pin before retargeting, so a rollout can never land between the two calls.
    let changed = api.set_devices_follow_latest(&ids, !pin).await?;
    if let Some(r) = &target_release {
        api.set_devices_target_release(&ids, r.id).await?;
    }

    println!(
        "\n{} {} device(s) {}.",
        "Done:".bright_green(),
        changed,
        if pin { "pinned" } else { "unpinned" }
    );
    Ok(())
}
