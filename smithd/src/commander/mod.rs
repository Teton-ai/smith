use crate::downloader::DownloaderHandle;
use crate::filemanager::FileManagerHandle;
use crate::logstream::LogStreamHandle;
use crate::shutdown::ShutdownSignals;
use crate::tunnel::TunnelHandle;
use crate::updater::UpdaterHandle;
use crate::utils::schema::{SafeCommandRequest, SafeCommandResponse, SafeCommandRx, SafeCommandTx};
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};
use tracing::info;

mod free;
mod logstream;
mod network;
mod ota;
mod restart;
mod tunnel;
mod upgrade;
mod variable;

struct CommandQueueExecutor {
    shutdown: ShutdownSignals,
    queue: mpsc::Receiver<SafeCommandRequest>,
    responses: mpsc::Sender<SafeCommandResponse>,
    tunnel_handle: TunnelHandle,
    updater_handle: UpdaterHandle,
    downloader_handle: DownloaderHandle,
    filemanager_handle: FileManagerHandle,
    logstream_handle: LogStreamHandle,
}

impl CommandQueueExecutor {
    fn new(
        shutdown: ShutdownSignals,
        queue: mpsc::Receiver<SafeCommandRequest>,
        responses: mpsc::Sender<SafeCommandResponse>,
        tunnel_handle: TunnelHandle,
        updater_handle: UpdaterHandle,
        downloader_handle: DownloaderHandle,
        filemanager_handle: FileManagerHandle,
        logstream_handle: LogStreamHandle,
    ) -> Self {
        Self {
            shutdown,
            queue,
            responses,
            tunnel_handle,
            updater_handle,
            downloader_handle,
            filemanager_handle,
            logstream_handle,
        }
    }

    async fn execute_command(&mut self, action: SafeCommandRequest) -> SafeCommandResponse {
        match action.command {
            SafeCommandTx::Ping => SafeCommandResponse {
                id: action.id,
                command: SafeCommandRx::Pong,
                status: 0,
            },
            SafeCommandTx::UpdateVariables { variables } => {
                variable::execute(action.id, variables).await
            }
            SafeCommandTx::Restart => restart::execute(&action).await,
            SafeCommandTx::FreeForm { cmd } => free::execute(action.id, cmd).await,
            SafeCommandTx::OpenTunnel {
                port,
                user,
                pub_key,
            } => tunnel::open_port(action.id, &self.tunnel_handle, port, user, pub_key).await,
            SafeCommandTx::CloseTunnel => tunnel::close_ssh(action.id, &self.tunnel_handle).await,
            SafeCommandTx::Upgrade => upgrade::upgrade(action.id, &self.updater_handle).await,
            SafeCommandTx::UpdateNetwork { network } => network::execute(action.id, network).await,
            SafeCommandTx::DownloadOTA {
                tools,
                payload,
                rate,
            } => {
                ota::download_ota(
                    action.id,
                    &self.downloader_handle,
                    &self.filemanager_handle,
                    &tools,
                    &payload,
                    rate,
                )
                .await
            }
            SafeCommandTx::CheckOTAStatus => {
                ota::check_ota(action.id, &self.downloader_handle).await
            }
            SafeCommandTx::StartOTA => ota::start_ota(action.id, &self.filemanager_handle).await,
            SafeCommandTx::StartLogStream { service } => {
                logstream::start_stream(action.id, &self.logstream_handle, service).await
            }
            SafeCommandTx::StopLogStream => {
                logstream::stop_stream(action.id, &self.logstream_handle).await
            }
        }
    }

    async fn run(&mut self) {
        loop {
            tokio::select! {
                Some(command) = self.queue.recv() => {
                    let response = self.execute_command(command).await;
                    _ = self.responses.send(response).await;
                }
                _ = self.shutdown.token.cancelled() => {
                    break;
                }
            }
        }

        info!("Commander Executioner task shutting down");
    }
}

