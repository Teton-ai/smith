use super::fsops::{self, MAX_DOWNLOAD_BYTES, OpenedFile};
use crate::magic::MagicHandle;
use crate::session::SessionHandle;
use crate::shutdown::ShutdownSignals;
use crate::utils::schema::{FileOpError, FileOpRequest, FileOpResponse};
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::http::Request;
use tracing::{error, info, warn};

/// Hard ceiling on a browsing session, matching the log stream's cap.
const SESSION_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// A session with no traffic for this long is abandoned; the dashboard tab was
/// almost certainly closed.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Concurrent browsing sessions per device. Two is enough for a second operator
/// to look while the first is mid-download, and bounds descriptor use.
const MAX_SESSIONS: usize = 2;
/// Files held open awaiting transfer within one session.
const MAX_HELD_FILES: usize = 4;
/// A descriptor whose upload never starts is released after this, so an api
/// crash between `Open` and `StartUpload` can't leak it for the session's life.
const HELD_FILE_TTL: Duration = Duration::from_secs(5 * 60);
/// Transfer chunk. Matches the api's existing test-file chunking.
const CHUNK_SIZE: usize = 64 * 1024;

struct Session {
    task: tokio::task::JoinHandle<()>,
}

impl Session {
    fn stop(&self) {
        self.task.abort();
    }
}

pub enum ActorMessage {
    OpenSession {
        session_id: String,
        result: oneshot::Sender<Result<()>>,
    },
    CloseSession {
        session_id: String,
    },
    SessionEnded {
        session_id: String,
    },
}

pub struct Actor {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<ActorMessage>,
    sender: mpsc::Sender<ActorMessage>,
    magic: MagicHandle,
    session: SessionHandle,
    sessions: HashMap<String, Session>,
}

impl Actor {
    pub fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<ActorMessage>,
        sender: mpsc::Sender<ActorMessage>,
        magic: MagicHandle,
        session: SessionHandle,
    ) -> Self {
        Self {
            shutdown,
            receiver,
            sender,
            magic,
            session,
            sessions: HashMap::new(),
        }
    }

    async fn open_session(&mut self, session_id: String) -> Result<()> {
        if self.sessions.contains_key(&session_id) {
            return Err(anyhow::anyhow!("File session {session_id} already exists"));
        }
        if self.sessions.len() >= MAX_SESSIONS {
            return Err(anyhow::anyhow!(
                "Too many file sessions open ({MAX_SESSIONS} max)"
            ));
        }

        // Prefer the short-lived device JWT; falls back to the opaque token
        // when no valid JWT is cached (see SessionHandle::bearer_token).
        let token = self
            .session
            .bearer_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No device token available"))?;

        let server_url = self.magic.get_server().await;
        let ws_url = websocket_url(&server_url, &session_id)?;
        let upload_url = format!("{server_url}/files/upload");

        let session_id_clone = session_id.clone();
        let shutdown = self.shutdown.clone();
        let cleanup_sender = self.sender.clone();

        let task = tokio::spawn(async move {
            let result = tokio::time::timeout(
                SESSION_TIMEOUT,
                run_session(&ws_url, &upload_url, &token, shutdown),
            )
            .await;

            match result {
                Ok(Ok(())) => info!("File session {session_id_clone} ended"),
                Ok(Err(e)) => error!("File session {session_id_clone} error: {e}"),
                Err(_) => info!("File session {session_id_clone} timed out"),
            }

            cleanup_sender
                .send(ActorMessage::SessionEnded {
                    session_id: session_id_clone,
                })
                .await
                .inspect_err(|e| error!("Failed to report file session end: {e}"))
                .ok();
        });

        self.sessions.insert(session_id, Session { task });
        Ok(())
    }

    fn close_session(&mut self, session_id: &str) {
        if let Some(session) = self.sessions.remove(session_id) {
            info!("Closing file session {session_id}");
            session.stop();
        }
    }

    pub async fn run(&mut self) {
        info!("FileBrowser actor is running");

        loop {
            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    match msg {
                        ActorMessage::OpenSession { session_id, result } => {
                            let res = self.open_session(session_id).await;
                            result
                                .send(res)
                                .inspect_err(|_| warn!("File session requester went away"))
                                .ok();
                        }
                        ActorMessage::CloseSession { session_id } => {
                            self.close_session(&session_id);
                        }
                        ActorMessage::SessionEnded { session_id } => {
                            self.sessions.remove(&session_id);
                        }
                    }
                }
                _ = self.shutdown.token.cancelled() => {
                    break;
                }
            }
        }

        for (session_id, session) in self.sessions.drain() {
            info!("Stopping file session {session_id} on shutdown");
            session.stop();
        }

        info!("FileBrowser actor shutting down");
    }
}

