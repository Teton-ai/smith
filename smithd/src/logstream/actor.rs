use crate::shutdown::ShutdownSignals;
use futures_util::stream::StreamExt;
use futures_util::SinkExt;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, warn};

pub enum ActorMessage {
    StartStream {
        service: Option<String>,
        response: oneshot::Sender<Result<u16, String>>,
    },
    StopStream,
}

pub struct Actor {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<ActorMessage>,
    active_port: Option<u16>,
    stop_signal: Option<oneshot::Sender<()>>,
}

impl Actor {
    pub fn new(shutdown: ShutdownSignals, receiver: mpsc::Receiver<ActorMessage>) -> Self {
        Self {
            shutdown,
            receiver,
            active_port: None,
            stop_signal: None,
        }
    }

    pub async fn run(&mut self) {
        loop {
            tokio::select! {
                _ = self.shutdown.token.cancelled() => {
                    info!("Log stream actor shutting down");
                    break;
                }
                msg = self.receiver.recv() => {
                    match msg {
                        Some(ActorMessage::StartStream { service, response }) => {
                            self.handle_start_stream(service, response).await;
                        }
                        Some(ActorMessage::StopStream) => {
                            self.handle_stop_stream().await;
                        }
                        None => {
                            warn!("Log stream actor channel closed");
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn handle_start_stream(
        &mut self,
        service: Option<String>,
        response: oneshot::Sender<Result<u16, String>>,
    ) {
        info!("Handling start stream request for service: {:?}", service);

        if self.active_port.is_some() {
            warn!("Stream already active on port: {:?}", self.active_port);
            let _ = response.send(Err("Stream already active".to_string()));
            return;
        }

        let port = match self.find_available_port().await {
            Ok(p) => {
                info!("Found available port: {}", p);
                p
            }
            Err(e) => {
                error!("Failed to find available port: {}", e);
                let _ = response.send(Err(e));
                return;
            }
        };

        let (stop_tx, stop_rx) = oneshot::channel();
        self.stop_signal = Some(stop_tx);
        self.active_port = Some(port);

        let port_clone = port;
        let service_clone = service.clone();

        tokio::spawn(async move {
            info!("Spawning WebSocket server on port {}", port_clone);
            if let Err(e) = Self::run_websocket_server(port_clone, service_clone, stop_rx).await {
                error!("WebSocket server error on port {}: {}", port_clone, e);
            }
        });

        info!("Sending success response with port {}", port);
        let _ = response.send(Ok(port));
    }

    async fn handle_stop_stream(&mut self) {
        if let Some(signal) = self.stop_signal.take() {
            let _ = signal.send(());
        }
        self.active_port = None;
        info!("Log stream stopped");
    }

    async fn find_available_port(&self) -> Result<u16, String> {
        use std::net::TcpListener;

        TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .map(|addr| addr.port())
            .map_err(|e| format!("Failed to find available port: {}", e))
    }

    async fn run_websocket_server(
        port: u16,
        service: Option<String>,
        mut stop_rx: oneshot::Receiver<()>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use tokio::net::TcpListener;

        let addr = format!("127.0.0.1:{}", port);
        let listener = TcpListener::bind(&addr).await?;
        info!("Log stream WebSocket server listening on {}", addr);

        loop {
            tokio::select! {
                _ = &mut stop_rx => {
                    info!("Stopping WebSocket server");
                    break;
                }
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _)) => {
                            let service_clone = service.clone();
                            tokio::spawn(async move {
                                if let Err(e) = Self::handle_connection(stream, service_clone).await {
                                    error!("Error handling connection: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            error!("Error accepting connection: {}", e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_connection(
        stream: tokio::net::TcpStream,
        service: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use tokio_tungstenite::{accept_async, tungstenite::Message};

        let ws_stream = accept_async(stream).await?;
        let (mut write, mut read) = ws_stream.split();

        let mut cmd = Command::new("journalctl");
        cmd.arg("-f").arg("-n").arg("100");

        if let Some(service_name) = service {
            cmd.arg("-u").arg(service_name);
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().ok_or("Failed to get stdout")?;
        let mut reader = BufReader::new(stdout).lines();

        loop {
            tokio::select! {
                line_result = reader.next_line() => {
                    match line_result {
                        Ok(Some(line)) => {
                            if write.send(Message::Text(line)).await.is_err() {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            error!("Error reading journalctl output: {}", e);
                            break;
                        }
                    }
                }
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Err(e)) => {
                            error!("WebSocket error: {}", e);
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        let _ = child.kill().await;
        Ok(())
    }
}
