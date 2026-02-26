use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
mod download;
use crate::downloader::download::DownloadStats;
use crate::magic::MagicHandle;
use crate::shutdown::ShutdownSignals;
use crate::utils::network::NetworkClient;
use anyhow::{self, Context};
use download::download_file_mb;
use tokio::sync::Mutex;
use tokio::{
    sync::{mpsc, oneshot},
    time,
};
use tracing::info;

#[derive(Debug)]
enum DownloaderMessage {
    Download {
        remote_file: String,
        local_file: String,
        rate: f64,
        rpc: Option<oneshot::Sender<anyhow::Result<DownloadStats>>>,
    },
    CheckStatus {
        rpc: oneshot::Sender<anyhow::Result<DownloadingStatus>>,
    },
}

#[derive(Debug)]
pub enum DownloadingStatus {
    Failed,
    Downloading,
    Success,
}

struct Downloader {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<DownloaderMessage>,
    magic: MagicHandle,
    downloading_count: Arc<AtomicUsize>,
    network: NetworkClient,
    force_stop: Arc<AtomicBool>,
    last_download_status: Arc<AtomicBool>,
    timeout: u64,
    download_lock: Arc<Mutex<()>>,
}

impl Downloader {
    fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<DownloaderMessage>,
        magic: MagicHandle,
        timeout: u64,
    ) -> Self {
        let network = NetworkClient::new();
        let force_stop = Arc::new(AtomicBool::new(false));
        let is_downloading = Arc::new(AtomicUsize::new(0));
        let last_download_status = Arc::new(AtomicBool::new(false));
        let download_lock = Arc::new(Mutex::new(()));

        Self {
            shutdown,
            receiver,
            magic,
            network,
            downloading_count: is_downloading,
            force_stop,
            timeout,
            last_download_status,
            download_lock,
        }
    }

    async fn handle_message(&mut self, msg: DownloaderMessage) {
        match msg {
            DownloaderMessage::Download {
                remote_file,
                local_file,
                rate,
                rpc,
            } => {
                self.downloading_count.fetch_add(1, Ordering::SeqCst);

                let magic = self.magic.clone();
                let force_stop = self.force_stop.clone();
                let is_downloading = self.downloading_count.clone();
                let last_download_status = self.last_download_status.clone();
                let download_lock = self.download_lock.clone();

                tokio::spawn(async move {
                    // Aqcuire global lock
                    let _guard = download_lock.lock().await;

                    // Do the download
                    let result =
                        download_file_mb(magic, remote_file, local_file, rate, force_stop).await;

                    if result.is_ok() {
                        last_download_status.store(true, Ordering::SeqCst);
                    } else {
                        last_download_status.store(false, Ordering::SeqCst);
                    }

                    // Reset status
                    is_downloading.fetch_sub(1, Ordering::SeqCst);

                    if let Some(rpc) = rpc {
                        let _ = rpc.send(result);
                    }

                    // Lock released when _gaurd dropped
                });
            }
            DownloaderMessage::CheckStatus { rpc } => {
                // Check if the thread is currently downloading
                let mut status = DownloadingStatus::Failed;
                if self.downloading_count.load(Ordering::SeqCst) > 0 {
                    status = DownloadingStatus::Downloading;
                } else if self.last_download_status.load(Ordering::SeqCst) {
                    status = DownloadingStatus::Success;
                }

                let _ = rpc.send(Ok(status));
            }
        }
    }

    async fn run(&mut self) {
        info!("Download task is running");

        let hostname = self.magic.get_server().await;

        self.network.set_hostname(hostname);

        loop {
            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    info!("Received message: {:?}", msg);
                    self.handle_message(msg).await;
                }

                _ = self.shutdown.token.cancelled() => {
                    let mut count = 1;

                    loop {
                        let active_downloads = self.downloading_count.load(Ordering::SeqCst);

                        if active_downloads == 0 {
                            info!("All downloads stopped, exiting.");
                            break;
                        }

                        info!("Waiting for download task to finish");
                        time::sleep(time::Duration::from_secs(1)).await;

                        if count > self.timeout {
                            info!("Download task did not finish in time. Forcing stop");
                            if !self.force_stop.load(Ordering::SeqCst) {
                                self.force_stop.store(true, Ordering::SeqCst);
                            }
                        }
                        count += 1;

                    }
                    info!("Download task shutting down gracefully");
                    break;
                }
            }
        }
    }
}

#[derive(Clone)]

pub struct DownloaderHandle {
    sender: mpsc::Sender<DownloaderMessage>,
}

impl DownloaderHandle {
    pub fn new(shutdown: ShutdownSignals, magic: MagicHandle) -> Self {
        let (sender, receiver) = mpsc::channel(8);

        let timeout = 5; // 5 second timeout

        let mut actor = Downloader::new(shutdown, receiver, magic, timeout);

        tokio::spawn(async move { actor.run().await });

        Self { sender }
    }

    pub async fn download(
        &self,
        remote_file: &str,
        local_file: &str,
        rate: f64,
    ) -> anyhow::Result<()> {
        // unwrap because if this fails then we are in a bad state
        self.sender
            .send(DownloaderMessage::Download {
                remote_file: remote_file.to_string(),
                local_file: local_file.to_string(),
                rate,
                rpc: None,
            })
            .await
            .unwrap();

        Ok(())
    }

    pub async fn download_blocking(
        &self,
        remote_file: &str,
        local_file: &str,
        rate: f64,
    ) -> anyhow::Result<()> {
        let (rpc, receiver) = oneshot::channel::<anyhow::Result<DownloadStats>>();

        self.sender
            .send(DownloaderMessage::Download {
                remote_file: remote_file.to_string(),
                local_file: local_file.to_string(),
                rate,
                rpc: Some(rpc),
            })
            .await
            .unwrap();

        // Get the stats
        let stats = receiver.await.context("Download task died")??;

        // Check if download was actually successful
        if !stats.success {
            return Err(anyhow::anyhow!(
                "Download failed: {}",
                stats
                    .error_message
                    .unwrap_or_else(|| "Unknown error".to_string())
            ));
        }

        // Log success info
        // TODO: check if this is actually going to log
        info!(
            "Downloaded {} bytes in {:.2}s at {:.2} MB/s",
            stats.bytes_downloaded, stats.elapsed_seconds, stats.average_speed_mbps
        );

        Ok(())
    }

    pub async fn check_download_status(&self) -> anyhow::Result<DownloadingStatus> {
        // unwrap because if this fails then we are in a bad state
        let (rpc, receiver) = oneshot::channel();

        self.sender
            .send(DownloaderMessage::CheckStatus { rpc })
            .await
            .unwrap();

        receiver.await.unwrap()
    }
}
