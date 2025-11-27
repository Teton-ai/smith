use crate::magic::MagicHandle;
use crate::shutdown::ShutdownHandler;
use anyhow;
use futures::StreamExt;
use governor::{Quota, RateLimiter};
use reqwest::{Client, Url};
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{error, info, warn};

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

pub async fn download_file_mb(
    magic: MagicHandle,
    remote_file: String,
    local_file: String,
    rate: f64,
    force_stop: Arc<AtomicBool>,
) -> anyhow::Result<DownloadStats> {
    // Convert the MB rate to bytes/sec
    let bytes_per_second = ((rate * 1_000_000.0).ceil() as u64).max(1);

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
) -> anyhow::Result<DownloadStats> {
    let mut rec_track = 0;
    let mut stats = DownloadStats::default();
    let configuration = magic.clone();
    let client = Client::new();
    let server_api_url = configuration.get_server().await;

    if let Some(r) = recurse {
        if r > 1 {
            // Break out of the recursion loop
            stats.error_message = Some("Downloaded 0 bytes too many times".to_owned());

            return Ok(stats.clone());
        } else {
            rec_track = r + 1
        }
    }

    let token = magic.get_token().await;
    let token = token.unwrap_or_default();

    let url = if remote_path.is_empty() {
        format!("{}/download", &server_api_url)
    } else {
        let mut url = Url::parse(&format!("{}/download", &server_api_url))
            .map_err(|e| anyhow::anyhow!("Invalid server URL: {}", e))?;
        url.query_pairs_mut().append_pair("path", remote_path);
        url.to_string()
    };

    // Create local file path if it does not exist
    if let Some(parent) = Path::new(local_path).parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).await?;
        }
    }

    // Check if file already exists and get its size for resuming
    let mut downloaded: u64 = 0;
    let mut resume_download = false;

    if let Ok(metadata) = tokio::fs::metadata(local_path).await {
        downloaded = metadata.len();
    }

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

    // Get the etag for verification
    let etag = initial_response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok());

    // Get the total content length of the object
    let file_length = initial_response
        .headers()
        .get("x-file-size")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());

    let content_length = match file_length {
        Some(length) => length,
        None => {
            return Err(anyhow::anyhow!("x-file-size header missing or invalid"));
        }
    };

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

    // Check if resume should be true
    if downloaded > 0 && downloaded < content_length {
        resume_download = true;
        info!("Found existing file with size {} bytes", downloaded);
    }

    // Build get based on if some of the file has already been downloaded
    let mut request = client.get(presigned_url);

    if resume_download && downloaded > 0 {
        if let Some(stored) = xattr::get(local_path, "user.etag")? {
            let stored_etag = String::from_utf8_lossy(&stored);

        match etag {
            Some(etag) if stored_etag == etag => {
                // Check if the full file is already downloaded
                if downloaded == content_length {
                    info!("File already fully downloaded at {}", local_path);
                    stats.success = true;
                    stats.bytes_downloaded = downloaded;
                    return Ok(stats);
                }

                resume_download = true;
                info!("Resuming download from byte {}", downloaded);
                request = request.header("Range", format!("bytes={}-", downloaded));
            }
            Some(etag) => {
                warn!(
                    "ETag mismatch (local: {}, remote: {}), restarting download",
                    stored_etag, etag
                );
                // Remove the existing file and start over
                tokio::fs::remove_file(local_path).await?;

                resume_download = false;
                downloaded = 0;
            }
            None => {
                // No stored ETag, cannot verify, restart download
                warn!("No ETag provided by server, restarting download");
                tokio::fs::remove_file(local_path).await?;

                resume_download = false;
                downloaded = 0;
            }
        }
    }

    // Send the request to download the file
    let mut response = request.send().await?;

    let expected_status = if resume_download && downloaded > 0 {
        reqwest::StatusCode::PARTIAL_CONTENT
    } else {
        reqwest::StatusCode::OK
    };

    if response.status() != expected_status {
        if resume_download && response.status() == reqwest::StatusCode::OK {
            warn!("Server does not support resume, starting from beginning");

            tokio::fs::remove_file(local_path).await?;

            downloaded = 0;
            resume_download = false;

            response = client.get(presigned_url).send().await?;

            if !response.status().is_success() {
                return Err(anyhow::anyhow!(
                    "Failed to download file from pre-signed URL: {:?}",
                    response.status()
                ));
            }
        } else {
            return Err(anyhow::anyhow!(
                "Failed to download file from pre-signed URL: {:?}",
                response.status()
            ));
        }
    }

    // Open the file for writing
    let mut file = if resume_download && downloaded > 0 {
        tokio::fs::OpenOptions::new()
            .write(true)
            .append(true)
            .open(local_path)
            .await?
    } else {
        let f = tokio::fs::File::create(local_path).await?;
        if let Some(etag) = etag {
            xattr::set(local_path, "user.etag", etag.as_bytes())?;
        }

        f
    };

    let bytes_per_second_u32 = (bytes_per_second.min(u32::MAX as u64)) as u32;
    let quota = Quota::per_second(NonZeroU32::new(bytes_per_second_u32).unwrap());

    let limiter = RateLimiter::direct(quota);
    let mut stream = response.bytes_stream();
    let mut session_downloaded: u64 = 0;
    let start = std::time::Instant::now();

    // Force rate limiter to start empty so we don't have a large burst when starting download
    let max_burst = bytes_per_second_u32;

    match limiter.check_n(NonZeroU32::new(max_burst).unwrap()) {
        Ok(_) => (),

        Err(e) => error!("Rate limit exceeded: {}", e),
    }

    while let Some(chunk_result) = stream.next().await {
        // Check if download should be forcefully stopped
        if force_stop.load(std::sync::atomic::Ordering::SeqCst) {
            warn!("Timeout interrupt - download stopping forcefully");
            file.flush().await?;
            break;
        }

        match chunk_result {
            Ok(chunk) => {
                let chunk_size = NonZeroU32::new(chunk.len() as u32).unwrap();

                // Wait for rate limiter
                if let Err(e) = limiter.until_n_ready(chunk_size).await {
                    warn!("Rate limit exceeded: {}", e);
                }

                // Write chunk to file
                file.write_all(&chunk).await?;
                session_downloaded += chunk.len() as u64;
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
        session_downloaded as f64 / elapsed
    } else {
        0.0
    };

    stats.bytes_downloaded = session_downloaded;
    stats.elapsed_seconds = elapsed;
    stats.average_speed_mbps = avg_speed / 1_000_000.0;

    match tokio::fs::metadata(local_path).await {
        Ok(metadata) => {
            let file_size = metadata.len();

            if file_size != content_length {
                // error!(
                //     "Size mismatch: file on disk ({}), downloaded amount ({}), expected content length ({:?})",
                //     file_size, session_downloaded, content_length
                // );

                return Err(anyhow::anyhow!(
                    "Size mismatch: file on disk ({}), downloaded amount ({}), expected content length ({:?})",
                    file_size,
                    session_downloaded,
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
                info!(
                    "Downloaded file verification passed for file {}",
                    local_path
                );
                stats.success = true;
            }
        }

        Err(e) => {
            error!("Failed to verify file size on disk: {}", e);

            stats.error_message = Some(format!("Failed to verify file size on disk: {}", e));
            return Err(anyhow::anyhow!("Failed to verify file size on disk: {}", e));
        }
    }

    Ok(stats.clone())
}
