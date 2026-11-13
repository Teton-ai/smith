use super::pace::{Pacer, PacerStats, Probe};
use crate::magic::MagicHandle;
use crate::postman::{LinkDelay, LinkDelayWatch};
use crate::session::SessionHandle;
use anyhow::{self, Context};
use futures::StreamExt;
use md5::{Digest, Md5};
use reqwest::{Client, Url};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tracing::{error, info, warn};

/// The ETag the API reports for the S3 object, set on the `.part` and kept by
/// the rename.
pub(crate) const ETAG_XATTR: &str = "user.etag";

/// Length of the `.part` prefix known to be on disk. After a power cut, ext4
/// can keep a file length whose last pages hold zeros instead of the written
/// data, so a resume trusts only this length, not the file size.
const SYNCED_LEN_XATTR: &str = "user.synced_len";

/// Bytes at most lost from a `.part` by a power cut.
const SYNC_INTERVAL: u64 = 8 * 1024 * 1024;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// A stalled connection otherwise holds the global download lock for ever.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// How often the pacer reads the link delay and publishes a rate.
const FEED_INTERVAL: Duration = Duration::from_secs(5);

/// Feed ticks without a new link delay before the pacer counts a loss. A /home
/// post that fails or stalls gives no sample.
const UNCHANGED_TICKS_TO_LOST: u32 = 3;

/// A paused download rechecks `force_stop` this often, within the 5 s
/// shutdown timeout.
const FORCE_STOP_POLL: Duration = Duration::from_secs(1);

/// What a download needs to pace itself and to stop.
#[derive(Clone)]
pub(super) struct Pacing {
    pub remote: String,
    /// Highest rate in bytes per second.
    pub cap: u64,
    pub force_stop: Arc<AtomicBool>,
    pub link_delay: LinkDelayWatch,
}

/// The pacer's state as the feed last published it.
struct Paced {
    /// `None` while paused.
    rate: Option<u64>,
    stats: PacerStats,
}

/// Aborts the feed task when dropped, so no exit path leaves it running.
struct FeedGuard(JoinHandle<()>);

impl Drop for FeedGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn next_probe(changed: bool, value: Option<LinkDelay>, unchanged_ticks: &mut u32) -> Option<Probe> {
    if changed && let Some(delay) = value {
        *unchanged_ticks = 0;
        return Some(Probe::Delay {
            baseline: Duration::from_millis(delay.baseline_ms.into()),
            current: Duration::from_millis(delay.current_ms.into()),
        });
    }
    *unchanged_ticks = unchanged_ticks.saturating_add(1);
    (*unchanged_ticks >= UNCHANGED_TICKS_TO_LOST).then_some(Probe::Lost)
}

