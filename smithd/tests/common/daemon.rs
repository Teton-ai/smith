use anyhow::{Context, Result};
use std::time::Duration;
use tokio::time::{sleep, timeout};

pub struct DaemonWaiter {
    timeout_duration: Duration,
}

impl DaemonWaiter {
    pub fn new(timeout_duration: Duration) -> Self {
        Self { timeout_duration }
    }

    pub async fn wait_for_registration(&self, api_url: &str, serial: &str) -> Result<()> {
        timeout(self.timeout_duration, async {
            loop {
                // Poll API to check if device registered
                let client = reqwest::Client::new();
                let response = client
                    .get(&format!("{}/devices/{}", api_url, serial))
                    .send()
                    .await;

                if let Ok(resp) = response {
                    if resp.status().is_success() {
                        return Ok(());
                    }
                }

                sleep(Duration::from_millis(500)).await;
            }
        })
        .await
        .context("Timeout waiting for device registration")?
    }

    pub async fn wait_for_ping(&self, api_url: &str, serial: &str) -> Result<PingInfo> {
        timeout(self.timeout_duration, async {
            loop {
                // Check if device has sent at least one ping
                let client = reqwest::Client::new();
                let response = client
                    .get(&format!("{}/devices/{}/status", api_url, serial))
                    .send()
                    .await;

                if let Ok(resp) = response {
                    if resp.status().is_success() {
                        if let Ok(ping_info) = resp.json::<PingInfo>().await {
                            return Ok(ping_info);
                        }
                    }
                }

                sleep(Duration::from_millis(500)).await;
            }
        })
        .await
        .context("Timeout waiting for device ping")?
    }
}

impl Default for DaemonWaiter {
    fn default() -> Self {
        Self::new(Duration::from_secs(60))
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PingInfo {
    pub timestamp: i64,
    pub release_id: Option<i32>,
    pub responses: Vec<CommandResponse>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CommandResponse {
    pub id: i32,
    pub status: i32,
}
