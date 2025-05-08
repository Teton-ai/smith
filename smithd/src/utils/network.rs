use crate::magic::structure::ConfigPackage;
use anyhow::{Result, anyhow};
use flate2::{Compression, write::GzEncoder};
use futures_util::StreamExt;
use reqwest::{Response, StatusCode};
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

    pub async fn get_release_packages(&self, release_id: Option<i32>) -> Vec<ConfigPackage> {
        if release_id.is_none() {
            return vec![];
        }
        let release_id = release_id.unwrap();
        let url = format!("{}/releases/{}/packages", self.hostname, release_id);
        let stream = self.client.get(url).send().await;

        match stream {
            Ok(stream) => stream.json().await.unwrap_or_default(),
            Err(e) => {
                error!("Error getting packages: {}", e);
                vec![]
            }
        }
    }

    pub async fn get_package(&self, package_name: &str) -> Result<()> {
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
