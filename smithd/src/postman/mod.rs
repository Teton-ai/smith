use crate::commander::CommanderHandle;
use crate::magic::MagicHandle;
use crate::police::PoliceHandle;
use crate::session::{RefreshOutcome, SessionHandle};
use crate::shutdown::ShutdownSignals;
use crate::utils::network::NetworkClient;
use crate::utils::schema::{
    DeviceRegistration, DeviceRegistrationResponse, HomePost, HomePostResponse,
    SafeCommandResponse, SafeCommandRx, ServiceCheck, ServiceStatus,
};
use crate::utils::system::SystemInfo;
use anyhow::{Result, anyhow};
use reqwest::{Response, StatusCode};
use std::fmt::Write;
use std::path::PathBuf;
use std::time::Duration;
use tokio::{sync::mpsc, time};
use tracing::{error, info, warn};

enum PollMode {
    Active { ticks_without_commands: u32 },
    Idle,
}

struct Postman {
    shutdown: ShutdownSignals,
    police: PoliceHandle,
    receiver: mpsc::Receiver<PostmanMessage>,
    commander: CommanderHandle,
    magic: MagicHandle,
    session: SessionHandle,
    network: NetworkClient,
    hostname: String,
    token: Option<String>,
    problems: Option<u32>,
    poll_mode: PollMode,
    services_to_check: Vec<ServiceCheck>,
}

#[derive(Debug)]
enum PostmanMessage {}

impl Postman {
    fn new(
        shutdown: ShutdownSignals,
        police: PoliceHandle,
        receiver: mpsc::Receiver<PostmanMessage>,
        commander: CommanderHandle,
        magic: MagicHandle,
        session: SessionHandle,
    ) -> Self {
        let network = NetworkClient::default();

        Self {
            shutdown,
            police,
            receiver,
            commander,
            network,
            magic,
            session,
            token: None,
            hostname: "".to_owned(),
            problems: None,
            poll_mode: PollMode::Idle,
            services_to_check: Vec::new(),
        }
    }

    async fn handle_message(&mut self, _msg: PostmanMessage) {}

    async fn run(&mut self) {
        info!("Postman runnning");

        self.hostname = self.magic.get_server().await;
        self.network.set_hostname(self.hostname.clone());

        self.token = self.magic.get_token().await;
        let mut system_info = SystemInfo::new().await;

        self.commander
            .insert_result(vec![
                SafeCommandResponse {
                    id: -1,
                    command: SafeCommandRx::GetVariables,
                    status: 0,
                },
                SafeCommandResponse {
                    id: -2,
                    command: SafeCommandRx::UpdateSystemInfo {
                        system_info: system_info.to_value(),
                    },
                    status: 0,
                },
                SafeCommandResponse {
                    id: -4,
                    command: SafeCommandRx::GetNetwork,
                    status: 0,
                },
            ])
            .await;

        const IDLE_INTERVAL_SECS: u64 = 20;
        const ACTIVE_INTERVAL_SECS: u64 = 1;
        const IDLE_THRESHOLD_TICKS: u32 = 60;

        let mut keep_alive_interval = time::interval(Duration::from_secs(IDLE_INTERVAL_SECS));
        keep_alive_interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip); // or ::Delay
        let mut update_interval = time::interval(Duration::from_secs(300));