#[derive(Debug, PartialEq, Eq)]
enum State {
    Queued,
    Completed,
}

struct SafeCommandState {
    state: State,
    response: Option<SafeCommandResponse>,
}

struct Commander {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<CommanderMessage>,
    queue: mpsc::Sender<SafeCommandRequest>,
    responses: mpsc::Receiver<SafeCommandResponse>,
    results: HashMap<i32, SafeCommandState>,
}

enum CommanderMessage {
    QueueCommand {
        action: SafeCommandRequest,
    },
    QueueResponse {
        action: SafeCommandResponse,
    },
    GetResults {
        tx: oneshot::Sender<Vec<SafeCommandResponse>>,
    },
}

impl Commander {
    fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<CommanderMessage>,
        queue: mpsc::Sender<SafeCommandRequest>,
        responses: mpsc::Receiver<SafeCommandResponse>,
    ) -> Self {
        Self {
            shutdown,
            receiver,
            queue,
            responses,
            results: HashMap::new(),
        }
    }

    async fn run(&mut self) {
        info!("Commander task is runnning");

        loop {
            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    match msg {
                        CommanderMessage::QueueCommand { action } => {
                            info!("Received command {:?}", action);
                            self.results.insert(action.id, SafeCommandState {
                                state: State::Queued,
                                response: None,
                            });
                            _ = self.queue.send(action).await;
                        }
                        CommanderMessage::GetResults { tx } => {
                            info!("Results size: {}", self.results.len());

                            let results = self.results.values().filter_map(|state| {
                                state.response.clone()
                            }).collect();

                            _ = tx.send(results);

                            // Clear the results that are completed
                            self.results.retain(|_, state| {
                                state.state != State::Completed
                            });
                        }
                        CommanderMessage::QueueResponse { action } => {
                            self.results.insert(action.id, SafeCommandState {
                                state: State::Completed,
                                response: Some(action),
                            });
                        }
                    }
                }
                Some(response) = self.responses.recv() => {
                    let state = self.results.get_mut(&response.id).unwrap();
                    state.state = State::Completed;
                    state.response = Some(response);
                }
                _ = self.shutdown.token.cancelled() => {
                    break;
                }
            }
        }

        info!("Commander task shutting down");
    }
}

#[derive(Clone)]
pub struct CommanderHandle {
    sender: mpsc::Sender<CommanderMessage>,
}

impl CommanderHandle {
    pub fn new(
        shutdown: ShutdownSignals,
        tunnel: TunnelHandle,
        updater: UpdaterHandle,
        downloader: DownloaderHandle,
        filemanager: FileManagerHandle,
        logstream: LogStreamHandle,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(10);
        let (command_queue_tx, command_queue_rx) = mpsc::channel(10);
        let (response_queue_tx, response_queue_rx) = mpsc::channel(10);
        let mut actor = Commander::new(
            shutdown.clone(),
            receiver,
            command_queue_tx,
            response_queue_rx,
        );
        let mut actor2 = CommandQueueExecutor::new(
            shutdown,
            command_queue_rx,
            response_queue_tx,
            tunnel,
            updater,
            downloader,
            filemanager,
            logstream,
        );
        tokio::spawn(async move { actor.run().await });
        tokio::spawn(async move { actor2.run().await });

        Self { sender }
    }

    pub async fn execute_api_batch(&self, commands: Vec<SafeCommandRequest>) {
        for command in commands {
            _ = self
                .sender
                .send(CommanderMessage::QueueCommand { action: command })
                .await;
        }
    }

    pub async fn insert_result(&self, commands: Vec<SafeCommandResponse>) {
        for command in commands {
            _ = self
                .sender
                .send(CommanderMessage::QueueResponse { action: command })
                .await;
        }
    }

    pub async fn get_results(&self) -> Vec<SafeCommandResponse> {
        let (tx, rx) = oneshot::channel();
        _ = self.sender.send(CommanderMessage::GetResults { tx }).await;
        rx.await.unwrap_or_default()
    }
}
