use crate::{api::SmithAPI, auth, print::TablePrint};
use anyhow::{Context as _, bail};
use chrono_humanize::HumanTime;
use clap::{Args, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use models::os::{NewOsUpload, OsPartReport, OsUploadPlan};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// Read size while hashing. Large enough that a 20 GB image is not death by
/// syscall, small enough to stay off the stack and out of a big allocation.
const HASH_BUFFER_SIZE: usize = 8 * 1024 * 1024;

/// Attempts per part before the whole push gives up. Parts fail independently
/// -- a flaky link drops one, not the upload.
const PART_ATTEMPTS: u32 = 3;

#[derive(Args, Debug)]
pub struct OsPush {
    /// Release to attach the image to
    release_number: String,
    /// Path to the .tar.gz image
    #[arg(short, long)]
    path: PathBuf,
    /// Bytes per part
    #[arg(long, default_value = "104857600")]
    part_size: i32,
    /// Parallel part uploads
    #[arg(long, default_value = "8")]
    concurrency: usize,
    /// Discard any upload already in progress and start from zero
    #[arg(long)]
    restart: bool,
}

#[derive(Subcommand, Debug)]
pub enum OsCommands {
    /// Upload a base OS image and attach it to a draft release
    Push(OsPush),
    /// Show the image attached to a release
    Get {
        release_number: String,
        #[arg(long, default_value = "false")]
        json: bool,
    },
    /// Detach and delete the image from a draft release
    Rm {
        release_number: String,
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

impl OsCommands {
    pub async fn handle(self, config: crate::config::Config) -> anyhow::Result<()> {
        match self {
            OsCommands::Push(push) => push_os(push, config).await,
            OsCommands::Get {
                release_number,
                json,
            } => get_os(release_number, json, config).await,
            OsCommands::Rm {
                release_number,
                yes,
            } => remove_os(release_number, yes, config).await,
        }
    }
}

async fn build_api(config: &crate::config::Config) -> anyhow::Result<SmithAPI> {
    let secrets = auth::get_secrets(config)
        .await
        .with_context(|| "Error getting token")?
        .with_context(|| "No Token found, please Login")?;
    Ok(SmithAPI::new(secrets, config))
}

fn parse_release(release_number: &str) -> anyhow::Result<i32> {
    release_number
        .trim_start_matches('#')
        .parse()
        .context("Failed to parse release number as i32")
}

/// Bytes in a given part. Every part is `part_size` except the last, which is
/// whatever remains -- the same split the api assumes when it checks coverage.
fn part_len(part_number: i32, part_size: i64, size_bytes: i64) -> i64 {
    let offset = (part_number as i64 - 1) * part_size;
    part_size.min(size_bytes - offset)
}

async fn sha256_file(path: &Path, size_bytes: u64) -> anyhow::Result<String> {
    let progress = ProgressBar::new(size_bytes);
    progress.set_style(
        ProgressStyle::with_template(
            "  hashing  {bar:24} {bytes}/{total_bytes}  {bytes_per_sec}  eta {eta}",
        )?
        .progress_chars("=> "),
    );

    let mut file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("Failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; HASH_BUFFER_SIZE];

    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        progress.inc(read as u64);
    }

    progress.finish_and_clear();
    Ok(format!("{:x}", hasher.finalize()))
}

/// How one push splits its file. Shared by every part, so the byte range for a
/// part is derived the same way everywhere -- including by the api when it
/// checks the recorded parts cover the image.
#[derive(Clone)]
struct PushLayout {
    path: PathBuf,
    part_size: i64,
    size_bytes: i64,
}

/// Uploads one part straight to S3 and reports its ETag back to the api. The
/// ETag only ever appears on this response, so recording it is part of sending
/// the part rather than a later step.
async fn upload_part(
    api: &SmithAPI,
    http: &reqwest::Client,
    release_id: i32,
    layout: &PushLayout,
    url: &str,
    part_number: i32,
) -> anyhow::Result<i64> {
    let PushLayout {
        path,
        part_size,
        size_bytes,
    } = layout;

    let offset = (part_number as i64 - 1) * part_size;
    let len = part_len(part_number, *part_size, *size_bytes);

    let mut file = tokio::fs::File::open(path)
        .await
        .with_context(|| format!("Failed to open {}", path.display()))?;
    file.seek(std::io::SeekFrom::Start(offset as u64)).await?;

    let mut chunk = vec![0u8; len as usize];
    file.read_exact(&mut chunk).await.with_context(|| {
        format!("Failed to read part {part_number} ({len} bytes at offset {offset})")
    })?;

    let mut last_error = None;
    for attempt in 1..=PART_ATTEMPTS {
        let response = http.put(url).body(chunk.clone()).send().await;

        let etag = match response {
            Ok(response) if response.status().is_success() => response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|etag| etag.to_str().ok())
                .map(|etag| etag.to_string()),
            Ok(response) => {
                let status = response.status();
                last_error = Some(anyhow::anyhow!(
                    "S3 rejected part {part_number} with status {status}"
                ));
                None
            }
            Err(err) => {
                last_error = Some(anyhow::Error::from(err).context(format!(
                    "Failed to send part {part_number} (attempt {attempt})"
                )));
                None
            }
        };

        if let Some(etag) = etag {
            api.report_os_part(
                release_id,
                part_number,
                OsPartReport {
                    etag,
                    size_bytes: len,
                },
            )
            .await
            .with_context(|| format!("Failed to record part {part_number}"))?;

            return Ok(len);
        }

        if attempt < PART_ATTEMPTS {
            tokio::time::sleep(std::time::Duration::from_secs(2 * attempt as u64)).await;
        }
    }

    Err(last_error
        .unwrap_or_else(|| anyhow::anyhow!("Failed to upload part {part_number}"))
        .context(format!("Gave up on part {part_number}")))
}

async fn upload_plan(
    api: Arc<SmithAPI>,
    release_id: i32,
    path: &Path,
    plan: OsUploadPlan,
    size_bytes: i64,
    concurrency: usize,
) -> anyhow::Result<()> {
    let part_size = plan.part_size as i64;

    let already: i64 = plan
        .uploaded_parts
        .iter()
        .map(|n| part_len(*n, part_size, size_bytes))
        .sum();

    let progress = ProgressBar::new(size_bytes as u64);
    progress.set_style(
        ProgressStyle::with_template(
            "  uploading {bar:24} {bytes}/{total_bytes}  {bytes_per_sec}  eta {eta}",
        )?
        .progress_chars("=> "),
    );
    progress.set_position(already as u64);

    if !plan.uploaded_parts.is_empty() {
        println!(
            "  resuming: {} of {} parts already uploaded",
            plan.uploaded_parts.len(),
            plan.total_parts
        );
    }

    let http = reqwest::Client::new();
    let permits = Arc::new(Semaphore::new(concurrency.max(1)));
    let layout = PushLayout {
        path: path.to_path_buf(),
        part_size,
        size_bytes,
    };
    let mut tasks = JoinSet::new();

    for part in plan.parts {
        let api = Arc::clone(&api);
        let permits = Arc::clone(&permits);
        let http = http.clone();
        let layout = layout.clone();
        let progress = progress.clone();

        tasks.spawn(async move {
            let _permit = permits
                .acquire()
                .await
                .context("Upload semaphore closed unexpectedly")?;

            let sent = upload_part(
                &api,
                &http,
                release_id,
                &layout,
                &part.url,
                part.part_number,
            )
            .await?;

            progress.inc(sent as u64);
            Ok::<(), anyhow::Error>(())
        });
    }

    let mut failure = None;
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                // Keep the first real failure; the rest are usually the same
                // network problem reported several times over.
                tasks.abort_all();
                if failure.is_none() {
                    failure = Some(err);
                }
            }
            Err(err) if err.is_cancelled() => {}
            Err(err) => {
                tasks.abort_all();
                if failure.is_none() {
                    failure = Some(anyhow::Error::from(err).context("Upload task panicked"));
                }
            }
        }
    }

    progress.finish_and_clear();

    match failure {
        Some(err) => Err(err.context(
            "Upload incomplete. Re-run the same command to resume from the parts already sent",
        )),
        None => Ok(()),
    }
}

