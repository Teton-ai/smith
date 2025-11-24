use crate::{downloader::DownloaderHandle, magic::structure::ConfigPackage};
use anyhow::{Context, Result, anyhow};
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

    pub async fn get_package(
        &self,
        package_name: &str,
        // token: &str,
        downloader: &DownloaderHandle,
    ) -> Result<()> {
        let path = env::current_dir()?;

        let mut local_packages_folder = path.clone();
        local_packages_folder.push("packages");

        let mut local_package_path = local_packages_folder.clone();
        local_package_path.push(package_name);

        let mut local_package_path_tmp = local_package_path.clone();
        local_package_path_tmp.set_extension("tmp");

        if local_package_path.exists() {
            info!("Package already exists locally, downloader will check for resume");
        } else {
            info!("Package does not exist locally, fetching...");
        }

        let remote_file = format!("packages/{}", package_name);
        let _ = downloader
            .download_blocking(
                &remote_file,
                local_package_path.to_str().unwrap_or(""),
                5000.0,
            )
            .await?; // basically
        // unlimited rate here

        Ok(())
    }
}