/// Derive the session websocket URL from the configured server, e.g.
/// `https://api.smith.teton.ai/smith` -> `wss://api.smith.teton.ai/ws/file-session/{id}`.
fn websocket_url(server_url: &str, session_id: &str) -> Result<String> {
    let parsed = url::Url::parse(server_url)?;
    let scheme = if parsed.scheme() == "https" {
        "wss"
    } else {
        "ws"
    };
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid server URL: no host"))?;
    let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
    Ok(format!(
        "{scheme}://{host}{port}/ws/file-session/{session_id}"
    ))
}

struct HeldFile {
    opened: OpenedFile,
    held_since: tokio::time::Instant,
}

async fn run_session(
    ws_url: &str,
    upload_url: &str,
    device_token: &str,
    shutdown: ShutdownSignals,
) -> Result<()> {
    let request = Request::builder()
        .uri(ws_url)
        .header("Authorization", format!("Bearer {device_token}"))
        .header(
            "Host",
            url::Url::parse(ws_url)?
                .host_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid websocket URL: no host"))?,
        )
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tokio_tungstenite::tungstenite::handshake::client::generate_key(),
        )
        .body(())?;

    let (ws_stream, _) = tokio_tungstenite::connect_async(request).await?;
    let (mut write, mut read) = ws_stream.split();

    info!("Connected to file session websocket: {ws_url}");

    // No global timeout: a legitimate 512 MiB upload over a slow uplink can
    // take longer than any fixed deadline worth setting.
    let uploader = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()?;

    let mut held: HashMap<u64, HeldFile> = HashMap::new();
    let mut idle = Box::pin(tokio::time::sleep(IDLE_TIMEOUT));

    loop {
        tokio::select! {
            msg = read.next() => {
                let Some(msg) = msg else {
                    info!("File session websocket ended");
                    break;
                };

                match msg {
                    Ok(Message::Text(text)) => {
                        let request: FileOpRequest = match serde_json::from_str(&text) {
                            Ok(request) => request,
                            Err(e) => {
                                warn!("Ignoring malformed file operation: {e}");
                                continue;
                            }
                        };

                        let response = handle_request(
                            request,
                            &mut held,
                            &uploader,
                            upload_url,
                            device_token,
                        )
                        .await;

                        if let Some(response) = response {
                            let encoded = serde_json::to_string(&response)?;
                            if let Err(e) = write.send(Message::Text(encoded)).await {
                                error!("Failed to send file operation response: {e}");
                                break;
                            }
                        }

                        // Reset only after the request (possibly a long upload)
                        // finished, so slow operations don't count as idle time.
                        idle = Box::pin(tokio::time::sleep(IDLE_TIMEOUT));
                    }
                    Ok(Message::Ping(data)) => {
                        write
                            .send(Message::Pong(data))
                            .await
                            .inspect_err(|e| error!("Failed to pong: {e}"))
                            .ok();
                    }
                    Ok(Message::Close(_)) => {
                        info!("File session closed by server");
                        break;
                    }
                    Err(e) => {
                        error!("File session websocket error: {e}");
                        break;
                    }
                    _ => {}
                }
            }
            _ = &mut idle => {
                info!("File session idle for {IDLE_TIMEOUT:?}, closing");
                break;
            }
            _ = shutdown.token.cancelled() => {
                info!("Shutdown signal received, closing file session");
                break;
            }
        }

        // Release descriptors whose upload never started.
        held.retain(|op_id, file| {
            let fresh = file.held_since.elapsed() < HELD_FILE_TTL;
            if !fresh {
                warn!("Releasing file held for op {op_id}: upload never started");
            }
            fresh
        });
    }

    write
        .send(Message::Close(None))
        .await
        .inspect_err(|e| error!("Failed to close file session cleanly: {e}"))
        .ok();

    Ok(())
}

