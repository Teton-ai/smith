use crate::auditor::run_audit_checks;
use crate::downloader::DownloaderHandle;
use crate::filebrowser::FileBrowserHandle;
use crate::filemanager::FileManagerHandle;
use crate::logstream::LogStreamHandle;
use crate::magic::MagicHandle;
use crate::settings;
use crate::shutdown::ShutdownSignals;
use crate::tunnel::TunnelHandle;
use crate::updater::UpdaterHandle;
use crate::utils::net_guard::NetGuard;
use crate::utils::schema::{SafeCommandRequest, SafeCommandResponse, SafeCommandRx, SafeCommandTx};
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, warn};

mod files;
mod free;
mod logs;
pub(crate) mod network;
mod ota;
mod restart;
mod tunnel;
mod upgrade;
mod variable;

pub struct Handles {
    pub magic: MagicHandle,
    pub tunnel: TunnelHandle,
    pub updater: UpdaterHandle,
    pub downloader: DownloaderHandle,
    pub filemanager: FileManagerHandle,
    pub logstream: LogStreamHandle,
    pub filebrowser: FileBrowserHandle,
    pub net_guard: NetGuard,
}

/// Commands that reconfigure or reconnect Wi-Fi. A scan is included because
/// its directed probes take the radio off channel.
fn changes_network(command: &SafeCommandTx) -> bool {
    matches!(
        command,
        SafeCommandTx::UpdateNetwork { .. }
            | SafeCommandTx::ApplyNetworks { .. }
            | SafeCommandTx::WifiScan
    )
}

/// Runs one command. The seam between the executor loop and the device, so
/// the tests can run the commander without real handles.
trait Execute {
    fn execute(
        &mut self,
        action: SafeCommandRequest,
    ) -> impl Future<Output = SafeCommandResponse> + Send;
}

impl Execute for Handles {
    async fn execute(&mut self, action: SafeCommandRequest) -> SafeCommandResponse {
        let _network_busy = changes_network(&action.command).then(|| self.net_guard.hold());
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
            } => tunnel::open_port(action.id, &self.tunnel, port, user, pub_key).await,
            SafeCommandTx::CloseTunnel => tunnel::close_ssh(action.id, &self.tunnel).await,
            SafeCommandTx::Upgrade => upgrade::upgrade(action.id, &self.updater).await,
            SafeCommandTx::UpdateNetwork { network } => network::execute(action.id, network).await,
            SafeCommandTx::DownloadOTA {
                tools,
                payload,
                rate,
            } => {
                ota::download_ota(
                    action.id,
                    &self.downloader,
                    &self.filemanager,
                    &tools,
                    &payload,
                    rate,
                )
                .await
            }
            SafeCommandTx::CheckOTAStatus => ota::check_ota(action.id, &self.downloader).await,
            SafeCommandTx::StartOTA => ota::start_ota(action.id, &self.filemanager).await,
            SafeCommandTx::TestNetwork => {
                let server = self.magic.get_server().await;
                network::test_network(action.id, &server).await
            }
            SafeCommandTx::ExtendedNetworkTest { duration_minutes } => {
                let server = self.magic.get_server().await;
                network::extended_network_test(action.id, duration_minutes, &server).await
            }
            SafeCommandTx::StreamLogs {
                session_id,
                service_name,
            } => logs::start_stream(action.id, &self.logstream, session_id, service_name).await,
            SafeCommandTx::StopLogStream { session_id } => {
                logs::stop_stream(action.id, &self.logstream, session_id).await
            }
            SafeCommandTx::GetLogs {
                unit,
                since,
                until,
                grep,
            } => free::execute_get_logs(action.id, unit, since, until, grep).await,
            SafeCommandTx::RunAudit => {
                let (disk_encrypted, password_access_disabled) = run_audit_checks().await;
                SafeCommandResponse {
                    id: action.id,
                    command: SafeCommandRx::AuditReport {
                        disk_encrypted,
                        password_access_disabled,
                    },
                    status: 0,
                }
            }
            SafeCommandTx::ReportNMProfiles => network::execute_report_nm_profiles(action.id).await,
            SafeCommandTx::WifiScan => network::execute_wifi_scan(action.id).await,
            SafeCommandTx::ApplyNetworks { version, networks } => {
                network::execute_apply_networks(action.id, version, networks).await
            }
            SafeCommandTx::OpenFileSession { session_id } => {
                files::open_session(action.id, &self.filebrowser, session_id).await
            }
            SafeCommandTx::CloseFileSession { session_id } => {
                files::close_session(action.id, &self.filebrowser, session_id).await
            }
            SafeCommandTx::ReportSettings => match settings::read_now().await {
                Some(report) => SafeCommandResponse {
                    id: action.id,
                    command: SafeCommandRx::ReportSettings {
                        report: Box::new(report),
                    },
                    status: 0,
                },
                None => SafeCommandResponse {
                    id: action.id,
                    command: SafeCommandRx::ReportSettings {
                        report: Default::default(),
                    },
                    status: -1,
                },
            },
            // Understood but not applied by this build yet: answered like an
            // unknown command, so the command history shows why nothing changed.
            SafeCommandTx::ApplySettings { version, .. } => {
                warn!(
                    command_id = action.id,
                    version, "ApplySettings is not supported by this smithd yet; upgrade smithd"
                );
                SafeCommandResponse {
                    id: action.id,
                    command: SafeCommandRx::Unknown,
                    status: -1,
                }
            }
            // Issued by a newer api than this daemon understands. Report a
            // failure so the operator sees why the command did nothing instead
            // of it silently disappearing.
            SafeCommandTx::Unknown => {
                warn!(
                    command_id = action.id,
                    "Received a command this daemon does not understand; upgrade smithd"
                );
                SafeCommandResponse {
                    id: action.id,
                    command: SafeCommandRx::Unknown,
                    status: -1,
                }
            }
        }
    }
}

