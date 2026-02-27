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
mod logs;
mod network;
mod ota;
mod restart;
mod tunnel;
mod upgrade;
mod variable;

pub struct Handles {
    pub tunnel: TunnelHandle,
    pub updater: UpdaterHandle,
    pub downloader: DownloaderHandle,
    pub filemanager: FileManagerHandle,
    pub logstream: LogStreamHandle,
}

struct CommandQueueExecutor {
    shutdown: ShutdownSignals,
    queue: mpsc::Receiver<SafeCommandRequest>,
    responses: mpsc::Sender<SafeCommandResponse>,
    handles: Handles,
}

impl CommandQueueExecutor {
    fn new(
        shutdown: ShutdownSignals,
        queue: mpsc::Receiver<SafeCommandRequest>,
        responses: mpsc::Sender<SafeCommandResponse>,
        handles: Handles,
    ) -> Self {
        Self {
            shutdown,
            queue,
            responses,
            handles,
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
            } => tunnel::open_port(action.id, &self.handles.tunnel, port, user, pub_key).await,
            SafeCommandTx::CloseTunnel => tunnel::close_ssh(action.id, &self.handles.tunnel).await,
            SafeCommandTx::Upgrade => upgrade::upgrade(action.id, &self.handles.updater).await,
            SafeCommandTx::UpdateNetwork { network } => network::execute(action.id, network).await,
            SafeCommandTx::DownloadOTA {
                tools,
                payload,
                rate,
            } => {
                ota::download_ota(
                    action.id,
                    &self.handles.downloader,
                    &self.handles.filemanager,
                    &tools,
                    &payload,
                    rate,
                )
                .await
            }
            SafeCommandTx::CheckOTAStatus => {
                ota::check_ota(action.id, &self.handles.downloader).await
            }
            SafeCommandTx::StartOTA => ota::start_ota(action.id, &self.handles.filemanager).await,
            SafeCommandTx::TestNetwork => network::test_network(action.id).await,
            SafeCommandTx::ExtendedNetworkTest { duration_minutes } => {
                network::extended_network_test(action.id, duration_minutes).await
            }
            SafeCommandTx::StreamLogs {
                session_id,
                service_name,
            } => {
                logs::start_stream(action.id, &self.handles.logstream, session_id, service_name)
                    .await
            }
            SafeCommandTx::StopLogStream { session_id } => {
                logs::stop_stream(action.id, &self.handles.logstream, session_id).await
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
    pub fn new(shutdown: ShutdownSignals, handles: Handles) -> Self {
        let (sender, receiver) = mpsc::channel(10);
        let (command_queue_tx, command_queue_rx) = mpsc::channel(10);
        let (response_queue_tx, response_queue_rx) = mpsc::channel(10);
        let mut actor = Commander::new(
            shutdown.clone(),
            receiver,
            command_queue_tx,
            response_queue_rx,
        );
        let mut actor2 =
            CommandQueueExecutor::new(shutdown, command_queue_rx, response_queue_tx, handles);
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
