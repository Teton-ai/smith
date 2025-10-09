use crate::magic::structure::ConfigPackage;
use anyhow::{Context, Result, anyhow};
use flate2::{Compression, write::GzEncoder};
use futures_util::StreamExt;
use reqwest::{Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::{env, io::Write, time::Duration};
use tokio::io::AsyncWriteExt;
use tokio::time;
use tracing::{error, info};

pub struct NetworkClient {
    hostname: String,
    id: String,
    client: reqwest::Client,
}

impl Default for NetworkClient {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkClient {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .gzip(true)
            .build()
            .unwrap();

        let id = crate::utils::system::get_serial_number();

        let hostname = "".to_owned();

        Self {
            id,
            hostname,
            client,
        }
    }

    pub fn get_serial(&self) -> String {
        self.id.clone()
    }

    pub fn get_mac_wlan0(&self) -> String {
        // read mac from cat /sys/class/net/wlan0/address or assign DEMO
        std::fs::read_to_string("/sys/class/net/wlan0/address")
            .unwrap_or(String::from("DE:MO:00:00:00:00"))
            .trim()
            .to_owned()
    }

    pub fn set_hostname(&mut self, hostname: String) {
        self.hostname = hostname;
    }

    pub async fn send_compressed_post<T: serde::Serialize>(
        &self,
        token: &str,
        endpoint: &str,
        message: &T,
    ) -> Result<(StatusCode, Response)> {
        let client = self.client.clone();
        let url = format!("{}{}", self.hostname, endpoint);

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        let json = serde_json::to_vec(&message).unwrap_or_default();
        encoder.write_all(&json)?;

        let compressed_data = encoder.finish()?;

        let request = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .header("Content-Encoding", "gzip")
            .body(compressed_data)
            .send()
            .await?;

        let status_code = request.status();

        Ok((status_code, request))
    }

    pub async fn get_release_packages(
        &self,
        release_id: i32,
        token: &str,
    ) -> Result<Vec<ConfigPackage>> {
        let url = format!("{}/releases/{}/packages", self.hostname, release_id);
        let response = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await;

        response?
            .json()
            .await
            .with_context(|| "Failed to Parse JSON respone")
    }

    pub async fn get_package(&self, package_name: &str, token: &str) -> Result<()> {
        let path = env::current_dir()?;

        let mut local_packages_folder = path.clone();
        local_packages_folder.push("packages");

        let mut local_package_path = local_packages_folder.clone();
        local_package_path.push(package_name);

        let mut local_package_path_tmp = local_package_path.clone();
        local_package_path_tmp.set_extension("tmp");

        if local_package_path.exists() {
            info!("Package already exists locally");
            return Ok(());
        } else {
            info!("Package does not exist locally, fetching...");
        }

        let query = vec![("name", package_name)];
        let url = format!("{}/package", self.hostname);
        let stream = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", token))
            .timeout(Duration::from_secs(10 * 60))
            .query(&query)
            .send()
            .await?;

        if stream.status() != 200 {
            return Err(anyhow!("Failed to get package"));
        }

        let mut response = stream.bytes_stream();

        let start_time = time::Instant::now();

        tokio::fs::create_dir_all(&local_packages_folder).await?;
        let mut file = tokio::fs::File::create(&local_package_path_tmp).await?;
        let mut total_bytes = 0u64;
        while let Some(chunk) = response.next().await {
            let data = chunk?;
            total_bytes += data.len() as u64;
            file.write_all(&data).await?;
        }

        file.flush().await?;

        let download_duration = time::Instant::now() - start_time;

        if total_bytes == 0 {
            error!(
                "Downloaded 0 bytes for package {} — deleting temp file",
                package_name
            );
            tokio::fs::remove_file(&local_package_path_tmp).await.ok();
            return Err(anyhow!(
                "Package {} download failed: 0 bytes received",
                package_name
            ));
        }

        tokio::fs::rename(&local_package_path_tmp, &local_package_path).await?;

        info!(
            "Package {} downloaded in {:?} to {:?}",
            package_name, download_duration, local_package_path
        );

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkStats {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
}

pub async fn read_network_stats() -> Result<HashMap<String, NetworkStats>> {
    let content = tokio::fs::read_to_string("/proc/net/dev").await?;
    parse_network_stats(&content)
}

fn parse_network_stats(content: &str) -> Result<HashMap<String, NetworkStats>> {
    let mut stats = HashMap::new();

    for line in content.lines().skip(2) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 17 {
            continue;
        }

        let interface = parts[0].trim_end_matches(':').to_string();

        let rx_bytes = parts[1].parse().unwrap_or(0);
        let rx_errors = parts[3].parse().unwrap_or(0);
        let tx_bytes = parts[9].parse().unwrap_or(0);
        let tx_errors = parts[11].parse().unwrap_or(0);

        stats.insert(
            interface,
            NetworkStats {
                rx_bytes,
                tx_bytes,
                rx_errors,
                tx_errors,
            },
        );
    }

    Ok(stats)
}

pub async fn get_primary_interface_name() -> Result<String> {
    let content = tokio::fs::read_to_string("/proc/net/route")
        .await
        .context("Failed to read /proc/net/route")?;

    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1] == "00000000" {
            return Ok(parts[0].to_string());
        }
    }

    Ok("eth0".to_string())
}
