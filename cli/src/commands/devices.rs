use crate::{api::SmithAPI, auth, execute_device_command};
use anyhow::Context as _;
use chrono::{DateTime, Utc};
use clap::Subcommand;
use colored::Colorize;

#[derive(Subcommand, Debug)]
pub enum DevicesCommands {
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
