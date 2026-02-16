mod api;
mod auth;
mod cli;
mod commands;
mod config;
mod print;
mod tunnel;

use crate::cli::{
    Cli, Commands, DistroCommands, GetResourceType, RestartResourceType, StatusResourceType,
};
use crate::commands::devices::get_online_colored;
use crate::print::TablePrint;
use anyhow::{Context, bail};
use api::SmithAPI;
use chrono_humanize::HumanTime;
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use models::device::{Device, DeviceFilter};
use std::{
    collections::HashSet,
    io::{self, IsTerminal, Read},
    thread,
    time::Duration,
};
use termion::raw::IntoRawMode;
use tokio::sync::oneshot;
use tunnel::Session;

fn print_markdown_help() {
    println!("# Smith CLI Commands\n");
    println!("This is a comprehensive guide to all available `sm` commands.\n");

    println!("## Authentication\n");
    println!("### `sm auth login [--no-open]`");
    println!("Login to Smith API. Opens browser by default unless `--no-open` is specified.\n");
    println!("### `sm auth logout`");
    println!("Logs out the current session.\n");
    println!("### `sm auth show`");
    println!("Shows the current authentication token being used.\n");

    println!("## Profile Management\n");
    println!("### `sm profile [PROFILE_NAME]`");
    println!(
        "View or change the current profile. If no profile name is provided, shows the current profile.\n"
    );

    println!("## Device Management\n");
    println!("### `sm devices ls [--json] [-l KEY=VALUE]... [--online|--offline]`");
    println!("List all devices. Use `--json` flag to output in JSON format.");
    println!("Use `-l` or `--label` to filter by labels (e.g., `-l department=xd -l region=us`).");
    println!(
        "Use `--online` to show only online devices or `--offline` to show only offline devices.\n"
    );
    println!("### `sm devices logs <SERIAL_NUMBER> [--nowait]`");
    println!("Get logs for a specific device by serial number.");
    println!(
        "- `--nowait`: Queue the command and return immediately without waiting for results. Use `sm command <id>` to check status later.\n"
    );

    println!("## Service Management\n");
    println!("### `sm service status --unit <UNIT> <SERIAL_NUMBER> [--nowait]`");
    println!("Get status of a systemd service on a device.");
    println!("- `--unit`: Service unit name (e.g., smithd.service)");
    println!(
        "- `--nowait`: Queue the command and return immediately without waiting for results.\n"
    );

    println!("## Device Status\n");
    println!("### `sm status <SERIAL_NUMBER> [--nowait]`");
    println!("Get smithd status for a device (runs 'smithd status' command).");
    println!("\n**Output includes:**");
    println!("- Update/upgrade status (whether the system is up-to-date)");
    println!("- Installed package versions (currently running on the device)");
    println!("- Target package versions (versions that should be running)");
    println!("- Update status flag (true/false for each package indicating if it's updated)");
    println!("\n**Options:**");
    println!(
        "- `--nowait`: Queue the command and return immediately. Commands typically take at least 30 seconds to complete.\n"
    );

    println!("## Command Management\n");
    println!("### `sm command <ID>...`");
    println!("Check command results by ID. Format: `device_id:command_id`");
    println!("Can check multiple commands at once by providing multiple IDs.\n");

    println!("## Distribution Management\n");
    println!("### `sm distributions ls [--json]` (alias: `sm distro ls`)");
    println!("List current distributions. Use `--json` flag to output in JSON format.\n");
    println!("### `sm distributions releases` (alias: `sm distro releases`)");
    println!("List current distribution releases.\n");

    println!("## Release Management\n");
    println!("### `sm release <RELEASE_NUMBER> [--deploy]`");
    println!("Interact with a specific release.");
    println!("- Without `--deploy`: View release information");
    println!("- With `--deploy`: Deploy the release and wait for completion (5 minute timeout)\n");

    println!("## Tunneling\n");
    println!("### `sm tunnel <SERIAL_NUMBER> [--overview-debug]`");
    println!("Create an SSH tunnel to a device for direct access.\n");

    println!("## Utility Commands\n");
    println!("### `sm completion <SHELL>`");
    println!(
        "Generate shell completion scripts. Supported shells: bash, zsh, fish, powershell, elvish.\n"
    );
    println!("### `sm update [--check]`");
    println!("Update the CLI tool.");
    println!("- `--check`: Only check for updates without installing\n");
    println!("### `sm agent-help`");
    println!("Print this markdown help guide (useful for agents).\n");

    println!("## Notes\n");
    println!("- Commands with `--nowait` are recommended for agents and automation");
    println!("- Use `sm command <device_id>:<command_id>` to check the status of queued commands");
    println!("- Most device commands take at least 30 seconds to complete");
    println!("- JSON output is available for `devices ls` and `distributions ls` commands");
    println!(
        "- Multiple labels can be combined for filtering (e.g., `sm devices ls -l department=xd -l env=prod`)"
    );
    println!("- Devices are considered online if they pinged within the last 5 minutes");
}

fn update(_check: bool) -> Result<(), anyhow::Error> {
    let updater = self_update::backends::github::Update::configure()
        .repo_owner("teton-ai")
        .repo_name("smith")
        .bin_name("sm")
        .show_download_progress(true)
        .current_version(self_update::cargo_crate_version!())
        .build()?;

    match updater.update() {
        Ok(status) => match status {
            self_update::Status::UpToDate(version) => {
                println!("The tool is already up-to-date with version: {}", version);
            }
            self_update::Status::Updated(version) => {
                println!("Successfully updated to version: {}", version);
            }
        },
        Err(e) => {
            println!("Error during update: {}", e);
            return Err(anyhow::anyhow!("Update failed"));
        }
    }

    Ok(())
}

fn check_and_update_if_needed() -> Result<(), anyhow::Error> {
    let last_check_file = config::Config::get_last_update_check_file();

    let should_check = std::fs::read_to_string(&last_check_file)
        .ok()
        .and_then(|contents| contents.trim().parse::<i64>().ok())
        .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0))
        .map(|last_check| {
            let now = chrono::Utc::now();
            let duration = now.signed_duration_since(last_check);
            duration.num_hours() >= 24
        })
        .unwrap_or(true);

    if should_check {
        let current_version = self_update::cargo_crate_version!();
        let updater = self_update::backends::github::Update::configure()
            .repo_owner("teton-ai")
            .repo_name("smith")
            .bin_name("sm")
            .show_download_progress(false)
            .current_version(current_version)
            .build()?;

        if let Ok(status) = updater.get_latest_release()
            && status.version != current_version
        {
            println!(
                "A new version {} is available! Run 'sm update' to update.",
                status.version
            );
        }

        let now = chrono::Utc::now().timestamp();
        std::fs::write(&last_check_file, now.to_string())?;
    }

    Ok(())
}