        loop {
            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    _ = self.handle_message(msg).await;
                }
                _ = keep_alive_interval.tick() => {
                    if let Err(e) = self.ensure_token().await {
                        error!("Failed to register device: {}", e);
                        continue;
                    }

                    let mut responses = self.commander.get_results().await;
                    // Bundle any reports captured while offline into this POST;
                    // they're deleted from the queue once it's acknowledged.
                    let (diag_responses, diag_paths) = self.load_queued_diagnostics().await;
                    responses.extend(diag_responses);

                    let release_id = self.magic.get_release_id().await.ok();
                    let service_statuses = self.check_services().await;

                    let ping_home_body = HomePost::new(responses, release_id, service_statuses);

                    let response = self.ping_home(ping_home_body, diag_paths).await;

                    let target_release_id = response.target_release_id;
                    self.services_to_check = response.services;

                    if let Some(target_release_id) = target_release_id {
                        self.magic.set_target_release_id(target_release_id).await;
                    }

                    let has_commands = !response.commands.is_empty();
                    self.commander.execute_api_batch(response.commands).await;

                    if has_commands {
                        if matches!(self.poll_mode, PollMode::Idle) {
                            info!("Switching to active polling mode (1 second interval)");
                            keep_alive_interval = time::interval(Duration::from_secs(ACTIVE_INTERVAL_SECS));
                            keep_alive_interval.reset();
                        }
                        self.poll_mode = PollMode::Active { ticks_without_commands: 0 };
                    } else if let PollMode::Active { ticks_without_commands } = &mut self.poll_mode {
                        *ticks_without_commands += 1;
                        if *ticks_without_commands >= IDLE_THRESHOLD_TICKS {
                            info!("Switching to idle polling mode (20 second interval)");
                            keep_alive_interval = time::interval(Duration::from_secs(IDLE_INTERVAL_SECS));
                            keep_alive_interval.reset();
                            self.poll_mode = PollMode::Idle;
                        }
                    }
                }
                _ = update_interval.tick() => {
                    let new_system_info = SystemInfo::new().await;
                    // Only update the system_info if it has actually changed
                    if new_system_info != system_info {
                        system_info = new_system_info.clone();
                        self.commander
                            .insert_result(vec![
                                // Keep the system info in sync.
                                SafeCommandResponse {
                                    id: -2,
                                    command: SafeCommandRx::UpdateSystemInfo {
                                        system_info: new_system_info.to_value(),
                                    },
                                    status: 0,
                                },
                            ])
                            .await;
                    }
                }
                _ = self.shutdown.token.cancelled() => {
                    break;
                }
            }
        }

        info!("Postman task shut down");
    }

    async fn check_services(&self) -> Vec<ServiceStatus> {
        use futures::stream::{self, StreamExt};

        const MAX_CONCURRENT_PROBES: usize = 4;

        let services = self.services_to_check.clone();
        let statuses: Vec<ServiceStatus> = stream::iter(services)
            .map(|service| async move {
                let id = service.id;
                let name = service.name;
                let result = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    tokio::process::Command::new("systemctl")
                        .args(["show", &name, "--property=ActiveState,NRestarts"])
                        .output(),
                )
                .await;

                match result {
                    Ok(Ok(output)) if output.status.success() => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let mut active_state = String::from("unknown");
                        let mut n_restarts: u32 = 0;
                        for line in stdout.lines() {
                            if let Some(val) = line.strip_prefix("ActiveState=") {
                                active_state = val.to_string();
                            } else if let Some(val) = line.strip_prefix("NRestarts=") {
                                n_restarts = val.parse().unwrap_or(0);
                            }
                        }
                        ServiceStatus {
                            id,
                            active_state,
                            n_restarts,
                        }
                    }
                    Ok(Ok(output)) => {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        error!(
                            "systemctl exited with {} for service {}: {}",
                            output.status, name, stderr
                        );
                        ServiceStatus {
                            id,
                            active_state: "unknown".to_string(),
                            n_restarts: 0,
                        }
                    }
                    Ok(Err(e)) => {
                        error!("Failed to check service {}: {}", name, e);
                        ServiceStatus {
                            id,
                            active_state: "unknown".to_string(),
                            n_restarts: 0,
                        }
                    }
                    Err(_) => {
                        error!("Timeout checking service {}", name);
                        ServiceStatus {
                            id,
                            active_state: "unknown".to_string(),
                            n_restarts: 0,
                        }
                    }
                }
            })
            .buffered(MAX_CONCURRENT_PROBES)
            .collect()
            .await;

        statuses
    }

    async fn ensure_token(&mut self) -> Result<(), anyhow::Error> {
        if self.token.is_none() {
            warn!("!NO TOKEN! trying to register device");

            let response = self
                .register_device(DeviceRegistration {
                    serial_number: self.network.get_serial(),
                    wifi_mac: self.network.get_mac_wlan0(),
                })
                .await?;

            if response.0 == StatusCode::OK {
                let registration_response = response.1.json::<DeviceRegistrationResponse>().await?;
                self.magic.set_token(&registration_response.token).await;
                self.token = Some(registration_response.token);
            } else {
                error!("Failed to register device: {:?}", response.0);
                return Err(anyhow!("Failed to register device"));
            }
        }
        Ok(())
    }

    async fn ping_home(&mut self, message: HomePost, delivered: Vec<PathBuf>) -> HomePostResponse {
        // Prefer the short-lived JWT from session; fall back to the opaque
        // token (which is also what session returns when no JWT is cached).
        let token = self
            .session
            .bearer_token()
            .await
            .or_else(|| self.token.clone())
            .unwrap_or_default();

        let result = self
            .network
            .send_compressed_post(&token, "/home", &message)
            .await;

        match result {
            Ok((status_code, response)) => match status_code {
                StatusCode::OK => {
                    info!("Posting successful");
                    if let Some(problem) = self.problems {
                        self.police.report_problem_solved(problem).await;
                        self.problems = None;
                    };
                    // The queued diagnostic reports rode along in this POST; now
                    // that it's acknowledged, drop them from the upload queue.
                    for path in &delivered {
                        if let Err(err) = crate::netdiag::store::clear_uploaded(path.clone()).await {
                            warn!("Delivered report but failed to remove {}: {err}", path.display());
                        }
                    }
                    response.json().await.unwrap_or_default()
                }
                StatusCode::UNAUTHORIZED => {
                    // Standard access/refresh-token flow: the bearer (probably
                    // an expired JWT) was rejected. Try to mint a new JWT from
                    // the opaque "refresh" token. Only unregister if the
                    // opaque token itself is also rejected.
                    warn!("Got 401 on /home; attempting JWT refresh");
                    match self.session.force_refresh().await {
                        RefreshOutcome::Refreshed => {
                            info!("Refreshed JWT after 401; next ping will retry");
                        }
                        RefreshOutcome::Unauthorized => {
                            warn!("Opaque token rejected by /auth/session; unregistering device");
                            self.unregister_device().await;
                        }
                        RefreshOutcome::Transient => {
                            warn!("JWT refresh failed transiently; keeping token");
                        }
                    }
                    HomePostResponse::default()
                }
                _ => {
                    error!(
                        "Posting failed with status: {:?} {:?}",
                        status_code, response
                    );
                    HomePostResponse::default()
                }
            },
            Err(err) => {
                let mut s = format!("{}", err);
                let mut e = err.source().unwrap();
                while let Some(src) = e.source() {
                    let _ = write!(s, "\n\nCaused by: {}", src);
                    e = src;
                }
                error!("POST FAILURE: {}", s);
                if self.problems.is_none() {
                    self.problems = self.police.report_problem_starting().await;
                }
                HomePostResponse::default()
            }
        }
    }

    /// Load queued diagnostic reports (captured while offline) as `/home`
    /// responses to ride along in the next POST, returning the responses and the
    /// files they came from so they can be deleted once the POST is acknowledged.
    /// Reports that are corrupt on disk are dropped here rather than blocking the
    /// queue forever. Bounded per poll so a backlog can't bloat the keep-alive.
    async fn load_queued_diagnostics(&self) -> (Vec<SafeCommandResponse>, Vec<PathBuf>) {
        const MAX_PER_POLL: usize = 10;
        /// Synthetic ids for queued reports; the backend keys on the report's own
        /// id inside the payload, so these only need to be distinct in the batch.
        const DIAG_ID_BASE: i32 = -100;

        let pending = match crate::netdiag::store::pending_uploads().await {
            Ok(pending) => pending,
            Err(err) => {
                warn!("Could not scan the diagnostics queue: {err:#}");
                return (Vec::new(), Vec::new());
            }
        };
        if pending.is_empty() {
            return (Vec::new(), Vec::new());
        }

        let mut responses = Vec::new();
        let mut paths = Vec::new();
        for (i, path) in pending.into_iter().take(MAX_PER_POLL).enumerate() {
            let bytes = match tokio::fs::read(&path).await {
                Ok(bytes) => bytes,
                Err(err) => {
                    warn!("Could not read queued report {}: {err}", path.display());
                    continue;
                }
            };

            let report: serde_json::Value = match serde_json::from_slice(&bytes) {
                Ok(value) => value,
                Err(err) => {
                    error!(
                        "Queued report {} is not valid JSON ({err}); dropping it",
                        path.display()
                    );
                    if let Err(err) = crate::netdiag::store::clear_uploaded(path.clone()).await {
                        warn!("Failed to remove corrupt report {}: {err}", path.display());
                    }
                    continue;
                }
            };

            responses.push(SafeCommandResponse {
                id: DIAG_ID_BASE - i as i32,
                command: SafeCommandRx::NetworkDiagnosticReport { report },
                status: 0,
            });
            paths.push(path);
        }
        (responses, paths)
    }

    async fn register_device(
        &mut self,
        message: DeviceRegistration,
    ) -> Result<(StatusCode, Response)> {
        let url = String::from("/register");

        let token = self.token.clone().unwrap_or_default();

        self.network
            .send_compressed_post(&token, &url, &message)
            .await
    }

    async fn unregister_device(&mut self) {
        self.token = None;
        self.magic.delete_token().await;
    }
}

#[derive(Clone)]
pub struct PostmanHandle {
    _sender: mpsc::Sender<PostmanMessage>,
}

impl PostmanHandle {
    pub fn new(
        shutdown: ShutdownSignals,
        police: PoliceHandle,
        commander: CommanderHandle,
        magic: MagicHandle,
        session: SessionHandle,
    ) -> Self {
        let (_sender, receiver) = mpsc::channel(8);
        let mut actor = Postman::new(shutdown, police, receiver, commander, magic, session);
        tokio::spawn(async move { actor.run().await });

        Self { _sender }
    }
}
