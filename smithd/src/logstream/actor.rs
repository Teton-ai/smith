use crate::magic::MagicHandle;
use crate::shutdown::ShutdownSignals;
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info};

struct LogStream {
    task: tokio::task::JoinHandle<()>,
}

impl LogStream {
    fn stop(&self) {
        self.task.abort();
    }
}

pub enum ActorMessage {
    StartStream {
        session_id: String,
        service_name: String,
        ws_url: String,
        result: oneshot::Sender<Result<()>>,
    },
    StopStream {
        session_id: String,
    },
}

pub struct Actor {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<ActorMessage>,
    magic: MagicHandle,
    streams: HashMap<String, LogStream>,
}

impl Actor {
    pub fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<ActorMessage>,
        magic: MagicHandle,
    ) -> Self {
        Self {
            shutdown,
            receiver,
            magic,
            streams: HashMap::new(),
        }
    }

    async fn start_stream(
        &mut self,
        session_id: String,
        service_name: String,
        ws_url: String,
    ) -> Result<()> {
        if self.streams.contains_key(&session_id) {
            return Err(anyhow::anyhow!("Stream already exists for session {}", session_id));
        }

        let token = self
            .magic
            .get_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No device token available"))?;
        let session_id_clone = session_id.clone();
        let shutdown = self.shutdown.clone();

        let task = tokio::spawn(async move {
            if let Err(e) = run_log_stream(&ws_url, &token, &service_name, shutdown).await {
                error!("Log stream error for session {}: {}", session_id_clone, e);
            }
            info!("Log stream ended for session {}", session_id_clone);
        });

        self.streams.insert(session_id, LogStream { task });
        Ok(())
    }

    fn stop_stream(&mut self, session_id: &str) {
        if let Some(stream) = self.streams.remove(session_id) {
            info!("Stopping log stream for session {}", session_id);
            stream.stop();
        }
    }

    pub async fn run(&mut self) {
        info!("LogStream actor is running");

        loop {
            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    match msg {
                        ActorMessage::StartStream { session_id, service_name, ws_url, result } => {
                            let res = self.start_stream(session_id, service_name, ws_url).await;
                            let _ = result.send(res);
                        }
                        ActorMessage::StopStream { session_id } => {
                            self.stop_stream(&session_id);
                        }
                    }
                }
                _ = self.shutdown.token.cancelled() => {
                    break;
                }
            }
        }

        for (session_id, stream) in self.streams.drain() {
            info!("Stopping log stream for session {} on shutdown", session_id);
            stream.stop();
        }

        info!("LogStream actor shutting down");
    }
}

async fn run_log_stream(
    ws_url: &str,
    device_token: &str,
    service_name: &str,
    shutdown: ShutdownSignals,
) -> Result<()> {
    let request = Request::builder()
        .uri(ws_url)
        .header("Authorization", format!("Bearer {}", device_token))
        .header("Host", url::Url::parse(ws_url)?.host_str().unwrap_or("localhost"))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", tokio_tungstenite::tungstenite::handshake::client::generate_key())
        .body(())?;

    let (ws_stream, _) = tokio_tungstenite::connect_async(request).await?;
    let (mut write, mut read) = ws_stream.split();

    info!("Connected to WebSocket for log streaming: {}", ws_url);

    let mut child = Command::new("journalctl")
        .args([
            "-u",
            service_name,
            "--follow",
            "--no-pager",
            "-o",
            "short-iso",
            "-n",
            "100",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("Failed to capture stdout"))?;
    let mut reader = BufReader::new(stdout).lines();

    loop {
        tokio::select! {
            line = reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        if let Err(e) = write.send(Message::Text(line)).await {
                            error!("Failed to send log line: {}", e);
                            break;
                        }
                    }
                    Ok(None) => {
                        info!("journalctl stream ended");
                        break;
                    }
                    Err(e) => {
                        error!("Error reading journalctl output: {}", e);
                        break;
                    }
                }
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Close(_))) => {
                        info!("WebSocket closed by server");
                        break;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = write.send(Message::Pong(data)).await;
                    }
                    Some(Err(e)) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    None => {
                        info!("WebSocket stream ended");
                        break;
                    }
                    _ => {}
                }
            }
            _ = shutdown.token.cancelled() => {
                info!("Shutdown signal received, stopping log stream");
                break;
            }
        }
    }

    let _ = write.send(Message::Close(None)).await;
    child.kill().await.ok();

    Ok(())
}