fn parse_label_filters(
    labels: Vec<String>,
) -> anyhow::Result<Option<std::collections::HashMap<String, String>>> {
    if labels.is_empty() {
        return Ok(None);
    }

    let mut map = std::collections::HashMap::new();
    for label_str in labels {
        let parts: Vec<&str> = label_str.splitn(2, '=').collect();
        if parts.len() == 2 {
            map.insert(parts[0].to_string(), parts[1].to_string());
        } else {
            return Err(anyhow::anyhow!(
                "Invalid label format: '{}'. Expected 'key=value'",
                label_str
            ));
        }
    }
    Ok(Some(map))
}

/// Resolves devices from a DeviceSelector
/// Returns a Vec of all matching devices (may contain duplicates if multiple search terms match the same device)
async fn resolve_devices_from_selector(
    api: &SmithAPI,
    selector: &cli::DeviceSelector,
) -> anyhow::Result<Vec<Device>> {
    let online_filter = if selector.online {
        Some(true)
    } else if selector.offline {
        Some(false)
    } else {
        None
    };

    if selector.ids.is_empty() {
        // No IDs specified, apply filters only
        api.get_devices(DeviceFilter {
            labels: selector.labels.clone(),
            online: online_filter,
            ..Default::default()
        })
        .await
    } else if selector.search {
        // Use search filter for partial matching on multiple IDs
        let mut all_devices = Vec::new();

        for search_term in &selector.ids {
            let devices = api
                .get_devices(DeviceFilter {
                    labels: selector.labels.clone(),
                    online: online_filter,
                    search: Some(search_term.clone()),
                    ..Default::default()
                })
                .await
                .with_context(|| format!("Failed to search for device '{}'", search_term))?;

            if devices.is_empty() {
                eprintln!(
                    "{}: No devices found matching: '{}'",
                    "Warning".yellow(),
                    search_term
                );
            }
            all_devices.extend(devices);
        }
        Ok(all_devices)
    } else {
        // Get specific devices by exact serial number
        let mut all_devices = Vec::new();

        for id in &selector.ids {
            let devices = api
                .get_devices(DeviceFilter {
                    serial_number: Some(id.clone()),
                    ..Default::default()
                })
                .await
                .with_context(|| format!("Failed to fetch device '{}'", id))?;

            if devices.is_empty() {
                eprintln!("{}: Device not found: '{}'", "Warning".yellow(), id);
            }
            all_devices.extend(devices);
        }
        Ok(all_devices)
    }
}

/// Resolves exactly one device from a DeviceSelector
/// Returns an error if zero or multiple devices are found
async fn resolve_single_device_from_selector(
    api: &SmithAPI,
    selector: &cli::DeviceSelector,
    command_name: &str,
) -> anyhow::Result<Device> {
    // Validate single device for search mode
    if selector.search && selector.ids.len() != 1 {
        bail!(
            "{} command only supports a single device. Please specify exactly one device ID or search term.",
            command_name
        );
    }

    // Validate single device for exact mode
    if !selector.search && selector.ids.len() > 1 {
        bail!(
            "{} command only supports a single device. Please specify exactly one device.",
            command_name
        );
    }

    let devices = resolve_devices_from_selector(api, selector).await?;

    if devices.is_empty() {
        bail!("No device found matching the selector");
    }

    if devices.len() > 1 {
        eprintln!(
            "Error: Multiple devices matched ({} devices)",
            devices.len()
        );
        eprintln!("Matched devices:");
        for device in devices.iter().take(10) {
            eprintln!("  - {}", device.serial_number);
        }
        if devices.len() > 10 {
            eprintln!("  ... and {} more", devices.len() - 10);
        }
        bail!(
            "{} command only supports a single device. Please refine your selector.",
            command_name
        );
    }

    Ok(devices.into_iter().next().unwrap())
}