/// Runs the queued commands one at a time. The channels to and from the
/// commander are unbounded, so neither actor ever waits on the other. Bounded
/// in both directions, a large batch fills both and blocks the two for ever.
struct CommandQueueExecutor<E> {
    shutdown: ShutdownSignals,
    queue: mpsc::UnboundedReceiver<SafeCommandRequest>,
    responses: mpsc::UnboundedSender<SafeCommandResponse>,
    executor: E,
}

impl<E: Execute> CommandQueueExecutor<E> {
    async fn run(&mut self) {
        loop {
            tokio::select! {
                Some(command) = self.queue.recv() => {
                    let response = self.executor.execute(command).await;
                    if let Err(err) = self.responses.send(response) {
                        warn!(command_id = err.0.id, "Commander stopped; result dropped");
                    }
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

impl SafeCommandState {
    fn queued() -> Self {
        Self {
            state: State::Queued,
            response: None,
        }
    }

    fn completed(response: SafeCommandResponse) -> Self {
        Self {
            state: State::Completed,
            response: Some(response),
        }
    }
}

/// Bounds results kept while posts fail. No commands arrive during such an outage,
/// so only a burst followed by a long outage reaches the bound.
const MAX_KEPT_RESULTS: usize = 100;

/// Never awaits inside its loop, so every sender to its bounded inbox
/// (postman, settings, auditor, nm_watcher) always gets through.
struct Commander {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<CommanderMessage>,
    queue: mpsc::UnboundedSender<SafeCommandRequest>,
    responses: mpsc::UnboundedReceiver<SafeCommandResponse>,
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
    RequeueResults {
        actions: Vec<SafeCommandResponse>,
    },
}

/// Puts back command results whose post failed. An entry already under the
/// same id is newer (a re-sent command still running) and wins.
///
/// Only command results (positive ids) are kept: nothing else produces them
/// again. Reports (negative ids) are dropped; the api has often stored them
/// already, and resending them on each failed post multiplied the load.
fn requeue_results(
    results: &mut HashMap<i32, SafeCommandState>,
    actions: Vec<SafeCommandResponse>,
) {
    let mut dropped = Vec::new();
    for action in actions {
        if action.id <= 0 {
            dropped.push(action.id);
            continue;
        }
        results
            .entry(action.id)
            .or_insert_with(|| SafeCommandState::completed(action));
    }
    if !dropped.is_empty() {
        warn!(ids = ?dropped, "Post failed; reports not sent again");
    }
    // Command ids come from a sequence, so the lowest is the oldest command.
    // Reports queued since the failed post (negative ids) are replaced by
    // their next version; they go last.
    let mut completed: Vec<i32> = results
        .iter()
        .filter(|(_, state)| state.state == State::Completed)
        .map(|(id, _)| *id)
        .collect();
    let excess = completed.len().saturating_sub(MAX_KEPT_RESULTS);
    if excess == 0 {
        return;
    }
    completed.sort_unstable_by_key(|id| (*id < 0, *id));
    for id in completed.into_iter().take(excess) {
        results.remove(&id);
        error!(
            command_id = id,
            "More than {MAX_KEPT_RESULTS} results waiting for the api; dropped the oldest"
        );
    }
}

impl Commander {
    fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<CommanderMessage>,
        queue: mpsc::UnboundedSender<SafeCommandRequest>,
        responses: mpsc::UnboundedReceiver<SafeCommandResponse>,
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
                            self.results.insert(action.id, SafeCommandState::queued());
                            if let Err(err) = self.queue.send(action) {
                                error!(command_id = err.0.id, "Command executor stopped; command not run");
                            }
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
                            self.results.insert(action.id, SafeCommandState::completed(action));
                        }
                        CommanderMessage::RequeueResults { actions } => {
                            requeue_results(&mut self.results, actions);
                        }
                    }
                }
                Some(response) = self.responses.recv() => {
                    // Inserted rather than looked up: the entry is always there
                    // today, and a missing one must not panic the commander.
                    self.results.insert(response.id, SafeCommandState::completed(response));
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
        Self::spawn(shutdown, handles)
    }

    fn spawn<E: Execute + Send + 'static>(shutdown: ShutdownSignals, executor: E) -> Self {
        let (sender, receiver) = mpsc::channel(10);
        let (queue_tx, queue_rx) = mpsc::unbounded_channel();
        let (responses_tx, responses_rx) = mpsc::unbounded_channel();
        let mut commander = Commander::new(shutdown.clone(), receiver, queue_tx, responses_rx);
        let mut executor = CommandQueueExecutor {
            shutdown,
            queue: queue_rx,
            responses: responses_tx,
            executor,
        };
        tokio::spawn(async move { commander.run().await });
        tokio::spawn(async move { executor.run().await });

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

    /// Results taken by `get_results` whose post failed, sent on the next poll.
    pub async fn requeue_results(&self, actions: Vec<SafeCommandResponse>) {
        if actions.is_empty() {
            return;
        }
        let count = actions.len();
        if self
            .sender
            .send(CommanderMessage::RequeueResults { actions })
            .await
            .is_err()
        {
            error!("Commander stopped; {count} results whose post failed are lost");
        }
    }

    pub async fn get_results(&self) -> Vec<SafeCommandResponse> {
        let (tx, rx) = oneshot::channel();
        _ = self.sender.send(CommanderMessage::GetResults { tx }).await;
        rx.await.unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn network_commands_hold_the_guard() {
        assert!(changes_network(&SafeCommandTx::WifiScan));
        assert!(changes_network(&SafeCommandTx::ApplyNetworks {
            version: 1,
            networks: Vec::new(),
        }));
        assert!(!changes_network(&SafeCommandTx::Ping));
        assert!(!changes_network(&SafeCommandTx::ReportNMProfiles));
        assert!(!changes_network(&SafeCommandTx::TestNetwork));
    }

    fn pong(id: i32, status: i32) -> SafeCommandResponse {
        SafeCommandResponse {
            id,
            command: SafeCommandRx::Pong,
            status,
        }
    }

    fn responses(results: &HashMap<i32, SafeCommandState>) -> Vec<(i32, i32)> {
        let mut responses: Vec<_> = results
            .values()
            .filter_map(|state| state.response.as_ref().map(|r| (r.id, r.status)))
            .collect();
        responses.sort_unstable();
        responses
    }

    #[test]
    fn a_requeued_result_is_sent_again() {
        let mut results = HashMap::new();
        requeue_results(&mut results, vec![pong(7, 0)]);
        assert_eq!(responses(&results), [(7, 0)]);
        assert_eq!(results.get(&7).map(|s| &s.state), Some(&State::Completed));
    }

    #[test]
    fn a_newer_result_under_the_same_id_wins_over_the_requeued_one() {
        let mut results = HashMap::from([(7, SafeCommandState::completed(pong(7, 0)))]);
        requeue_results(&mut results, vec![pong(7, 1)]);
        assert_eq!(responses(&results), [(7, 0)]);
    }

    #[test]
    fn a_report_whose_post_failed_is_dropped_and_a_fresh_one_kept() {
        let mut results = HashMap::from([(-6, SafeCommandState::completed(pong(-6, 0)))]);
        requeue_results(&mut results, vec![pong(-1, 0), pong(-6, 1), pong(7, 0)]);
        assert_eq!(responses(&results), [(-6, 0), (7, 0)]);
    }

    #[test]
    fn a_running_command_is_not_marked_done_by_a_requeued_result() {
        let mut results = HashMap::from([(7, SafeCommandState::queued())]);
        requeue_results(&mut results, vec![pong(7, 0)]);
        assert_eq!(results.get(&7).map(|s| &s.state), Some(&State::Queued));
        assert!(responses(&results).is_empty());
    }

    #[test]
    fn past_the_bound_the_oldest_commands_are_dropped_before_reports_and_running_commands() {
        // A running command and a report queued since the failed post.
        let mut results = HashMap::from([
            (0, SafeCommandState::queued()),
            (-6, SafeCommandState::completed(pong(-6, 0))),
        ]);
        // One over the bound once the report is counted, in no particular order.
        let actions: Vec<_> = (1..=MAX_KEPT_RESULTS)
            .rev()
            .map(|id| pong(id as i32, 0))
            .collect();
        requeue_results(&mut results, actions);
        assert!(
            !results.contains_key(&1),
            "the lowest command id is dropped"
        );
        assert!(results.contains_key(&-6), "the report is kept");
        assert_eq!(results.get(&0).map(|s| &s.state), Some(&State::Queued));
        assert_eq!(results.len(), MAX_KEPT_RESULTS + 1);
    }

    /// Answers every command with a pong, yielding first so the queue and
    /// the results pile up the way slow device commands make them.
    struct Pong;

    impl Execute for Pong {
        async fn execute(&mut self, action: SafeCommandRequest) -> SafeCommandResponse {
            tokio::task::yield_now().await;
            pong(action.id, 0)
        }
    }

    #[tokio::test]
    async fn a_large_batch_runs_to_the_end_and_its_results_are_collected() {
        const COMMANDS: i32 = 100;
        let shutdown = crate::shutdown::ShutdownHandler::new();
        let commander = CommanderHandle::spawn(shutdown.signals(), Pong);
        let batch = (1..=COMMANDS)
            .map(|id| SafeCommandRequest {
                id,
                command: SafeCommandTx::Ping,
                continue_on_error: false,
            })
            .collect();

        let collect = async {
            commander.execute_api_batch(batch).await;
            let mut ids = Vec::new();
            while ids.len() < COMMANDS as usize {
                ids.extend(commander.get_results().await.into_iter().map(|r| r.id));
                tokio::task::yield_now().await;
            }
            ids
        };
        let mut ids = tokio::time::timeout(Duration::from_secs(10), collect)
            .await
            .unwrap_or_else(|_| panic!("commander deadlocked"));

        ids.sort_unstable();
        assert_eq!(ids, (1..=COMMANDS).collect::<Vec<_>>());
    }
}
