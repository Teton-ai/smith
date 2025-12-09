use crate::{api::SmithAPI, auth, execute_device_command, print::TablePrint};
use anyhow::Context as _;
use chrono::{DateTime, Utc};
use clap::{Args, Subcommand};
use colored::Colorize;
use models::device::DeviceFilter;

#[derive(Args, Debug)]
pub struct DevicesGet {
    id_or_serial_number: Option<String>,
    #[arg(short, long, default_value = "false")]
    json: bool,
    /// Filter by labels (format: key=value). Can be used multiple times.
    #[arg(short, long = "label", value_name = "KEY=VALUE")]
    labels: Vec<String>,
    /// Show only online devices (last seen < 5 minutes)
    #[arg(long, conflicts_with = "offline")]
    online: bool,
    /// Show only offline devices (last seen >= 5 minutes)
    #[arg(long, conflicts_with = "online")]
    offline: bool,
}

#[derive(Subcommand, Debug)]
pub enum DevicesCommands {
    /// List the current distributions
    Get(DevicesGet),
    /// Test network speed for a device
    TestNetwork {
        /// Device serial number or ID
        device: String,
    },
    /// Get logs for a specific device
    Logs {
        /// Device serial number
        serial_number: String,
        /// Don't wait for result, just queue the command and return immediately (faster, recommended for agents - use 'sm command <id>' to check results later)
        #[arg(long, default_value = "false")]
        nowait: bool,
    },
}

impl DevicesCommands {
    pub async fn handle(self, config: crate::config::Config) -> anyhow::Result<()> {
        match self {
            DevicesCommands::Get(get) => handle_devices_get(get, config).await?,
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
                println!("The device will download a 20MB test file and report back the results.");
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

                let logs =
                    execute_device_command(&api, serial_number, "Fetching logs", nowait, |id| {
                        api.send_logs_command(id)
                    })
                    .await?;

                if !logs.is_empty() {
                    println!("{}", logs);
                }
            }
        };

        Ok(())
    }
}

fn get_online_colored(serial_number: &str, last_seen: &Option<DateTime<Utc>>) -> String {
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

async fn handle_devices_get(get: DevicesGet, config: crate::config::Config) -> anyhow::Result<()> {
    let DevicesGet {
        id_or_serial_number,
        json,
        labels,
        online,
        offline,
    } = get;

    let secrets = auth::get_secrets(&config)
        .await
        .with_context(|| "Error getting token")?
        .with_context(|| "No Token found, please Login")?;

    let api = SmithAPI::new(secrets, &config);

    let online_filter = if online {
        Some(true)
    } else if offline {
        Some(false)
    } else {
        None
    };

    let devices = match id_or_serial_number {
        Some(id_or_serial_number) => {
            let device = api.get_device(id_or_serial_number).await?;
            vec![device]
        }
        None => {
            api.get_devices(DeviceFilter {
                labels,
                online: online_filter,
                ..Default::default()
            })
            .await?
        }
    };

    if json {
        println!("{}", serde_json::to_string(&devices).unwrap());
        return Ok(());
    }

    let mut table = TablePrint::new_with_headers(vec!["Device", "Labels", "Version"]);
    for d in devices {
        let labels_str = d
            .labels
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<String>>()
            .join(", ");

        table.add_row(vec![
            get_online_colored(&d.serial_number, &d.last_seen),
            labels_str,
            d.system_info
                .as_ref()
                .map(|info| {
                    info["smith"]["version"]
                        .as_str()
                        .unwrap_or("")
                        .parse()
                        .unwrap()
                })
                .unwrap_or_default(),
        ]);
    }
    table.print();

    Ok(())
}