async fn push_os(push: OsPush, config: crate::config::Config) -> anyhow::Result<()> {
    let OsPush {
        release_number,
        path,
        part_size,
        concurrency,
        restart,
    } = push;

    let release_id = parse_release(&release_number)?;

    let metadata = tokio::fs::metadata(&path)
        .await
        .with_context(|| format!("Cannot read {}", path.display()))?;
    if !metadata.is_file() {
        bail!("{} is not a file", path.display());
    }
    let size_bytes = metadata.len() as i64;
    if size_bytes == 0 {
        bail!("{} is empty", path.display());
    }

    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .with_context(|| format!("Cannot determine a file name from {}", path.display()))?;

    let api = Arc::new(build_api(&config).await?);

    if restart {
        // A missing image is the normal case here, so a failure is worth a line
        // rather than an abort.
        if let Err(err) = api.delete_os(release_id).await {
            println!("  nothing to discard ({err})");
        }
    }

    let checksum = sha256_file(&path, metadata.len()).await?;
    println!("  {file_name}  sha256 {checksum}");

    let plan = api
        .create_os_upload(
            release_id,
            NewOsUpload {
                file_name,
                checksum,
                size_bytes,
                part_size: Some(part_size),
            },
        )
        .await?;

    upload_plan(
        Arc::clone(&api),
        release_id,
        &path,
        plan,
        size_bytes,
        concurrency,
    )
    .await?;

    let os = api.complete_os_upload(release_id).await?;
    println!(
        "OS image attached to release {release_id}: {} ({} bytes)",
        os.file_name, os.size_bytes
    );

    Ok(())
}

async fn get_os(
    release_number: String,
    json: bool,
    config: crate::config::Config,
) -> anyhow::Result<()> {
    let release_id = parse_release(&release_number)?;
    let api = build_api(&config).await?;
    let os = api.get_os(release_id).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&os)?);
        return Ok(());
    }

    let mut table =
        TablePrint::new_with_headers(vec!["File", "Size", "Status", "Checksum", "Uploaded"]);
    table.add_row(vec![
        os.file_name,
        human_size(os.size_bytes),
        os.status,
        os.checksum,
        os.uploaded_at
            .map(|at| HumanTime::from(at).to_string())
            .unwrap_or_else(|| "-".to_string()),
    ]);
    table.print();

    Ok(())
}

/// Sizes here run to tens of gigabytes, where a raw byte count tells a reader
/// nothing at a glance.
fn human_size(bytes: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

async fn remove_os(
    release_number: String,
    yes: bool,
    config: crate::config::Config,
) -> anyhow::Result<()> {
    let release_id = parse_release(&release_number)?;
    let api = build_api(&config).await?;

    if !yes {
        let os = api.get_os(release_id).await?;
        let confirmed = cliclack::confirm(format!(
            "Delete {} ({} bytes) from release {release_id}?",
            os.file_name, os.size_bytes
        ))
        .interact()?;
        if !confirmed {
            println!("Aborted.");
            return Ok(());
        }
    }

    api.delete_os(release_id).await?;
    println!("OS image removed from release {release_id}.");

    Ok(())
}