/// Feeds the pacer from the /home link delay and publishes the allowed rate.
fn start_feed(pacing: &Pacing) -> (watch::Receiver<Paced>, FeedGuard) {
    // The tokio clock keeps the pacer in step with the sleeps that follow its rate.
    let started = tokio::time::Instant::now();
    let mut pacer = Pacer::new(pacing.cap, &pacing.remote, started.into_std());
    let (tx, rx) = watch::channel(Paced {
        rate: pacer.rate(started.into_std()),
        stats: pacer.stats().clone(),
    });
    let mut link_delay = pacing.link_delay.clone();
    // A clone reports a value sent before this download as new.
    link_delay.mark_unchanged();
    let handle = tokio::spawn(async move {
        let mut ticker = tokio::time::interval_at(started + FEED_INTERVAL, FEED_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut unchanged_ticks = 0;
        loop {
            ticker.tick().await;
            let changed = link_delay.has_changed().unwrap_or(false);
            let value = *link_delay.borrow_and_update();
            let now = tokio::time::Instant::now().into_std();
            if let Some(probe) = next_probe(changed, value, &mut unchanged_ticks) {
                pacer.observe(probe, now, rand::random());
            }
            tx.send_replace(Paced {
                rate: pacer.rate(now),
                stats: pacer.stats().clone(),
            });
        }
    });
    (rx, FeedGuard(handle))
}

/// The rate to pace the next chunk at. Waits while the pacer has paused; a
/// `force_stop` ends the wait so the chunk loop can sync the `.part` and stop.
async fn allowed_rate(rate_rx: &mut watch::Receiver<Paced>, pacing: &Pacing) -> u64 {
    loop {
        if let Some(rate) = rate_rx.borrow_and_update().rate {
            return rate.max(1);
        }
        if pacing.force_stop.load(Ordering::SeqCst) {
            return pacing.cap.max(1);
        }
        if let Ok(Err(_)) = tokio::time::timeout(FORCE_STOP_POLL, rate_rx.changed()).await {
            return pacing.cap.max(1);
        }
    }
}

fn log_summary(remote: &str, bytes: u64, started: Instant, paced: &watch::Receiver<Paced>) {
    let stats = paced.borrow().stats.clone();
    let (baseline, current) = stats.last_delay.unzip();
    info!(
        remote,
        bytes,
        seconds = started.elapsed().as_secs(),
        delay_samples = stats.delay_samples,
        lost_ticks = stats.lost,
        lowest_rate = stats.lowest_rate,
        back_offs = stats.back_offs,
        pauses = stats.pauses,
        ?baseline,
        ?current,
        "download paced"
    );
}

pub(crate) async fn md5_of_file(path: &Path) -> anyhow::Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut file =
            std::fs::File::open(&path).with_context(|| format!("opening {}", path.display()))?;
        let mut hasher = Md5::new();
        std::io::copy(&mut file, &mut hasher)
            .with_context(|| format!("hashing {}", path.display()))?;
        Ok(format!("{:x}", hasher.finalize()))
    })
    .await?
}

/// Package objects are uploaded in one part, so their S3 ETag is the MD5 of
/// the content. Any other ETag (e.g. a multipart `-N` suffix) means the upload
/// mode changed and the content cannot be verified.
pub(crate) fn md5_from_etag(raw: &[u8]) -> anyhow::Result<String> {
    let etag = std::str::from_utf8(raw)
        .context("ETag is not UTF-8")?
        .trim()
        .trim_matches('"');
    if etag.len() != 32 || !etag.bytes().all(|b| b.is_ascii_hexdigit()) {
        anyhow::bail!("ETag {etag} is not an MD5 digest, cannot verify the content");
    }
    Ok(etag.to_ascii_lowercase())
}

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
    session: SessionHandle,
    remote_file: String,
    local_file: String,
    rate: f64,
    force_stop: Arc<AtomicBool>,
    link_delay: LinkDelayWatch,
) -> anyhow::Result<DownloadStats> {
    // Convert the MB rate to bytes/sec
    let bytes_per_second = ((rate * 1_000_000.0).ceil() as u64).max(1);
    let pacing = Pacing {
        remote: remote_file.clone(),
        cap: bytes_per_second,
        force_stop,
        link_delay,
    };

    let result = download_file(
        magic,
        session,
        local_file.as_str(),
        remote_file.as_str(),
        pacing,
    )
    .await?;

    Ok(result)
}

async fn download_file(
    magic: MagicHandle,
    session: SessionHandle,
    local_path: &str,
    remote_path: &str,
    pacing: Pacing,
) -> anyhow::Result<DownloadStats> {
    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()?;
    let server_api_url = magic.get_server().await;

    // Prefer the short-lived device JWT; falls back to the opaque token when no
    // valid JWT is cached (see SessionHandle::bearer_token).
    let token = session.bearer_token().await.unwrap_or_default();

    let url = if remote_path.is_empty() {
        format!("{}/download", &server_api_url)
    } else {
        let mut url = Url::parse(&format!("{}/download", &server_api_url))
            .map_err(|e| anyhow::anyhow!("Invalid server URL: {}", e))?;
        url.query_pairs_mut().append_pair("path", remote_path);
        url.to_string()
    };

    fetch(&client, &url, &token, local_path, pacing, None).await
}

