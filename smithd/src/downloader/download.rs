use crate::magic::MagicHandle;
use crate::shutdown::ShutdownHandler;
use anyhow;
use futures::StreamExt;
use governor::{Quota, RateLimiter};
use reqwest::Client;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{error, info};

#[derive(Debug, Clone)]

pub struct DownloadStats {
    pub bytes_downloaded: u64,

    pub elapsed_seconds: f64,

    pub average_speed_mbps: f64,

    pub success: bool,

    pub error_message: Option<String>,
}

impl Default for DownloadStats {
    fn default() -> Self {
        Self {
            bytes_downloaded: 0,

            elapsed_seconds: 0.0,

            average_speed_mbps: 0.0,

            success: false,

            error_message: None,
        }
    }
}

pub async fn download_package(
    magic: MagicHandle,
    remote_file: String,
    local_file: String,
    rate: f64,
    force_stop: Arc<AtomicBool>,
) -> anyhow::Result<String> {
    // Convert the MB rate to bytes/sec
    let bytes_per_second = (rate * 1_000_000.0) as u64;

    info!("Rate limit: {} bytes/sec", bytes_per_second);

    // Example: download at 1MB per second

    let result = download_file(
        magic,
        local_file.as_str(),
        remote_file.as_str(),
        bytes_per_second,
        force_stop,
        None,
    )
    .await?;

    Ok(result)
}

async fn download_file(
    magic: MagicHandle,

    local_path: &str,

    remote_path: &str,

    bytes_per_second: u64,

    force_stop: Arc<AtomicBool>,

    recurse: Option<u32>,
) -> anyhow::Result<String> {
    let mut rec_track = 0;

    let mut stats = DownloadStats::default();

    let shutdown = ShutdownHandler::new();

    let configuration = MagicHandle::new(shutdown.signals());

    configuration.load(None).await;

    let client = Client::new();

    let server_api_url = configuration.get_server().await;

    if let Some(r) = recurse {
        if r > 1 {
            // Break out of the recursion loop

            stats.error_message = Some("Downloaded 0 bytes too many times".to_owned());

            let output = convert_stats_to_string(stats, local_path).await;

            return Ok(output);
        } else {
            rec_track = r + 1
        }
    }

    let token = magic.get_token().await;

    let token = token.unwrap_or_default();

    let url = if remote_path.is_empty() {
        format!("{}/download", &server_api_url)
    } else {
        format!("{}/download/{}", &server_api_url, &remote_path)
    };

    let initial_response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    if !initial_response.status().is_success() && !initial_response.status().is_redirection() {
        return Err(anyhow::anyhow!(
            "Failed to download file: {:?}",
            initial_response.status()
        ));
    }

    // Extract the pre-signed URL from the Location header

    let presigned_url = match initial_response.headers().get("Location") {
        Some(location) => location
            .to_str()
            .map_err(|e| anyhow::anyhow!("Invalid location header: {:?}", e))?,

        None => {
            return Err(anyhow::anyhow!(
                "No pre-signed URL provided in response headers"
            ));
        }
    };

    let response = client.get(presigned_url).send().await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to download file from pre-signed URL: {:?}",
            response.status()
        ));
    }

    // Get the total content length of the object
    let content_length = response
        .headers()
        .get("Content-Length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());

    // Create root path if it does not exist
    if let Some(parent) = Path::new(local_path).parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).await?;
        }
    }

    // Open the file for writing
    let mut file = tokio::fs::File::create(local_path).await?;

    let quota = Quota::per_second(
        NonZeroU32::new(bytes_per_second as u32).unwrap_or(NonZeroU32::new(1).unwrap()),
    );

    let limiter = RateLimiter::direct(quota);
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let start = std::time::Instant::now();

    // Force rate limiter to start empty so we don't have a large burst when starting download
    let max_burst = bytes_per_second as u32;

    match limiter.check_n(NonZeroU32::new(max_burst).unwrap()) {
        Ok(_) => (),

        Err(e) => eprintln!("Rate limit exceeded: {}", e),
    }

    while let Some(chunk_result) = stream.next().await {
        // Check if download should be forcefully stopped

        if force_stop.load(std::sync::atomic::Ordering::SeqCst) {
            info!("Timeout interrupt - download stopping forcefully");

            file.flush().await?;

            break;
        }

        match chunk_result {
            Ok(chunk) => {
                let chunk_size = NonZeroU32::new(chunk.len() as u32).unwrap();

                // Wait for rate limiter
                if let Err(e) = limiter.until_n_ready(chunk_size).await {
                    eprintln!("Rate limit exceeded: {}", e);
                }

                // Write chunk to file
                file.write_all(&chunk).await?;

                downloaded += chunk.len() as u64;
            }

            Err(e) => {
                error!("Error downloading chunk: {}", e);

                return Err(anyhow::anyhow!("Download error: {}", e));
            }
        }
    }

    file.flush().await?;

    // Calculate and log final statistics

    let elapsed = start.elapsed().as_secs_f64();

    let avg_speed = if elapsed > 0.0 {
        downloaded as f64 / elapsed
    } else {
        0.0
    };

    stats.bytes_downloaded = downloaded;

    stats.elapsed_seconds = elapsed;

    stats.average_speed_mbps = avg_speed / 1_000_000.0;

    match tokio::fs::metadata(local_path).await {
        Ok(metadata) => {
            let file_size = metadata.len();

            if file_size != downloaded || Some(file_size) != content_length {
                error!(
                    "Size mismatch: file on disk ({}), downloaded amount ({}), expected content length ({:?})",
                    file_size, downloaded, content_length
                );

                return Err(anyhow::anyhow!(
                    "Size mismatch: file on disk ({}), downloaded amount ({}), expected content length ({:?})",
                    file_size,
                    downloaded,
                    content_length
                ));
            } else if file_size == 0 {
                // We know the file is completely busted here, try again 2x

                error!("File did not install properly. Re-installing");

                tokio::fs::remove_file(local_path).await?;

                Box::pin(download_file(
                    magic,
                    local_path,
                    remote_path,
                    bytes_per_second,
                    force_stop,
                    Some(rec_track),
                ))
                .await?;
            } else {
                info!("Downloaded file verification passed");

                stats.success = true;
            }
        }

        Err(e) => {
            error!("Failed to verify file size on disk: {}", e);

            stats.error_message = Some(format!("Failed to verify file size on disk: {}", e));
            return Err(anyhow::anyhow!("Failed to verify file size on disk: {}", e));
        }
    }

    let output = convert_stats_to_string(stats, local_path).await;

    Ok(output)
}

async fn convert_stats_to_string(stats: DownloadStats, local_path: &str) -> String {
    if stats.success {
        format!(
            "Download of {} succeeded - Downloaded file in {:.2} seconds at {:.2} MB/sec",
            local_path, stats.elapsed_seconds, stats.average_speed_mbps
        )
    } else {
        format!(
            "Download of {} failed - {:?}",
            local_path, stats.error_message
        )
    }
}
