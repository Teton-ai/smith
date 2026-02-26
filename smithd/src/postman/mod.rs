use crate::commander::CommanderHandle;
use crate::magic::MagicHandle;
use crate::police::PoliceHandle;
use crate::shutdown::ShutdownSignals;
use crate::utils::network::NetworkClient;
use crate::utils::schema::{
    DeviceRegistration, DeviceRegistrationResponse, HomePost, HomePostResponse,
    SafeCommandResponse, SafeCommandRx,
};
use crate::utils::system::SystemInfo;
use anyhow::{Result, anyhow};
use reqwest::{Response, StatusCode};
use std::fmt::Write;
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
    network: NetworkClient,
    hostname: String,
    token: Option<String>,
    problems: Option<u32>,
    poll_mode: PollMode,
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
    ) -> Self {
        let network = NetworkClient::default();

        Self {
            shutdown,
            police,
            receiver,
            commander,
            network,
            magic,
            token: None,
            hostname: "".to_owned(),
            problems: None,
            poll_mode: PollMode::Idle,
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

                    let responses = self.commander.get_results().await;
                    let release_id = self.magic.get_release_id().await;

                    let ping_home_body = HomePost::new(responses, release_id);

                    let response = self.ping_home(ping_home_body).await;
                    let target_release_id = response.target_release_id;
                    self.magic.set_target_release_id(target_release_id).await;

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

    async fn ping_home(&mut self, message: HomePost) -> HomePostResponse {
        let token = self.token.clone().unwrap_or_default();

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
                    response.json().await.unwrap_or_default()
                }
                StatusCode::UNAUTHORIZED => {
                    warn!("Token expired, we are going to delete the token");
                    self.unregister_device().await;
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
    ) -> Self {
        let (_sender, receiver) = mpsc::channel(8);
        let mut actor = Postman::new(shutdown, police, receiver, commander, magic);
        tokio::spawn(async move { actor.run().await });

        Self { _sender }
    }
}