async fn fetch(
    client: &Client,
    url: &str,
    token: &str,
    local_path: &str,
    pacing: Pacing,
    recurse: Option<u32>,
) -> anyhow::Result<DownloadStats> {
    let mut rec_track = 0;
    let mut stats = DownloadStats::default();

    // Use .part file for atomic downloads
    let part_path = format!("{}.part", local_path);
    let part_path_str = part_path.as_str();

    if let Some(r) = recurse {
        if r > 1 {
            // Break out of the recursion loop
            stats.error_message = Some("Downloaded 0 bytes too many times".to_owned());

            // Clean up .part file before giving up
            let _ = tokio::fs::remove_file(part_path_str)
                .await
                .inspect_err(|e| warn!("Failed to clean up .part file: {}", e));

            return Ok(stats.clone());
        } else {
            rec_track = r + 1
        }
    }

    // Create local file path if it does not exist
    if let Some(parent) = Path::new(local_path).parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).await?;
    }

    let initial_response = client
        .get(url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    if !initial_response.status().is_success() && !initial_response.status().is_redirection() {
        return Err(anyhow::anyhow!(
            "Failed to download file: {:?}",
            initial_response.status()
        ));
    }

    let etag = initial_response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("ETag header missing or invalid"))?;
    let expected_md5 = md5_from_etag(etag.as_bytes())?;

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

    // Check if final file already exists and is complete
    if let Ok(final_metadata) = tokio::fs::metadata(local_path).await
        && final_metadata.len() == content_length
        && let Ok(Some(stored)) = xattr::get(local_path, ETAG_XATTR)
        && String::from_utf8_lossy(&stored) == etag
    {
        info!("File already fully downloaded at {}", local_path);
        stats.success = true;
        stats.bytes_downloaded = content_length;
        return Ok(stats);
    }

    let mut downloaded = synced_part_len(part_path_str, etag, content_length).await?;

    if downloaded > 0 && downloaded == content_length {
        info!("Part file complete, finalizing: {}", local_path);
        finalize(part_path_str, local_path, &expected_md5).await?;
        stats.success = true;
        stats.bytes_downloaded = downloaded;
        return Ok(stats);
    }

    // Build get based on if some of the file has already been downloaded
    let mut request = client.get(presigned_url);
    let mut resume_download = downloaded > 0;
    if resume_download {
        info!("Resuming download from byte {}", downloaded);
        request = request.header("Range", format!("bytes={}-", downloaded));
    }

    // Send the request to download the file
    let mut response = request.send().await?;

    let expected_status = if resume_download {
        reqwest::StatusCode::PARTIAL_CONTENT
    } else {
        reqwest::StatusCode::OK
    };

    if response.status() != expected_status {
        if resume_download && response.status() == reqwest::StatusCode::OK {
            warn!("Server does not support resume, starting from beginning");

            downloaded = 0;
            resume_download = false;
        } else {
            return Err(anyhow::anyhow!(
                "Failed to download file from pre-signed URL: {:?}",
                response.status()
            ));
        }
    }

    if resume_download {
        let range_str = response
            .headers()
            .get("content-range")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        // Should be like "bytes 5000-9999/1000"
        if !range_str.starts_with(&format!("bytes {}-", downloaded)) {
            warn!(
                "Server sent wrong range: {:?}, expected to start at {}. Restarting download.",
                range_str, downloaded
            );

            downloaded = 0;
            resume_download = false;

            // Request again without range header
            response = client.get(presigned_url).send().await?;

            if !response.status().is_success() {
                return Err(anyhow::anyhow!(
                    "Failed to download file from pre-signed URL: {:?}",
                    response.status()
                ));
            }
        }
    }

    // Open the .part file for writing
    let mut file = if resume_download {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(part_path_str)
            .await?
    } else {
        let f = tokio::fs::File::create(part_path_str).await?;
        xattr::set(part_path_str, ETAG_XATTR, etag.as_bytes())?;
        set_synced_len(part_path_str, 0)?;
        f
    };

    let (mut rate_rx, feed) = start_feed(&pacing);
    let mut stream = response.bytes_stream();
    let mut session_downloaded: u64 = 0;
    let mut unsynced: u64 = 0;
    let start = std::time::Instant::now();
    let mut next = tokio::time::Instant::now();

    while let Some(chunk_result) = stream.next().await {
        // Check if download should be forcefully stopped
        if pacing.force_stop.load(Ordering::SeqCst) {
            warn!("Timeout interrupt - download stopping forcefully");
            sync_part(&mut file, part_path_str, downloaded + session_downloaded).await?;
            log_summary(&pacing.remote, session_downloaded, start, &rate_rx);
            return Err(anyhow::anyhow!("Download interrupted by force_stop"));
        }

        match chunk_result {
            Ok(chunk) => {
                if chunk.is_empty() {
                    continue;
                }

                let rate = allowed_rate(&mut rate_rx, &pacing).await;
                next = next.max(tokio::time::Instant::now())
                    + Duration::from_secs_f64(chunk.len() as f64 / rate as f64);
                tokio::time::sleep_until(next).await;

                // Write chunk to file
                file.write_all(&chunk).await?;
                session_downloaded += chunk.len() as u64;
                unsynced += chunk.len() as u64;
                if unsynced >= SYNC_INTERVAL {
                    sync_part(&mut file, part_path_str, downloaded + session_downloaded).await?;
                    unsynced = 0;
                }
            }

            Err(e) => {
                error!("Error downloading chunk: {}", e);
                sync_part(&mut file, part_path_str, downloaded + session_downloaded).await?;
                log_summary(&pacing.remote, session_downloaded, start, &rate_rx);
                return Err(anyhow::anyhow!("Download error: {}", e));
            }
        }
    }

    drop(feed);
    log_summary(&pacing.remote, session_downloaded, start, &rate_rx);
    file.flush().await?;
    drop(file);

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

    match tokio::fs::metadata(part_path_str).await {
        Ok(metadata) => {
            let file_size = metadata.len();

            if file_size != content_length {
                // Clean up .part file on size mismatch
                let _ = tokio::fs::remove_file(part_path_str)
                    .await
                    .inspect_err(|e| warn!("Failed to clean up .part file: {}", e));

                return Err(anyhow::anyhow!(
                    "Size mismatch: file on disk ({}), downloaded amount ({}), expected content length ({:?})",
                    file_size,
                    session_downloaded,
                    content_length
                ));
            } else if file_size == 0 {
                // We know the file is completely busted here, try again
                error!("File did not download properly. Re-trying");

                return Box::pin(fetch(
                    client,
                    url,
                    token,
                    local_path,
                    pacing,
                    Some(rec_track),
                ))
                .await;
            } else {
                finalize(part_path_str, local_path, &expected_md5).await?;
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

/// Returns the length to resume the `.part` from and truncates the file to it.
/// Without a synced length for the same ETag, the download starts again.
async fn synced_part_len(part_path: &str, etag: &str, content_length: u64) -> anyhow::Result<u64> {
    let Ok(metadata) = tokio::fs::metadata(part_path).await else {
        return Ok(0);
    };
    let same_object = xattr::get(part_path, ETAG_XATTR)?
        .is_some_and(|stored| String::from_utf8_lossy(&stored) == etag);
    let synced_len = xattr::get(part_path, SYNCED_LEN_XATTR)?
        .and_then(|raw| String::from_utf8_lossy(&raw).parse::<u64>().ok());

    match synced_len {
        Some(len) if same_object && len <= metadata.len() && len <= content_length => {
            if len < metadata.len() {
                info!(
                    "Truncating {} from {} to the synced length {}",
                    part_path,
                    metadata.len(),
                    len
                );
                tokio::fs::OpenOptions::new()
                    .write(true)
                    .open(part_path)
                    .await?
                    .set_len(len)
                    .await?;
            }
            Ok(len)
        }
        _ => {
            warn!("No synced length for {}, restarting download", part_path);
            tokio::fs::remove_file(part_path).await?;
            Ok(0)
        }
    }
}

fn set_synced_len(part_path: &str, len: u64) -> anyhow::Result<()> {
    xattr::set(part_path, SYNCED_LEN_XATTR, len.to_string().as_bytes())
        .with_context(|| format!("recording synced length of {}", part_path))
}

/// Records `len` as safe to resume from only after the data is on disk.
async fn sync_part(file: &mut fs::File, part_path: &str, len: u64) -> anyhow::Result<()> {
    file.flush().await?;
    file.sync_data().await?;
    set_synced_len(part_path, len)
}

/// Checks the content against the ETag and makes the final file durable, so a
/// power cut after the rename cannot leave a damaged blob under the final name.
async fn finalize(part_path: &str, local_path: &str, expected_md5: &str) -> anyhow::Result<()> {
    let actual_md5 = md5_of_file(Path::new(part_path)).await?;
    if actual_md5 != expected_md5 {
        tokio::fs::remove_file(part_path).await?;
        anyhow::bail!(
            "MD5 mismatch for {}: content {}, ETag {}, removed the partial file",
            local_path,
            actual_md5,
            expected_md5
        );
    }
    info!("Downloaded file verification passed for {}", local_path);

    tokio::fs::File::open(part_path).await?.sync_all().await?;
    tokio::fs::rename(part_path, local_path)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to finalize download: {}", e))?;
    if let Some(parent) = Path::new(local_path).parent() {
        tokio::fs::File::open(parent).await?.sync_all().await?;
    }

    info!("Download finalized: {}", local_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::{Body, Bytes};
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode, header};
    use axum::response::Response;
    use axum::routing::get;
    use std::sync::Mutex;

    const MIB: usize = 1024 * 1024;

    fn delay(baseline_ms: u32, current_ms: u32) -> Option<LinkDelay> {
        Some(LinkDelay {
            baseline_ms,
            current_ms,
        })
    }

    #[test]
    fn a_changed_delay_is_a_sample_and_silence_becomes_a_loss_on_the_third_tick() {
        let mut unchanged = 0;
        let seen = |probe: Option<Probe>| match probe {
            None => "none",
            Some(Probe::Lost) => "lost",
            Some(Probe::Delay { .. }) => "delay",
        };
        let ticks = [
            (true, delay(20, 30), "delay"),
            (false, delay(20, 30), "none"),
            (false, delay(20, 30), "none"),
            (false, delay(20, 30), "lost"),
            (false, delay(20, 30), "lost"),
            (true, None, "lost"),
            (true, delay(20, 400), "delay"),
            (false, delay(20, 400), "none"),
        ];
        for (i, (changed, value, want)) in ticks.into_iter().enumerate() {
            let got = seen(next_probe(changed, value, &mut unchanged));
            assert_eq!(got, want, "tick {i}");
        }
        assert_eq!(unchanged, 1);
    }

    #[test]
    fn a_delay_sample_carries_both_values() {
        let mut unchanged = 2;
        let Some(Probe::Delay { baseline, current }) =
            next_probe(true, delay(18, 250), &mut unchanged)
        else {
            panic!("expected a delay sample");
        };
        assert_eq!(baseline, Duration::from_millis(18));
        assert_eq!(current, Duration::from_millis(250));
        assert_eq!(unchanged, 0);
    }

    struct FakeCdn {
        base: String,
        data: Vec<u8>,
        etag: String,
        /// The first blob response stops after this many bytes with a broken connection.
        drop_after: Mutex<Option<usize>>,
        /// `Range` header of each blob request.
        ranges: Mutex<Vec<Option<String>>>,
    }

    async fn api_download(State(cdn): State<Arc<FakeCdn>>) -> Response {
        Response::builder()
            .header(header::LOCATION, format!("{}/blob", cdn.base))
            .header("X-File-Size", cdn.data.len())
            .header(header::ETAG, &cdn.etag)
            .body(Body::empty())
            .unwrap_or_default()
    }

    async fn blob(State(cdn): State<Arc<FakeCdn>>, headers: HeaderMap) -> Response {
        let range = headers
            .get(header::RANGE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let start = range
            .as_deref()
            .and_then(|r| r.strip_prefix("bytes="))
            .and_then(|r| r.strip_suffix('-'))
            .and_then(|r| r.parse::<usize>().ok())
            .unwrap_or(0);
        cdn.ranges.lock().map(|mut r| r.push(range)).ok();

        let len = cdn.data.len();
        let body = Bytes::copy_from_slice(&cdn.data[start..]);
        let drop_after = cdn.drop_after.lock().ok().and_then(|mut d| d.take());
        let body = match drop_after {
            Some(n) => Body::from_stream(futures::stream::iter([
                Ok(body.slice(..n)),
                Err(std::io::Error::other("connection reset")),
            ])),
            None => Body::from(body),
        };

        let builder = if start > 0 {
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(
                    header::CONTENT_RANGE,
                    format!("bytes {start}-{}/{len}", len - 1),
                )
        } else {
            Response::builder()
        };
        builder.body(body).unwrap_or_default()
    }

    /// Incompressible content, like a `.deb`.
    fn package_bytes(len: usize) -> Vec<u8> {
        let mut state: u32 = 0x1234_5678;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect()
    }

    async fn serve(data: Vec<u8>, etag: Option<String>) -> anyhow::Result<Arc<FakeCdn>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let etag = etag.unwrap_or_else(|| format!("\"{:x}\"", Md5::digest(&data)));
        let cdn = Arc::new(FakeCdn {
            base: format!("http://{}", listener.local_addr()?),
            data,
            etag,
            drop_after: Mutex::new(None),
            ranges: Mutex::new(Vec::new()),
        });
        let app = Router::new()
            .route("/download", get(api_download))
            .route("/blob", get(blob))
            .with_state(cdn.clone());
        tokio::spawn(async move { axum::serve(listener, app).await });
        Ok(cdn)
    }

    async fn download(cdn: &FakeCdn, local: &Path) -> anyhow::Result<DownloadStats> {
        fetch(
            &Client::new(),
            &format!("{}/download", cdn.base),
            "token",
            local.to_str().unwrap_or_default(),
            Pacing {
                remote: "pkg".to_owned(),
                cap: 1_000_000_000,
                force_stop: Arc::new(AtomicBool::new(false)),
                link_delay: watch::channel(None).1,
            },
            None,
        )
        .await
    }

    fn ranges(cdn: &FakeCdn) -> Vec<Option<String>> {
        cdn.ranges.lock().map(|r| r.clone()).unwrap_or_default()
    }

    /// A `.part` as a power cut leaves it: the prefix of the object with one
    /// page of zeros at `zero_page`, and the given synced length.
    fn power_cut_part(
        cdn: &FakeCdn,
        part: &Path,
        len: usize,
        zero_page: usize,
        synced_len: Option<u64>,
    ) -> anyhow::Result<()> {
        let mut content = cdn.data[..len].to_vec();
        content[zero_page..zero_page + 4096].fill(0);
        std::fs::write(part, content)?;
        xattr::set(part, ETAG_XATTR, cdn.etag.as_bytes())?;
        if let Some(synced_len) = synced_len {
            xattr::set(part, SYNCED_LEN_XATTR, synced_len.to_string().as_bytes())?;
        }
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn resume_after_connection_reset_gives_identical_file() -> anyhow::Result<()> {
        let cdn = serve(package_bytes(20 * MIB), None).await?;
        *cdn.drop_after.lock().map_err(|e| anyhow::anyhow!("{e}"))? = Some(10 * MIB + 123);
        let temp = tempfile::tempdir()?;
        let local = temp.path().join("app.deb");

        assert!(download(&cdn, &local).await.is_err());
        // Everything received is synced on the error, not only up to the last interval.
        let part_len = std::fs::metadata(temp.path().join("app.deb.part"))?.len();
        assert!(part_len > SYNC_INTERVAL);
        let stats = download(&cdn, &local).await?;

        assert!(stats.success);
        assert_eq!(std::fs::read(&local)?, cdn.data);
        assert_eq!(ranges(&cdn), [None, Some(format!("bytes={part_len}-"))]);
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn resume_truncates_part_to_synced_length() -> anyhow::Result<()> {
        let cdn = serve(package_bytes(20 * MIB), None).await?;
        let temp = tempfile::tempdir()?;
        let local = temp.path().join("app.deb");
        power_cut_part(
            &cdn,
            &temp.path().join("app.deb.part"),
            12 * MIB,
            12 * MIB - 4096,
            Some(8 * MIB as u64),
        )?;

        download(&cdn, &local).await?;

        assert_eq!(std::fs::read(&local)?, cdn.data);
        assert_eq!(ranges(&cdn), [Some("bytes=8388608-".to_owned())]);
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn part_without_synced_length_downloads_again() -> anyhow::Result<()> {
        let cdn = serve(package_bytes(20 * MIB), None).await?;
        let temp = tempfile::tempdir()?;
        let local = temp.path().join("app.deb");
        power_cut_part(
            &cdn,
            &temp.path().join("app.deb.part"),
            12 * MIB,
            12 * MIB - 4096,
            None,
        )?;

        download(&cdn, &local).await?;

        assert_eq!(std::fs::read(&local)?, cdn.data);
        assert_eq!(ranges(&cdn), [None]);
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn content_that_does_not_match_etag_is_not_accepted() -> anyhow::Result<()> {
        let cdn = serve(package_bytes(20 * MIB), None).await?;
        let temp = tempfile::tempdir()?;
        let local = temp.path().join("app.deb");
        let part = temp.path().join("app.deb.part");
        power_cut_part(
            &cdn,
            &part,
            12 * MIB,
            12 * MIB - 4096,
            Some(12 * MIB as u64),
        )?;

        let result = download(&cdn, &local).await;

        assert_eq!(
            result.map(|s| s.success).map_err(|e| e.to_string()),
            Err(format!(
                "MD5 mismatch for {}: content {:x}, ETag {}, removed the partial file",
                local.display(),
                Md5::digest({
                    let mut d = cdn.data.clone();
                    d[12 * MIB - 4096..12 * MIB].fill(0);
                    d
                }),
                cdn.etag.trim_matches('"')
            ))
        );
        assert!(!local.exists());
        assert!(!part.exists());
        Ok(())
    }

    #[tokio::test(start_paused = true)]
    async fn multipart_etag_is_refused_before_download() -> anyhow::Result<()> {
        let cdn = serve(package_bytes(MIB), Some("\"abc-2\"".to_owned())).await?;
        let temp = tempfile::tempdir()?;
        let local = temp.path().join("app.deb");

        let result = download(&cdn, &local).await;

        assert_eq!(
            result.map(|s| s.success).map_err(|e| e.to_string()),
            Err("ETag abc-2 is not an MD5 digest, cannot verify the content".to_owned())
        );
        assert_eq!(ranges(&cdn), []);
        assert!(!local.exists());
        Ok(())
    }

    fn feed_pacing(link_delay: LinkDelayWatch) -> Pacing {
        Pacing {
            remote: "test".to_owned(),
            cap: 2_000_000,
            force_stop: Arc::new(AtomicBool::new(false)),
            link_delay,
        }
    }

    const SLOW: LinkDelay = LinkDelay {
        baseline_ms: 20,
        current_ms: 500,
    };

    #[tokio::test(start_paused = true)]
    async fn feed_pauses_on_a_slow_link_delay() {
        let (tx, rx) = watch::channel(None);
        let (rate, _guard) = start_feed(&feed_pacing(rx));
        tx.send_replace(Some(SLOW));
        tokio::time::sleep(FEED_INTERVAL + Duration::from_millis(1)).await;
        assert_eq!(rate.borrow().rate, None);
    }

    #[tokio::test(start_paused = true)]
    async fn feed_ignores_a_link_delay_sent_before_the_download() {
        let (tx, rx) = watch::channel(None);
        tx.send_replace(Some(SLOW));
        let (rate, _guard) = start_feed(&feed_pacing(rx));
        tokio::time::sleep(FEED_INTERVAL + Duration::from_millis(1)).await;
        assert!(rate.borrow().rate.is_some());
    }
}