/// Legacy helper for commands that have separate device_filters + selector parameters
/// Combines them into a DeviceSelector and delegates to resolve_devices_from_selector
async fn resolve_target_devices(
    api: &SmithAPI,
    device_filters: Vec<String>,
    labels: Vec<String>,
    online: bool,
    offline: bool,
    search: bool,
) -> anyhow::Result<Vec<Device>> {
    let selector = cli::DeviceSelector {
        ids: device_filters,
        labels,
        online,
        offline,
        search,
    };

    resolve_devices_from_selector(api, &selector).await
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if !matches!(cli.command, Some(Commands::Update { .. })) {
        let _ = tokio::task::spawn_blocking(check_and_update_if_needed)
            .await
            .ok();
    }
    let mut config = match config::Config::load().await {
        Ok(config) => config,
        Err(err) => {
            if matches!(err.downcast_ref::<io::Error>(), Some(e) if e.kind() == io::ErrorKind::NotFound)
            {
                println!("Config file not found.");
                println!("Would you like to load the default configuration from 1pasword? [y/N]");

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;

                if input.trim().to_lowercase() == "y" {
                    println!("Creating default configuration...");
                    let default_config = config::Config::default().await;
                    default_config.save().await?;
                    println!("Default configuration created successfully.");
                    return Ok(());
                }
            }
            return Err(err);
        }
    };

    match cli.command {
        Some(command) => match command {
            Commands::Update { check } => {
                tokio::task::spawn_blocking(move || update(check))
                    .await
                    .expect("Update task panicked")?;
            }
            Commands::Profile { profile } => {
                if let Some(profile) = profile {
                    println!("Changing profile to {}", profile);
                    config.change_profile(profile).await?;
                    println!("new: {}", config);
                }
            }
            Commands::Get { resource } => match resource {
                GetResourceType::Device {
                    selector,
                    json,
                    output,
                } => {
                    let secrets = auth::get_secrets(&config)
                        .await
                        .with_context(|| "Error getting token")?
                        .with_context(|| "No Token found, please Login")?;

                    let api = SmithAPI::new(secrets, &config);

                    let devices = resolve_devices_from_selector(&api, &selector).await?;

                    if devices.is_empty() {
                        println!("No devices found");
                        return Ok(());
                    }

                    // Handle output format
                    if let Some(output_format) = output {
                        match output_format.as_str() {
                            "json" => {
                                println!("{}", serde_json::to_string_pretty(&devices)?);
                                return Ok(());
                            }
                            "wide" => {
                                // Wide format - will show more columns in table
                            }
                            // Custom field selection
                            field => {
                                for device in &devices {
                                    let value = match field {
                                        "serial_number" => device.serial_number.clone(),
                                        "id" => device.id.to_string(),
                                        "ip_address" | "ip" => device
                                            .ip_address
                                            .as_ref()
                                            .map(|ip| ip.ip_address.to_string())
                                            .unwrap_or_default(),
                                        "version" => device
                                            .system_info
                                            .as_ref()
                                            .and_then(|info| info["smith"]["version"].as_str())
                                            .unwrap_or("")
                                            .to_string(),
                                        "release" => device
                                            .release
                                            .as_ref()
                                            .map(|r| r.id.to_string())
                                            .unwrap_or_default(),
                                        "target_release" => device
                                            .target_release
                                            .as_ref()
                                            .map(|r| r.id.to_string())
                                            .unwrap_or_default(),
                                        _ => {
                                            eprintln!(
                                                "Unknown field: '{}'. Available fields: serial_number, id, ip_address, version, release, target_release",
                                                field
                                            );
                                            return Err(anyhow::anyhow!("Invalid output field"));
                                        }
                                    };
                                    println!("{}", value);
                                }
                                return Ok(());
                            }
                        }
                    }

                    if json {
                        println!("{}", serde_json::to_string_pretty(&devices)?);
                        return Ok(());
                    }

                    // Display devices in table format if multiple, or detailed view if single
                    if devices.len() == 1 {
                        let device = &devices[0];

                        // Status line
                        let status_str = if let Some(last_seen) = &device.last_seen {
                            use chrono_humanize::HumanTime;
                            let now = chrono::Utc::now();
                            let duration =
                                now.signed_duration_since(last_seen.with_timezone(&chrono::Utc));
                            if duration.num_minutes() < 5 {
                                format!("● {}", "online".bright_green())
                            } else {
                                let human_time =
                                    HumanTime::from(last_seen.with_timezone(&chrono::Utc));
                                format!("○ {} ({})", "offline".red(), human_time)
                            }
                        } else {
                            format!("○ {}", "unknown".yellow())
                        };

                        // Main info line
                        let version = device
                            .system_info
                            .as_ref()
                            .and_then(|info| info["smith"]["version"].as_str())
                            .unwrap_or("unknown");

                        let os_info = device
                            .system_info
                            .as_ref()
                            .and_then(|info| {
                                let os_name = info.get("os")?.get("name")?.as_str()?;
                                let arch = info.get("architecture")?.as_str()?;
                                Some(format!("{}/{}", os_name, arch))
                            })
                            .unwrap_or_else(|| "unknown".to_string());

                        println!(
                            "{} {} {}",
                            device.serial_number.bold(),
                            status_str,
                            format!("(id: {})", device.id).dimmed()
                        );
                        println!("  {} {}", "smith:".dimmed(), version);
                        println!("  {} {}", "os:".dimmed(), os_info);

                        // Release information
                        if let Some(release) = &device.release {
                            let target_str = if let Some(target_release) = &device.target_release {
                                if release.id != target_release.id {
                                    format!(
                                        " → {} {}",
                                        target_release.id,
                                        "(update available)".yellow()
                                    )
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            };
                            println!("  {} {}{}", "release:".dimmed(), release.id, target_str);
                        }

                        // IP Address
                        if let Some(ip_info) = &device.ip_address {
                            println!("  {} {}", "ip:".dimmed(), ip_info.ip_address);
                        }

                        // Modem
                        if let Some(modem) = &device.modem {
                            println!("  {} {}", "modem:".dimmed(), &modem.network_provider);
                        }

                        // Network info
                        if let Some(network) = &device.network {
                            let mut network_parts = Vec::new();
                            if let Some(download) = network.download_speed_mbps {
                                network_parts.push(format!("↓{:.1}Mbps", download));
                            }
                            if let Some(upload) = network.upload_speed_mbps {
                                network_parts.push(format!("↑{:.1}Mbps", upload));
                            }
                            if let Some(score) = network.network_score {
                                network_parts.push(format!("score:{}", score));
                            }
                            if !network_parts.is_empty() {
                                println!("  {} {}", "network:".dimmed(), network_parts.join(" "));
                            }
                        }

                        // Note
                        if let Some(note) = &device.note
                            && !note.is_empty()
                        {
                            println!("  {} {}", "note:".dimmed(), note);
                        }

                        // Labels
                        if !device.labels.is_empty() {
                            let labels_str = device
                                .labels
                                .iter()
                                .map(|(k, v)| format!("{}={}", k, v))
                                .collect::<Vec<String>>()
                                .join(", ");
                            println!("  {} {}", "labels:".dimmed(), labels_str.cyan());
                        }
                    } else {
                        // Multiple devices - show table format
                        let mut table = TablePrint::new_with_headers(vec![
                            "Device", "Version", "OS/Arch", "Release", "IP", "Labels",
                        ]);
                        for d in devices {
                            let version = d
                                .system_info
                                .as_ref()
                                .and_then(|info| info["smith"]["version"].as_str())
                                .unwrap_or("unknown")
                                .to_string();

                            let os_arch = d
                                .system_info
                                .as_ref()
                                .and_then(|info| {
                                    let os_name = info.get("os")?.get("name")?.as_str()?;
                                    let arch = info.get("architecture")?.as_str()?;
                                    Some(format!("{}/{}", os_name, arch))
                                })
                                .unwrap_or_else(|| "unknown".to_string());

                            let release_str = if let Some(release) = &d.release {
                                if let Some(target) = &d.target_release {
                                    if release.id != target.id {
                                        format!("{} → {}", release.id, target.id)
                                    } else {
                                        release.id.to_string()
                                    }
                                } else {
                                    release.id.to_string()
                                }
                            } else {
                                "-".to_string()
                            };

                            let ip_str = d
                                .ip_address
                                .as_ref()
                                .map(|ip| ip.ip_address.to_string())
                                .unwrap_or_else(|| "-".to_string());

                            let labels_str = if d.labels.is_empty() {
                                "-".to_string()
                            } else {
                                d.labels
                                    .iter()
                                    .map(|(k, v)| format!("{}={}", k, v))
                                    .collect::<Vec<String>>()
                                    .join(", ")
                            };

                            table.add_row(vec![
                                get_online_colored(&d.serial_number, &d.last_seen),
                                version,
                                os_arch,
                                release_str,
                                ip_str,
                                labels_str,
                            ]);
                        }
                        table.print();
                    }
                }
                GetResourceType::Commands {
                    selector,
                    limit,
                    json,
                } => {
                    let secrets = auth::get_secrets(&config)
                        .await
                        .with_context(|| "Error getting token")?
                        .with_context(|| "No Token found, please Login")?;

                    let api = SmithAPI::new(secrets, &config);

                    let devices = resolve_devices_from_selector(&api, &selector).await?;

                    if devices.is_empty() {
                        println!("No devices found");
                        return Ok(());
                    }

                    if json {
                        let mut all_commands = std::collections::HashMap::new();
                        for device in &devices {
                            let commands = api
                                .get_device_commands(device.id as u64, Some(limit))
                                .await?;
                            all_commands.insert(&device.serial_number, commands);
                        }
                        println!("{}", serde_json::to_string_pretty(&all_commands)?);
                        return Ok(());
                    }

                    for (idx, device) in devices.iter().enumerate() {
                        if idx > 0 {
                            println!();
                        }

                        println!(
                            "{} {} ({})",
                            "Device:".bold(),
                            device.serial_number.bright_cyan(),
                            device.id
                        );

                        let commands = api
                            .get_device_commands(device.id as u64, Some(limit))
                            .await?;

                        if commands.is_empty() {
                            println!("  {}", "No commands found".dimmed());
                            continue;
                        }

                        let mut table = TablePrint::new_with_headers(vec![
                            "ID",
                            "Issued At",
                            "Command",
                            "Status",
                        ]);

                        for cmd in commands {
                            let issued_at = cmd
                                .issued_at
                                .with_timezone(&chrono::Local)
                                .format("%Y-%m-%d %H:%M:%S")
                                .to_string();

                            let cmd_type = if let Some(cmd_str) = cmd.cmd_data.as_str() {
                                // Simple string command like "Restart", "Ping", etc.
                                cmd_str.to_string()
                            } else if let Some(cmd_obj) = cmd.cmd_data.as_object() {
                                // Object command - check for different command types
                                if cmd_obj.contains_key("Ping") {
                                    "Ping".to_string()
                                } else if cmd_obj.contains_key("Upgrade") {
                                    "Upgrade".to_string()
                                } else if cmd_obj.contains_key("Restart") {
                                    "Restart".to_string()
                                } else if let Some(freeform) = cmd_obj.get("FreeForm") {
                                    if let Some(cmd_text) = freeform.get("cmd") {
                                        cmd_text.as_str().unwrap_or("FreeForm").to_string()
                                    } else {
                                        "FreeForm".to_string()
                                    }
                                } else if cmd_obj.contains_key("TestNetwork") {
                                    "TestNetwork".to_string()
                                } else if cmd_obj.contains_key("OpenTunnel") {
                                    "OpenTunnel".to_string()
                                } else if cmd_obj.contains_key("CloseTunnel") {
                                    "CloseTunnel".to_string()
                                } else if cmd_obj.contains_key("UpdateNetwork") {
                                    "UpdateNetwork".to_string()
                                } else if cmd_obj.contains_key("UpdateVariables") {
                                    "UpdateVariables".to_string()
                                } else if cmd_obj.contains_key("DownloadOTA") {
                                    "DownloadOTA".to_string()
                                } else {
                                    cmd_obj
                                        .keys()
                                        .next()
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| "Unknown".to_string())
                                }
                            } else {
                                "Unknown".to_string()
                            };

                            let status = if cmd.response.is_some() {
                                "Completed".green().to_string()
                            } else if cmd.fetched {
                                "Fetched".blue().to_string()
                            } else if cmd.cancelled {
                                "Cancelled".red().to_string()
                            } else {
                                "Pending".yellow().to_string()
                            };

                            table.add_row(vec![
                                cmd.cmd_id.to_string(),
                                issued_at,
                                cmd_type,
                                status,
                            ]);
                        }

                        table.print();
                    }
                }
            },
            Commands::Restart { resource } => match resource {
                RestartResourceType::Device {
                    selector,
                    yes,
                    nowait,
                } => {
                    let secrets = auth::get_secrets(&config)
                        .await
                        .with_context(|| "Error getting token")?
                        .with_context(|| "No Token found, please Login")?;

                    let api = SmithAPI::new(secrets, &config);

                    // Check if no filters are specified - this should NEVER be allowed
                    let has_filters = !selector.ids.is_empty()
                        || !selector.labels.is_empty()
                        || selector.online
                        || selector.offline;

                    if !has_filters {
                        eprintln!(
                            "{}",
                            "Error: No device IDs or filters specified.".red().bold()
                        );
                        eprintln!(
                            "\n{}\n",
                            "You must specify which devices to restart.".yellow()
                        );
                        eprintln!("Examples:");
                        eprintln!(
                            "  {} {}",
                            "sm restart device".bold(),
                            "<device-id>...".bright_cyan()
                        );
                        eprintln!(
                            "  {} {}",
                            "sm restart device".bold(),
                            "ABC DEF --search".bright_cyan()
                        );
                        eprintln!(
                            "  {} {}",
                            "sm restart device".bold(),
                            "-l key=value".bright_cyan()
                        );
                        eprintln!(
                            "  {} {}",
                            "sm restart device".bold(),
                            "--online".bright_cyan()
                        );
                        return Err(anyhow::anyhow!("Aborted: No device selector specified"));
                    }

                    // Get target devices
                    let target_devices = resolve_devices_from_selector(&api, &selector).await?;

                    // Deduplicate devices
                    let mut seen_ids = HashSet::new();
                    let target_devices: Vec<_> = target_devices
                        .into_iter()
                        .filter(|device| seen_ids.insert(device.id))
                        .collect();

                    if target_devices.is_empty() {
                        println!("No devices found matching the specified filters.");
                        return Ok(());
                    }

                    // Show devices preview and confirm
                    let total_count = target_devices.len();
                    let preview_count = 10.min(total_count);

                    println!("{} {} device(s):", "Restarting".bold(), total_count);

                    for device in target_devices.iter().take(preview_count) {
                        println!("  - {}", device.serial_number);
                    }

                    if total_count > preview_count {
                        println!(
                            "  {} ({} more devices...)",
                            "...".dimmed(),
                            total_count - preview_count
                        );
                    }

                    if !yes {
                        print!("\n{} [y/N]: ", "Proceed?".bold());
                        io::Write::flush(&mut io::stdout())?;

                        let mut input = String::new();
                        io::stdin().read_line(&mut input)?;

                        if input.trim().to_lowercase() != "y" {
                            println!("Cancelled.");
                            return Ok(());
                        }
                    }

                    println!("\n{} restart commands...", "Sending".bold());

                    let mut command_ids = Vec::new();
                    for device in &target_devices {
                        let device_id = device.id;
                        let serial_number = &device.serial_number;

                        match api.send_restart_command(device_id as u64).await {
                            Ok((dev_id, cmd_id)) => {
                                command_ids.push((dev_id, cmd_id, serial_number.to_string()));
                                println!(
                                    "  {} [{}] - Restart queued: {}:{}",
                                    serial_number.bright_green(),
                                    dev_id,
                                    dev_id,
                                    cmd_id
                                );
                            }
                            Err(e) => {
                                println!(
                                    "  {} [{}] - Failed: {}",
                                    serial_number.red(),
                                    device_id,
                                    e
                                );
                            }
                        }
                    }

                    if nowait {
                        println!(
                            "\n{} Restart commands sent. Devices will reboot shortly.",
                            "Done!".bright_green()
                        );
                        return Ok(());
                    }

                    println!(
                        "\n{}",
                        "Note: Devices will go offline during restart. This is expected.".dimmed()
                    );
                }
                RestartResourceType::Service {
                    unit,
                    selector,
                    yes,
                    nowait,
                } => {
                    let secrets = auth::get_secrets(&config)
                        .await
                        .with_context(|| "Error getting token")?
                        .with_context(|| "No Token found, please Login")?;

                    let api = SmithAPI::new(secrets, &config);

                    // Check if no filters are specified
                    let has_filters = !selector.ids.is_empty()
                        || !selector.labels.is_empty()
                        || selector.online
                        || selector.offline;

                    if !has_filters {
                        eprintln!(
                            "{}",
                            "Error: No device IDs or filters specified.".red().bold()
                        );
                        eprintln!(
                            "\n{}\n",
                            "You must specify which devices to restart the service on.".yellow()
                        );
                        eprintln!("Examples:");
                        eprintln!(
                            "  {} {}",
                            "sm restart service nginx".bold(),
                            "<device-id>...".bright_cyan()
                        );
                        eprintln!(
                            "  {} {}",
                            "sm restart service nginx".bold(),
                            "web-server --search".bright_cyan()
                        );
                        return Err(anyhow::anyhow!("Aborted: No device selector specified"));
                    }

                    // Get target devices
                    let target_devices = resolve_devices_from_selector(&api, &selector).await?;

                    // Deduplicate devices
                    let mut seen_ids = HashSet::new();
                    let target_devices: Vec<_> = target_devices
                        .into_iter()
                        .filter(|device| seen_ids.insert(device.id))
                        .collect();

                    if target_devices.is_empty() {
                        println!("No devices found matching the specified filters.");
                        return Ok(());
                    }

                    // Show devices preview and confirm
                    let total_count = target_devices.len();
                    let preview_count = 10.min(total_count);

                    println!(
                        "{} {} on {} device(s):",
                        "Restarting service".bold(),
                        unit.cyan(),
                        total_count
                    );

                    for device in target_devices.iter().take(preview_count) {
                        println!("  - {}", device.serial_number);
                    }

                    if total_count > preview_count {
                        println!(
                            "  {} ({} more devices...)",
                            "...".dimmed(),
                            total_count - preview_count
                        );
                    }

                    if !yes {
                        print!("\n{} [y/N]: ", "Proceed?".bold());
                        io::Write::flush(&mut io::stdout())?;

                        let mut input = String::new();
                        io::stdin().read_line(&mut input)?;

                        if input.trim().to_lowercase() != "y" {
                            println!("Cancelled.");
                            return Ok(());
                        }
                    }

                    println!("\n{} service restart commands...", "Sending".bold());

                    let mut command_ids = Vec::new();
                    for device in &target_devices {
                        let device_id = device.id;
                        let serial_number = &device.serial_number;
                        let unit_clone = unit.clone();

                        match api
                            .send_service_restart_command(device_id as u64, unit_clone)
                            .await
                        {
                            Ok((dev_id, cmd_id)) => {
                                command_ids.push((dev_id, cmd_id, serial_number.to_string()));
                                println!(
                                    "  {} [{}] - Restart queued: {}:{}",
                                    serial_number.bright_green(),
                                    dev_id,
                                    dev_id,
                                    cmd_id
                                );
                            }
                            Err(e) => {
                                println!(
                                    "  {} [{}] - Failed: {}",
                                    serial_number.red(),
                                    device_id,
                                    e
                                );
                            }
                        }
                    }

                    if nowait {
                        println!(
                            "\n{} Service restart commands sent.",
                            "Done!".bright_green()
                        );
                        return Ok(());
                    }

                    println!(
                        "\n{} Use 'sm command <device_id>:<command_id>' to check results.",
                        "Note:".dimmed()
                    );
                }
            },
            Commands::Auth { command } => match command {
                cli::AuthCommands::Login { no_open } => {
                    auth::login(&config, !no_open).await?;
                }
                cli::AuthCommands::Logout => {
                    auth::logout()?;
                }
                cli::AuthCommands::Show => {
                    auth::show(&config).await?;
                }
            },
            Commands::Status { resource } => match resource {
                StatusResourceType::Device { selector, nowait } => {
                    let secrets = auth::get_secrets(&config)
                        .await
                        .with_context(|| "Error getting token")?
                        .with_context(|| "No Token found, please Login")?;

                    let api = SmithAPI::new(secrets, &config);

                    let device =
                        resolve_single_device_from_selector(&api, &selector, "status device")
                            .await?;
                    let serial_number = device.serial_number.clone();

                    let status = execute_device_command(
                        &api,
                        serial_number,
                        "Fetching smithd status",
                        nowait,
                        |id| api.send_smithd_status_command(id),
                    )
                    .await?;

                    if !status.is_empty() {
                        println!("{}", status);
                    }
                }
                StatusResourceType::Service {
                    unit,
                    selector,
                    nowait,
                } => {
                    let secrets = auth::get_secrets(&config)
                        .await
                        .with_context(|| "Error getting token")?
                        .with_context(|| "No Token found, please Login")?;

                    let api = SmithAPI::new(secrets, &config);

                    let device =
                        resolve_single_device_from_selector(&api, &selector, "status service")
                            .await?;
                    let serial_number = device.serial_number.clone();

                    let unit_clone = unit.clone();
                    let status = execute_device_command(
                        &api,
                        serial_number,
                        &format!("Checking status of {}", unit.bold()),
                        nowait,
                        |id| api.send_service_status_command(id, unit_clone),
                    )
                    .await?;

                    if !status.is_empty() {
                        println!("{}", status);
                    }
                }
            },
            Commands::Logs { selector, nowait } => {
                let secrets = auth::get_secrets(&config)
                    .await
                    .with_context(|| "Error getting token")?
                    .with_context(|| "No Token found, please Login")?;

                let api = SmithAPI::new(secrets, &config);

                let device = resolve_single_device_from_selector(&api, &selector, "logs").await?;
                let serial_number = device.serial_number.clone();

                let logs =
                    execute_device_command(&api, serial_number, "Fetching logs", nowait, |id| {
                        api.send_logs_command(id)
                    })
                    .await?;

                if !logs.is_empty() {
                    println!("{}", logs);
                }
            }
            Commands::TestNetwork { selector } => {
                let secrets = auth::get_secrets(&config)
                    .await
                    .with_context(|| "Error getting token")?
                    .with_context(|| "No Token found, please Login")?;

                let api = SmithAPI::new(secrets, &config);

                let devices = resolve_devices_from_selector(&api, &selector).await?;

                if devices.is_empty() {
                    println!("No devices found matching the selector");
                    return Ok(());
                }

                // Deduplicate devices
                let mut seen_ids = HashSet::new();
                let devices: Vec<_> = devices
                    .into_iter()
                    .filter(|device| seen_ids.insert(device.id))
                    .collect();

                println!(
                    "Sending network test command to {} device(s):",
                    devices.len()
                );

                for device in &devices {
                    match api.test_network(device.serial_number.clone()).await {
                        Ok(_) => {
                            println!("  {} {}", "✓".bright_green(), device.serial_number);
                        }
                        Err(e) => {
                            println!("  {} {} - Failed: {}", "✗".red(), device.serial_number, e);
                        }
                    }
                }

                println!(
                    "\n{}",
                    "Network test commands sent successfully!".bright_green()
                );
                println!("Each device will download a 20MB test file and report back the results.");
                println!("Check the dashboard to see the results.");
            }
            Commands::Command { ids } => {
                let secrets = auth::get_secrets(&config)
                    .await
                    .with_context(|| "Error getting token")?
                    .with_context(|| "No Token found, please Login")?;

                let api = SmithAPI::new(secrets, &config);

                for id_str in &ids {
                    let parts: Vec<&str> = id_str.split(':').collect();
                    if parts.len() != 2 {
                        return Err(anyhow::anyhow!(
                            "Invalid command ID format '{}'. Expected format: device_id:command_id",
                            id_str
                        ));
                    }

                    let device_id: u64 = parts[0].parse().with_context(|| {
                        format!("Invalid device_id in '{}': must be a number", id_str)
                    })?;

                    let command_id: u64 = parts[1].parse().with_context(|| {
                        format!("Invalid command_id in '{}': must be a number", id_str)
                    })?;

                    let command = api.get_device_command(device_id, command_id).await?;

                    println!("Command ID: {}", id_str);
                    println!("Fetched: {}", command.fetched);

                    if let Some(response) = command.response {
                        if let Some(stdout) = response["FreeForm"]["stdout"].as_str() {
                            println!("Output:\n{}", stdout);
                        } else {
                            println!("Status: Completed (no output)");
                        }
                    } else {
                        println!("Status: Pending");
                    }

                    if ids.len() > 1 {
                        println!("---");
                    }
                }
            }
            Commands::Distributions { command } => match command {
                DistroCommands::Ls { json } => {
                    let secrets = auth::get_secrets(&config)
                        .await
                        .with_context(|| "Error getting token")?
                        .with_context(|| "No Token found, please Login")?;

                    let api = SmithAPI::new(secrets, &config);

                    let distros = api.get_distributions().await?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&distros)?);
                        return Ok(());
                    }
                    let mut table =
                        TablePrint::new_with_headers(vec!["Name (arch)", "Description"]);
                    for d in distros {
                        table.add_row(vec![
                            format!("{} ({})", d.name, get_colored_arch(&d.architecture)),
                            d.description.to_owned().unwrap_or_default(),
                        ]);
                    }
                    table.print();
                }
                DistroCommands::Releases => {}
            },
            cli::Commands::Tunnel {
                serial_number,
                overview_debug,
            } => {
                let secrets = auth::get_secrets(&config)
                    .await
                    .with_context(|| "Error getting token")?
                    .with_context(|| "No Token found, please Login")?;

                let pub_key = config.get_identity_pub_key().await?;

                let api = SmithAPI::new(secrets, &config);

                let device = api.get_device(serial_number.clone()).await?;
                if device.last_seen.is_none_or(|last_seen| {
                    chrono::Utc::now()
                        .signed_duration_since(last_seen)
                        .num_minutes()
                        >= 5
                }) {
                    let mut last_ping = "never".to_string();
                    if let Some(last_seen) = device.last_seen {
                        last_ping =
                            HumanTime::from(last_seen.with_timezone(&chrono::Utc)).to_string();
                    }
                    bail!("This device is offline. Last ping was {}", last_ping);
                }

                println!(
                    "Creating tunnel for device [{}] {} {:?}",
                    device.id,
                    &serial_number.bold(),
                    overview_debug
                );

                let m = MultiProgress::new();
                let pb2 = m.add(ProgressBar::new_spinner());
                pb2.enable_steady_tick(Duration::from_millis(50));
                pb2.set_style(
                    ProgressStyle::with_template("{spinner:.blue} {msg}")
                        .unwrap()
                        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
                );
                pb2.set_message("Sending request to smith");

                let (tx, rx) = oneshot::channel();
                let username = config.current_tunnel_username();
                let username_clone = username.clone();
                let tunnel_openning_handler = tokio::spawn(async move {
                    api.open_tunnel(device.id as u64, pub_key, username_clone)
                        .await
                        .unwrap();
                    pb2.set_message("Request sent to smith 💻");

                    let port;
                    loop {
                        let response = api.get_last_command(device.id as u64).await.unwrap();

                        if response.fetched {
                            pb2.set_message("Command fetched by device 👍");
                        }

                        if let Some(response) = response.response {
                            port = response["OpenTunnel"]["port_server"].as_u64().unwrap();

                            tx.send(port).unwrap();
                            break;
                        }

                        thread::sleep(Duration::from_secs(1));
                    }

                    pb2.finish_with_message(format!("{} {}", "Port:".bold(), port));
                });

                let port = rx.await.unwrap() as u16;

                println!("Opening tunnel to port {}", port);

                tunnel_openning_handler.await.unwrap();

                // Give the server a moment to set up the SSH tunnel
                println!("Waiting for tunnel setup...");

                // Wait for server-side setup to complete
                tokio::time::sleep(Duration::from_secs(10)).await;

                let tunnel_server = config.current_tunnel_server();
                let mut ssh = Session::connect(
                    config.get_identity_file(),
                    username,
                    None,
                    (tunnel_server, port),
                )
                .await?;
                println!("Connected");
                // We're using `termion` to put the terminal into raw mode, so that we can
                // display the output of interactive applications correctly
                let _raw_term = io::stdout().into_raw_mode()?;
                ssh.call().await.with_context(|| "skill issues")?;

                println!("Exitcode: {:?}", ());
                ssh.close().await?;
                return Ok(());
            }
            Commands::Releases { command } => {
                command.handle(config).await?;
            }
            Commands::Completion { shell } => {
                let mut cmd = Cli::command();
                let name = env!("CARGO_BIN_NAME");
                generate(shell, &mut cmd, name, &mut io::stdout());
                return Ok(());
            }
            Commands::AgentHelp => {
                print_markdown_help();
                return Ok(());
            }
            Commands::Run {
                selector,
                yes,
                wait,
                command,
            } => {
                let secrets = auth::get_secrets(&config)
                    .await
                    .with_context(|| "Error getting token")?
                    .with_context(|| "No Token found, please Login")?;

                let api = SmithAPI::new(secrets, &config);

                // Determine command source: args after -- OR stdin
                let cmd_from_stdin;
                let cmd_string = if !command.is_empty() {
                    // Command provided via args after --, validate that -- was actually present
                    if !std::env::args().any(|arg| arg == "--") {
                        bail!(
                            "error: command must be provided either:\n  - as arguments after '--' (e.g., sm run 1234 -- echo hello)\n  - via stdin (e.g., echo 'sleep 1' | sm run 1234)"
                        );
                    }
                    cmd_from_stdin = false;
                    command.join(" ")
                } else {
                    // No args after --, try to read from stdin
                    // First check if stdin is a TTY to avoid blocking
                    if io::stdin().is_terminal() {
                        bail!(
                            "error: command must be provided either:\n  - as arguments after '--' (e.g., sm run 1234 -- echo hello)\n  - via stdin (e.g., echo 'sleep 1' | sm run 1234)"
                        );
                    }

                    let mut buffer = String::new();
                    let bytes_read = io::stdin()
                        .read_to_string(&mut buffer)
                        .context("Failed to read command from stdin")?;

                    let trimmed = buffer.trim();

                    if bytes_read == 0 || trimmed.is_empty() {
                        // No stdin data available
                        bail!(
                            "error: command must be provided either:\n  - as arguments after '--' (e.g., sm run 1234 -- echo hello)\n  - via stdin (e.g., echo 'sleep 1' | sm run 1234)"
                        );
                    }

                    cmd_from_stdin = true;
                    trimmed.to_string()
                };

                let target_devices = resolve_target_devices(
                    &api,
                    selector.ids,
                    selector.labels,
                    selector.online,
                    selector.offline,
                    selector.search,
                )
                .await?;

                // Deduplicate devices by ID to prevent duplicate command execution
                let mut seen_ids = HashSet::new();
                let target_devices: Vec<_> = target_devices
                    .into_iter()
                    .filter(|device| seen_ids.insert(device.id))
                    .collect();

                if target_devices.is_empty() {
                    println!("No devices found matching the specified filters.");
                    return Ok(());
                }

                // Show devices preview and confirm
                let total_count = target_devices.len();
                let preview_count = 10.min(total_count);

                println!(
                    "Running command '{}' on {} device(s):",
                    cmd_string.bold(),
                    total_count
                );

                for device in target_devices.iter().take(preview_count) {
                    println!("  - {}", device.serial_number);
                }

                if total_count > preview_count {
                    println!(
                        "  {} ({} more devices...)",
                        "...".dimmed(),
                        total_count - preview_count
                    );
                }

                if total_count > 1 && !yes && cmd_from_stdin {
                    bail!(
                        "error: cannot prompt for confirmation when command is piped via stdin.\nUse -y/--yes to proceed with multiple devices."
                    );
                }

                if total_count > 1 && !yes {
                    print!("\n{} [y/N]: ", "Proceed?".bold());
                    io::Write::flush(&mut io::stdout())?;

                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;

                    if input.trim().to_lowercase() != "y" {
                        println!("Cancelled.");
                        return Ok(());
                    }
                }

                println!();

                let mut command_ids = Vec::new();

                for device in &target_devices {
                    let device_id = device.id;
                    let serial_number = &device.serial_number;

                    match api
                        .send_custom_command(device_id as u64, cmd_string.clone())
                        .await
                    {
                        Ok((dev_id, cmd_id)) => {
                            command_ids.push((dev_id, cmd_id, serial_number.to_string()));
                            println!(
                                "  {} [{}] - Command queued: {}:{}",
                                serial_number.bright_green(),
                                dev_id,
                                dev_id,
                                cmd_id
                            );
                        }
                        Err(e) => {
                            println!("  {} [{}] - Failed: {}", serial_number.red(), device_id, e);
                        }
                    }
                }

                if !wait {
                    println!(
                        "\n{} Commands sent. Use 'sm command <device_id>:<command_id>' to check status.",
                        "Note:".bold()
                    );
                    return Ok(());
                }

                println!("\n{} for results...", "Waiting".bold());

                let m = MultiProgress::new();
                let mut progress_bars = Vec::new();

                for (device_id, command_id, serial_number) in &command_ids {
                    let pb = m.add(ProgressBar::new_spinner());
                    pb.enable_steady_tick(Duration::from_millis(50));
                    pb.set_style(
                        ProgressStyle::with_template("{spinner:.blue} {msg}")
                            .unwrap()
                            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
                    );
                    pb.set_message(format!(
                        "{} [{}:{}] Waiting...",
                        serial_number, device_id, command_id
                    ));
                    progress_bars.push((pb, *device_id, *command_id, serial_number.clone()));
                }

                let mut completed = vec![false; command_ids.len()];
                let mut results: Vec<(String, String)> =
                    vec![(String::new(), String::new()); command_ids.len()];

                while !completed.iter().all(|&c| c) {
                    for (idx, (pb, device_id, command_id, serial_number)) in
                        progress_bars.iter().enumerate()
                    {
                        if completed[idx] {
                            continue;
                        }

                        match api.get_device_command(*device_id, *command_id).await {
                            Ok(response) => {
                                if response.fetched && !completed[idx] {
                                    pb.set_message(format!(
                                        "{} [{}:{}] Fetched by device",
                                        serial_number, device_id, command_id
                                    ));
                                }

                                if let Some(response) = response.response {
                                    let output = if let Some(stdout) =
                                        response["FreeForm"]["stdout"].as_str()
                                    {
                                        stdout.to_string()
                                    } else {
                                        String::from("(no output)")
                                    };
                                    results[idx] = (serial_number.clone(), output);
                                    pb.finish_with_message(format!(
                                        "{} [{}:{}] Completed ✓",
                                        serial_number.bright_green(),
                                        device_id,
                                        command_id
                                    ));
                                    completed[idx] = true;
                                }
                            }
                            Err(e) => {
                                results[idx] = (serial_number.clone(), format!("Error: {}", e));
                                pb.finish_with_message(format!(
                                    "{} [{}:{}] Failed ✗",
                                    serial_number.red(),
                                    device_id,
                                    command_id
                                ));
                                completed[idx] = true;
                            }
                        }
                    }

                    if !completed.iter().all(|&c| c) {
                        thread::sleep(Duration::from_secs(1));
                    }
                }

                println!("\n{}", "Results:".bold());
                for (serial_number, output) in results {
                    println!("\n{} {}:", "Device:".bold(), serial_number.bright_cyan());
                    println!("{}", output);
                    println!("{}", "---".dimmed());
                }
            }
            Commands::Label {
                selector,
                devices: device_filters,
                set_labels,
            } => {
                let secrets = auth::get_secrets(&config)
                    .await
                    .with_context(|| "Error getting token")?
                    .with_context(|| "No Token found, please Login")?;

                let api = SmithAPI::new(secrets, &config);

                let target_devices = resolve_target_devices(
                    &api,
                    device_filters,
                    selector.labels,
                    selector.online,
                    selector.offline,
                    selector.search,
                )
                .await?;

                // Deduplicate devices by ID to prevent duplicate label operations
                let mut seen_ids = HashSet::new();
                let target_devices: Vec<_> = target_devices
                    .into_iter()
                    .filter(|device| seen_ids.insert(device.id))
                    .collect();

                if target_devices.is_empty() {
                    println!("No devices found matching the specified filters.");
                    return Ok(());
                }

                let new_labels_map = parse_label_filters(set_labels)?.unwrap_or_default();

                println!("Setting labels on {} device(s):", target_devices.len());
                for device in &target_devices {
                    println!("  - {}", device.serial_number);
                }
                println!("\nLabels to set:");
                for (key, value) in &new_labels_map {
                    println!("  {}={}", key, value);
                }

                print!("\nProceed? [y/N]: ");
                io::Write::flush(&mut io::stdout())?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;

                if input.trim().to_lowercase() != "y" {
                    println!("Cancelled.");
                    return Ok(());
                }

                println!("\n{} labels to devices...", "Applying".bold());

                for device in &target_devices {
                    let device_id = device.id;
                    let serial_number = &device.serial_number;

                    let mut device_labels = device
                        .labels
                        .iter()
                        .map(|(k, v)| (k.clone(), v.to_string()))
                        .collect::<std::collections::HashMap<String, String>>();

                    for (key, value) in &new_labels_map {
                        device_labels.insert(key.clone(), value.clone());
                    }

                    match api
                        .update_device_labels(device_id as u64, device_labels)
                        .await
                    {
                        Ok(()) => {
                            println!(
                                "  {} [{}] - Labels updated",
                                serial_number.bright_green(),
                                device_id
                            );
                        }
                        Err(e) => {
                            println!("  {} [{}] - Failed: {}", serial_number.red(), device_id, e);
                        }
                    }
                }

                println!("\n{}", "Done!".bright_green());
            }
        },
        None => {
            Cli::command().print_help()?;
        }
    }

    Ok(())
}