async fn handle_request(
    request: FileOpRequest,
    held: &mut HashMap<u64, HeldFile>,
    uploader: &reqwest::Client,
    upload_url: &str,
    device_token: &str,
) -> Option<FileOpResponse> {
    match request {
        FileOpRequest::List { op_id, path } => {
            // read_dir and lstat can block indefinitely on a hung network
            // mount, which would take a runtime worker thread with them.
            let result = tokio::task::spawn_blocking(move || fsops::list_dir(&path)).await;

            Some(match result {
                Ok(Ok((path, entries, truncated))) => FileOpResponse::Listing {
                    op_id,
                    path,
                    entries,
                    truncated,
                },
                Ok(Err(code)) => error_response(op_id, code),
                Err(e) => {
                    error!("Listing task failed: {e}");
                    error_response(op_id, FileOpError::Io)
                }
            })
        }

        FileOpRequest::Open { op_id, path } => {
            if held.len() >= MAX_HELD_FILES {
                return Some(error_response(op_id, FileOpError::TooManyOpenFiles));
            }

            let result = tokio::task::spawn_blocking(move || fsops::open_file(&path)).await;

            Some(match result {
                Ok(Ok(opened)) => {
                    let (name, size) = (opened.name.clone(), opened.size);
                    held.insert(
                        op_id,
                        HeldFile {
                            opened,
                            held_since: tokio::time::Instant::now(),
                        },
                    );
                    FileOpResponse::Opened { op_id, name, size }
                }
                Ok(Err(code)) => error_response(op_id, code),
                Err(e) => {
                    error!("Open task failed: {e}");
                    error_response(op_id, FileOpError::Io)
                }
            })
        }

        FileOpRequest::StartUpload {
            op_id,
            upload_token,
        } => {
            let Some(file) = held.remove(&op_id) else {
                return Some(error_response(op_id, FileOpError::NotFound));
            };

            Some(
                match upload(
                    uploader,
                    upload_url,
                    device_token,
                    &upload_token,
                    file.opened,
                )
                .await
                {
                    Ok(bytes_sent) => FileOpResponse::UploadFinished { op_id, bytes_sent },
                    Err(e) => {
                        error!("Upload for op {op_id} failed: {e}");
                        FileOpResponse::Error {
                            op_id,
                            code: FileOpError::Io,
                            message: e.to_string(),
                        }
                    }
                },
            )
        }

        FileOpRequest::Cancel { op_id } => {
            held.remove(&op_id);
            None
        }
    }
}

fn error_response(op_id: u64, code: FileOpError) -> FileOpResponse {
    let message = match code {
        FileOpError::NotFound => "No such file or directory",
        FileOpError::PermissionDenied => "Permission denied",
        FileOpError::NotADirectory => "Not a directory",
        FileOpError::NotRegularFile => "Not a regular file",
        FileOpError::TooLarge => "File is larger than the download limit",
        FileOpError::TooManyOpenFiles => "Too many files open at once",
        FileOpError::Io => "I/O error",
    };
    FileOpResponse::Error {
        op_id,
        code,
        message: message.to_string(),
    }
}

/// Stream the held descriptor to the api. The body is a chunked stream over the
/// file, never a buffer: a 512 MiB file must not become 512 MiB of daemon RSS.
async fn upload(
    client: &reqwest::Client,
    upload_url: &str,
    device_token: &str,
    upload_token: &str,
    opened: OpenedFile,
) -> Result<u64> {
    let file = tokio::fs::File::from_std(opened.file);

    // Bound what is actually read, not just what `st_size` claimed: some /proc
    // and /sys files report zero and then stream without end.
    let limited = tokio::io::AsyncReadExt::take(file, MAX_DOWNLOAD_BYTES);
    let stream = tokio_util::io::ReaderStream::with_capacity(limited, CHUNK_SIZE);

    let response = client
        .post(upload_url)
        .header("Authorization", format!("Bearer {device_token}"))
        .header("X-Upload-Token", upload_token)
        .header("Content-Type", "application/octet-stream")
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Upload rejected with status {}",
            response.status()
        ));
    }

    Ok(opened.size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_the_websocket_url_from_the_server_url() {
        assert_eq!(
            websocket_url("https://api.smith.teton.ai/smith", "abc").unwrap(),
            "wss://api.smith.teton.ai/ws/file-session/abc"
        );
        assert_eq!(
            websocket_url("http://localhost:8080/smith", "abc").unwrap(),
            "ws://localhost:8080/ws/file-session/abc"
        );
    }

    #[test]
    fn rejects_a_server_url_without_a_host() {
        assert!(websocket_url("not-a-url", "abc").is_err());
    }
}
