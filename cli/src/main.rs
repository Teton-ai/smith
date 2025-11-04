mod api;
mod auth;
mod cli;
mod config;
mod print;
mod schema;
mod tunnel;

use crate::cli::{Cli, Commands, DevicesCommands, DistroCommands, ServiceCommands};
use crate::print::TablePrint;
use anyhow::Context;
use api::SmithAPI;
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde_json::Value;
use std::{io, thread, time::Duration};
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
    println!("### `sm devices ls [--json]`");
    println!("List all devices. Use `--json` flag to output in JSON format.\n");
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

    let should_check = if last_check_file.exists() {
        if let Ok(contents) = std::fs::read_to_string(&last_check_file) {
            if let Ok(timestamp) = contents.trim().parse::<i64>() {
                let last_check = chrono::DateTime::from_timestamp(timestamp, 0);
                let now = chrono::Utc::now();

                if let Some(last_check) = last_check {
                    let duration = now.signed_duration_since(last_check);
                    duration.num_hours() >= 24
                } else {
                    true
                }
            } else {
                true
            }
        } else {
            true
        }
    } else {
        true
    };

    if should_check {
        let current_version = self_update::cargo_crate_version!();
        let updater = self_update::backends::github::Update::configure()
            .repo_owner("teton-ai")
            .repo_name("smith")
            .bin_name("sm")
            .show_download_progress(false)
            .current_version(current_version)
            .build()?;

        if let Ok(status) = updater.get_latest_release() {
            if status.version != current_version {
                println!(
                    "A new version {} is available! Run 'sm update' to update.",
                    status.version
                );
            }
        }

        let now = chrono::Utc::now().timestamp();
        std::fs::write(&last_check_file, now.to_string())?;
    }

    Ok(())
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

    println!("{}", config);

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
            Commands::Devices { command } => match command {
                DevicesCommands::Ls { json } => {
                    let secrets = auth::get_secrets(&config)
                        .await
                        .with_context(|| "Error getting token")?
                        .with_context(|| "No Token found, please Login")?;

                    let api = SmithAPI::new(secrets, &config);

                    let devices = api.get_devices(None).await?;
                    if json {
                        println!("{}", devices);
                        return Ok(());
                    }
                    let parsed_devices: Vec<Value> = serde_json::from_str(&devices)
                        .with_context(|| "Failed to parse devices JSON")?;
                    let rows: Vec<Vec<String>> = parsed_devices
                        .iter()
                        .map(|d| {
                            vec![
                                get_online_colored(
                                    d["serial_number"].as_str().unwrap_or(""),
                                    d["last_seen"].as_str().unwrap_or(""),
                                ),
                                d["system_info"]["smith"]["version"]
                                    .as_str()
                                    .unwrap_or("")
                                    .parse()
                                    .unwrap(),
                            ]
                        })
                        .collect();
                    TablePrint {
                        headers: vec![
                            "Serial Number (online)".to_string(),
                            "Daemon Version".to_string(),
                        ],
                        rows,
                    }
                    .print();
                }
                DevicesCommands::TestNetwork { device } => {
                    let secrets = auth::get_secrets(&config)
                        .await
                        .with_context(|| "Error getting token")?
                        .with_context(|| "No Token found, please Login")?;

                    let api = SmithAPI::new(secrets, &config);

                    println!("Sending network test command to device: {}", device.bold());
                    api.test_network(device).await?;
                    println!(
                        "{}",
                        "Network test command sent successfully!".bright_green()
                    );
                    println!(
                        "The device will download a 20MB test file and report back the results."
                    );
                    println!("Check the dashboard to see the results.");
                }
                DevicesCommands::Logs {
                    serial_number,
                    nowait,
                } => {
                    let secrets = auth::get_secrets(&config)
                        .await
                        .with_context(|| "Error getting token")?
                        .with_context(|| "No Token found, please Login")?;

                    let api = SmithAPI::new(secrets, &config);

                    let devices = api.get_devices(Some(serial_number.clone())).await?;

                    let parsed: Value = serde_json::from_str(&devices)?;

                    let id = parsed[0]["id"]
                        .as_u64()
                        .with_context(|| "Device not found")?;

                    println!(
                        "Fetching logs for device [{}] {}",
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

                    let (device_id, command_id) = api.send_logs_command(id).await?;

                    if nowait {
                        pb.finish_and_clear();
                        println!("Command queued: {}:{}", device_id, command_id);
                        println!(
                            "Note: Commands typically take at least 30 seconds to complete. Use 'sm command {}:{}' to check status.",
                            device_id, command_id
                        );
                        return Ok(());
                    }

                    pb.set_message("Request sent, waiting for device");

                    let logs;
                    loop {
                        let response = api.get_last_command(id).await?;

                        if response["fetched"].is_boolean()
                            && response["fetched"].as_bool().unwrap_or(false)
                        {
                            pb.set_message("Command fetched by device");
                        }

                        if response["response"].is_object() {
                            logs = response["response"]["FreeForm"]["stdout"]
                                .as_str()
                                .with_context(|| "Failed to get logs from response")?
                                .to_string();
                            break;
                        }

                        thread::sleep(Duration::from_secs(1));
                    }

                    pb.finish_and_clear();
                    println!("{}", logs);
                }
            },
            Commands::Service { command } => match command {
                ServiceCommands::Status {
                    unit,
                    serial_number,
                    nowait,
                } => {
                    let secrets = auth::get_secrets(&config)
                        .await
                        .with_context(|| "Error getting token")?
                        .with_context(|| "No Token found, please Login")?;

                    let api = SmithAPI::new(secrets, &config);

                    let devices = api.get_devices(Some(serial_number.clone())).await?;

                    let parsed: Value = serde_json::from_str(&devices)?;

                    let id = parsed[0]["id"]
                        .as_u64()
                        .with_context(|| "Device not found")?;

                    println!(
                        "Checking status of {} on device [{}] {}",
                        unit.bold(),
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

                    let (device_id, command_id) = api.send_service_status_command(id, unit).await?;

                    if nowait {
                        pb.finish_and_clear();
                        println!("Command queued: {}:{}", device_id, command_id);
                        println!(
                            "Note: Commands typically take at least 30 seconds to complete. Use 'sm command {}:{}' to check status.",
                            device_id, command_id
                        );
                        return Ok(());
                    }

                    pb.set_message("Request sent, waiting for device");

                    let status;
                    loop {
                        let response = api.get_last_command(id).await?;

                        if response["fetched"].is_boolean()
                            && response["fetched"].as_bool().unwrap_or(false)
                        {
                            pb.set_message("Command fetched by device");
                        }

                        if response["response"].is_object() {
                            status = response["response"]["FreeForm"]["stdout"]
                                .as_str()
                                .with_context(|| "Failed to get status from response")?
                                .to_string();
                            break;
                        }

                        thread::sleep(Duration::from_secs(1));
                    }

                    pb.finish_and_clear();
                    println!("{}", status);
                }
            },
            Commands::Status {
                serial_number,
                nowait,
            } => {
                let secrets = auth::get_secrets(&config)
                    .await
                    .with_context(|| "Error getting token")?
                    .with_context(|| "No Token found, please Login")?;

                let api = SmithAPI::new(secrets, &config);

                let devices = api.get_devices(Some(serial_number.clone())).await?;

                let parsed: Value = serde_json::from_str(&devices)?;

                let id = parsed[0]["id"]
                    .as_u64()
                    .with_context(|| "Device not found")?;

                println!(
                    "Fetching smithd status for device [{}] {}",
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

                let (device_id, command_id) = api.send_smithd_status_command(id).await?;

                if nowait {
                    pb.finish_and_clear();
                    println!("Command queued: {}:{}", device_id, command_id);
                    println!(
                        "Note: Commands typically take at least 30 seconds to complete. Use 'sm command {}:{}' to check status.",
                        device_id, command_id
                    );
                    return Ok(());
                }

                pb.set_message("Request sent, waiting for device");

                let status;
                loop {
                    let response = api.get_last_command(id).await?;

                    if response["fetched"].is_boolean()
                        && response["fetched"].as_bool().unwrap_or(false)
                    {
                        pb.set_message("Command fetched by device");
                    }

                    if response["response"].is_object() {
                        status = response["response"]["FreeForm"]["stdout"]
                            .as_str()
                            .with_context(|| "Failed to get status from response")?
                            .to_string();
                        break;
                    }

                    thread::sleep(Duration::from_secs(1));
                }

                pb.finish_and_clear();
                println!("{}", status);
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
                    println!("Fetched: {}", command["fetched"].as_bool().unwrap_or(false));

                    if command["response"].is_object() {
                        if let Some(stdout) = command["response"]["FreeForm"]["stdout"].as_str() {
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
                        println!("{}", distros);
                        return Ok(());
                    }
                    let parsed_distros: Vec<Value> = serde_json::from_str(&distros)
                        .with_context(|| "Failed to parse distributions JSON")?;
                    let rows: Vec<Vec<String>> = parsed_distros
                        .iter()
                        .map(|d| {
                            vec![
                                format!(
                                    "{} ({})",
                                    d["name"].as_str().unwrap_or(""),
                                    get_colored_arch(d["architecture"].as_str().unwrap_or(""))
                                ),
                                d["description"].as_str().unwrap_or("").to_string(),
                            ]
                        })
                        .collect();
                    TablePrint {
                        headers: vec!["Name (arch)".to_string(), "Description".to_string()],
                        rows,
                    }
                    .print();
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

                let devices = api.get_devices(Some(serial_number.clone())).await?;

                let parsed: Value = serde_json::from_str(&devices)?;

                let id = parsed[0]["id"].as_u64().unwrap();

                println!(
                    "Creating tunnel for device [{}] {} {:?}",
                    id,
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
                    api.open_tunnel(id, pub_key, username_clone).await.unwrap();
                    pb2.set_message("Request sent to smith 💻");

                    let port;
                    loop {
                        let response = api.get_last_command(id).await.unwrap();

                        if response["fetched"].is_boolean()
                            && response["fetched"].as_bool().unwrap()
                        {
                            pb2.set_message("Command fetched by device 👍");
                        }

                        if response["response"].is_object() {
                            port = response["response"]["OpenTunnel"]["port_server"]
                                .as_u64()
                                .unwrap();

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
                let mut ssh = Session::connect(
                    config.get_identity_file(),
                    username,
                    None,
                    ("bore.teton.ai", port),
                )
                .await?;
                println!("Connected");
                let code = {
                    // We're using `termion` to put the terminal into raw mode, so that we can
                    // display the output of interactive applications correctly
                    let _raw_term = io::stdout().into_raw_mode()?;
                    ssh.call().await.with_context(|| "skill issues")?;
                };

                println!("Exitcode: {:?}", code);
                ssh.close().await?;
                return Ok(());
            }
            Commands::Release {
                release_number,
                deploy,
            } => {
                let secrets = auth::get_secrets(&config)
                    .await
                    .with_context(|| "Error getting token")?
                    .with_context(|| "No Token found, please Login")?;

                let api = SmithAPI::new(secrets, &config);
                if deploy {
                    // Start the deployment
                    api.deploy_release(release_number.clone()).await?;

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
                        if let Some(status) = deployment.get("status").and_then(|s| s.as_str()) {
                            println!("Current status: {}", status);

                            if status == "Done" {
                                println!("Deployment completed successfully!");
                                return Ok(());
                            }

                            // If status is "failed" or any other terminal state, we can exit early
                            if status == "Failed" {
                                return Err(anyhow::anyhow!("Deployment failed"));
                            }
                        }

                        // Wait before the next check
                        println!(
                            "Waiting for devices to update... (elapsed: {:?})",
                            start_time.elapsed()
                        );
                        tokio::time::sleep(check_interval).await;
                    }
                } else {
                    let value = api.get_release_info(release_number).await?;
                    println!("{}", value);
                    return Ok(());
                }
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
        },
        None => {
            Cli::command().print_help()?;
        }
    }

    Ok(())
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

fn get_online_colored(serial_number: &str, last_seen: &str) -> String {
    let now = chrono::Utc::now();

    match chrono::DateTime::parse_from_rfc3339(last_seen) {
        Ok(parsed_time) => {
            let duration = now.signed_duration_since(parsed_time.with_timezone(&chrono::Utc));

            if duration.num_minutes() < 5 {
                serial_number.bright_green().to_string()
            } else {
                format!("{} (last seen {})", serial_number, last_seen)
                    .red()
                    .to_string()
            }
        }
        Err(_) => format!("{} (Unknown)", serial_number).yellow().to_string(),
    }
}