async fn execute_device_command<F, Fut>(
    api: &SmithAPI,
    serial_number: String,
    command_name: &str,
    nowait: bool,
    send_command: F,
) -> anyhow::Result<String>
where
    F: FnOnce(u64) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<(u64, u64)>>,
{
    let devices = api
        .get_devices(DeviceFilter {
            serial_number: Some(serial_number.clone()),
            ..Default::default()
        })
        .await?;
    let id = devices
        .first()
        .ok_or_else(|| anyhow::anyhow!("Device not found"))?
        .id;

    println!(
        "{} for device [{}] {}",
        command_name,
        id,
        &serial_number.bold()
    );

    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(50));
    pb.set_style(
        ProgressStyle::with_template("{spinner:.blue} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.set_message("Sending request to device");

    let (device_id, command_id) = send_command(id as u64).await?;

    if nowait {
        pb.finish_and_clear();
        println!("Command queued: {}:{}", device_id, command_id);
        println!(
            "Note: Commands typically take at least 30 seconds to complete. Use 'sm command {}:{}' to check status.",
            device_id, command_id
        );
        return Ok(String::new());
    }

    pb.set_message("Request sent, waiting for device");

    let result;
    loop {
        let response = api.get_last_command(id as u64).await?;

        if response.fetched {
            pb.set_message("Command fetched by device");
        }

        if let Some(response) = response.response {
            result = response["FreeForm"]["stdout"]
                .as_str()
                .with_context(|| "Failed to get output from response")?
                .to_string();
            break;
        }

        thread::sleep(Duration::from_secs(1));
    }

    pb.finish_and_clear();
    Ok(result)
}

fn get_colored_arch(arch: &str) -> String {
    match arch.to_lowercase().as_str() {
        "amd64" => arch.bright_blue().to_string(),
        "x86_64" => arch.bright_blue().to_string(),
        "arm64" => arch.bright_green().to_string(),
        "aarch64" => arch.bright_green().to_string(),
        "i386" => arch.yellow().to_string(),
        "x86" => arch.yellow().to_string(),
        "armhf" => arch.magenta().to_string(),
        "ppc64le" => arch.cyan().to_string(),
        "s390x" => arch.red().to_string(),
        "riscv64" => arch.bright_purple().to_string(),
        _ => arch.white().to_string(),
    }
}
