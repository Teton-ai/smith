mod api;
mod auth;
mod cli;
mod config;
mod print;
mod schema;
mod tunnel;

use crate::cli::{Cli, Commands, DevicesCommands, DistroCommands};
use crate::print::TablePrint;
use anyhow::Context;
use api::SmithAPI;
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde_json::Value;
use std::{io, sync::mpsc, thread, time::Duration};
use termion::raw::IntoRawMode;
use tokio::sync::oneshot;
use tunnel::Session;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

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
            },
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
                let tunnel_openning_handler = tokio::spawn(async move {
                    api.open_tunnel(id, pub_key, String::from("TODO"))
                        .await
                        .unwrap();
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

                let mut ssh = Session::connect(
                    config.get_identity_file(),
                    "TODO",
                    None,
                    ("bore.teton.ai", port),
                )
                .await?;
                println!("Connected");
                let code = {
                    // We're using `termion` to put the terminal into raw mode, so that we can
                    // display the output of interactive applications correctly
                    let _raw_term = std::io::stdout().into_raw_mode()?;
                    ssh.call("bash").await.with_context(|| "skill issues")?;
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
        },
        None => {
            println!("No command provided");
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
