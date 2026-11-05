use crate::commander::{CommanderHandle, network};
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
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use tokio::time;
use tracing::{error, info, warn};

/// When the api last answered a home poll, with any status; `None` until it
/// first does.
pub type LastPoll = watch::Receiver<Option<time::Instant>>;

const CMD_ID_GET_VARIABLES: i32 = -1;
const CMD_ID_UPDATE_SYSTEM_INFO: i32 = -2;
const CMD_ID_GET_NETWORK: i32 = -4;
const CMD_ID_REPORT_NM_PROFILES: i32 = -6;
const BACKOFF_BASE: Duration = Duration::from_secs(20);
// The retry wait stays below the unit's 10 min WatchdogSec.
const BACKOFF_CAP: Duration = Duration::from_secs(5 * 60);
// A reachable API gets time to recover before reboot escalation starts.
const TCP_WORKS_GRACE: Duration = Duration::from_secs(60 * 60);

/// Returns an update for the API when the system info changed since the last report.
async fn refresh_system_info(system_info: &mut SystemInfo) -> Option<SafeCommandResponse> {
    let new_system_info = SystemInfo::new().await;
    if new_system_info == *system_info {
        return None;
    }
    let update = SafeCommandResponse {
        id: CMD_ID_UPDATE_SYSTEM_INFO,
        command: SafeCommandRx::UpdateSystemInfo {
            system_info: new_system_info.to_value(),
        },
        status: 0,
    };
    *system_info = new_system_info;
    Some(update)
}

/// Tells systemd the poll loop is alive. The unit's `WatchdogSec` restarts
/// smithd when the loop stops, since the rest of the daemon keeps running and
/// the police never sees a problem. No-op outside systemd.
fn feed_watchdog() {
    if let Some(socket) = std::env::var_os("NOTIFY_SOCKET")
        && let Err(err) = notify_systemd(&socket, "WATCHDOG=1")
    {
        warn!("Could not feed the systemd watchdog: {err}");
    }
}

/// `socket` is a path, or an abstract name when it starts with `@`.
fn notify_systemd(socket: &std::ffi::OsStr, state: &str) -> std::io::Result<()> {
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::net::{SocketAddr, UnixDatagram};

    let addr = match socket.as_bytes().strip_prefix(b"@") {
        Some(name) => SocketAddr::from_abstract_name(name)?,
        None => SocketAddr::from_pathname(socket)?,
    };
    UnixDatagram::unbound()?.send_to_addr(state.as_bytes(), &addr)?;
    Ok(())
}

/// Adds `update` in place of any result with its id. Results put back after a
/// failed post can hold an older snapshot under the same synthetic id, and
/// another failure would keep only the first one, the stale one.
fn push_latest(responses: &mut Vec<SafeCommandResponse>, update: SafeCommandResponse) {
    responses.retain(|response| response.id != update.id);
    responses.push(update);
}

/// Equal jitter under an exponential ceiling; Retry-After shares the five-minute cap.
fn backoff(failed_posts: u32, retry_after: Option<Duration>, jitter: f64) -> Duration {
    let ceiling = BACKOFF_BASE
        .saturating_mul(1 << failed_posts.saturating_sub(1).min(8))
        .min(BACKOFF_CAP);
    let wait = ceiling / 2 + (ceiling / 2).mul_f64(jitter);
    wait.max(retry_after.unwrap_or_default().min(BACKOFF_CAP))
}

pub(crate) async fn can_connect_to_api(hostname: &str) -> bool {
    let Ok(url) = url::Url::parse(hostname) else {
        return false;
    };
    let (Some(host), Some(port)) = (url.host_str(), url.port_or_known_default()) else {
        return false;
    };
    time::timeout(
        Duration::from_secs(3),
        tokio::net::TcpStream::connect((host, port)),
    )
    .await
    .is_ok_and(|connection| connection.is_ok())
}

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
    failing_since: Option<Instant>,
    poll_mode: PollMode,
    services_to_check: Vec<ServiceCheck>,
    last_poll: watch::Sender<Option<time::Instant>>,
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
        last_poll: watch::Sender<Option<time::Instant>>,
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
            failing_since: None,
            poll_mode: PollMode::Idle,
            services_to_check: Vec::new(),
            last_poll,
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
                    id: CMD_ID_GET_VARIABLES,
                    command: SafeCommandRx::GetVariables,
                    status: 0,
                },
                SafeCommandResponse {
                    id: CMD_ID_UPDATE_SYSTEM_INFO,
                    command: SafeCommandRx::UpdateSystemInfo {
                        system_info: system_info.to_value(),
                    },
                    status: 0,
                },
                SafeCommandResponse {
                    id: CMD_ID_GET_NETWORK,
                    command: SafeCommandRx::GetNetwork,
                    status: 0,
                },
            ])
            .await;

        let commander = self.commander.clone();
        tokio::spawn(async move {
            let nm_profiles = network::execute_report_nm_profiles(CMD_ID_REPORT_NM_PROFILES).await;
            commander.insert_result(vec![nm_profiles]).await;
        });

        const IDLE_INTERVAL_SECS: u64 = 20;
        const ACTIVE_INTERVAL_SECS: u64 = 1;
        const IDLE_THRESHOLD_TICKS: u32 = 60;

        // Jittered so a fleet-wide restart does not post in lockstep.
        let keep_alive = time::sleep(BACKOFF_BASE.mul_f64(rand::random::<f64>()));
        tokio::pin!(keep_alive);
        let mut failed_posts: u32 = 0;
        let mut update_interval = time::interval(Duration::from_secs(300));

        loop {
            tokio::select! {
                // Biased so shutdown wins: while posts fail, the next tick is
                // ready after each one, and an unbiased pick kept polling for
                // several posts after SIGTERM.
                biased;
                _ = self.shutdown.token.cancelled() => {
                    break;
                }
                Some(msg) = self.receiver.recv() => {
                    _ = self.handle_message(msg).await;
                }
                () = &mut keep_alive => {
                    feed_watchdog();
                    if let Err(e) = self.ensure_token().await {
                        error!("Failed to register device: {}", e);
                        feed_watchdog();
                        failed_posts = failed_posts.saturating_add(1);
                        keep_alive.as_mut().reset(time::Instant::now() + backoff(failed_posts, None, rand::random()));
                        continue;
                    }

                    let mut responses = self.commander.get_results().await;
                    // A command can change what system info reports, such as the operational
                    // mode, so send it with the command result instead of on the next refresh.
                    if responses.iter().any(|response| response.id > 0)
                        && let Some(update) = refresh_system_info(&mut system_info).await
                    {
                        push_latest(&mut responses, update);
                    }

                    let release_id = self.magic.get_release_id().await.ok();
                    let service_statuses = self.check_services().await;

                    let ping_home_body = HomePost::new(responses, release_id, service_statuses);

                    let response = match self.ping_home(&ping_home_body).await {
                        Ok(response) => response,
                        Err(retry_after) => {
                            // get_results already drained these, including commands
                            // that finished during the outage; requeue or they are lost.
                            self.commander.requeue_results(ping_home_body.responses).await;
                            // Polling work and the retry wait must not share one watchdog interval.
                            feed_watchdog();
                            failed_posts = failed_posts.saturating_add(1);
                            keep_alive.as_mut().reset(time::Instant::now() + backoff(failed_posts, retry_after, rand::random()));
                            continue;
                        }
                    };
                    failed_posts = 0;

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
                        }
                        self.poll_mode = PollMode::Active { ticks_without_commands: 0 };
                    } else if let PollMode::Active { ticks_without_commands } = &mut self.poll_mode {
                        *ticks_without_commands += 1;
                        if *ticks_without_commands >= IDLE_THRESHOLD_TICKS {
                            info!("Switching to idle polling mode (20 second interval)");
                            self.poll_mode = PollMode::Idle;
                        }
                    }
                    let wait = match self.poll_mode {
                        PollMode::Active { .. } => ACTIVE_INTERVAL_SECS,
                        PollMode::Idle => IDLE_INTERVAL_SECS,
                    };
                    keep_alive.as_mut().reset(time::Instant::now() + Duration::from_secs(wait));
                }
                _ = update_interval.tick() => {
                    if let Some(update) = refresh_system_info(&mut system_info).await {
                        self.commander.insert_result(vec![update]).await;
                    }
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

    /// Failed delivery returns Retry-After when available; command results still need sending.
    async fn ping_home(
        &mut self,
        message: &HomePost,
    ) -> Result<HomePostResponse, Option<Duration>> {
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
            .send_compressed_post(&token, "/home", message)
            .await;

        // Any HTTP answer, even an error status, proves the link works.
        // Also what a settings apply waits for: an overloaded api answering
        // 503 must not revert settings that kept the link up.
        if result.is_ok() {
            self.failing_since = None;
            self.last_poll.send_replace(Some(time::Instant::now()));
            if let Some(problem) = self.problems.take() {
                self.police.report_problem_solved(problem).await;
            }
        }

        match result {
            Ok((status_code, response)) => match status_code {
                StatusCode::OK => {
                    info!("Posting successful");
                    // The api stored the results even if its answer does not parse.
                    Ok(response.json().await.unwrap_or_else(|err| {
                        warn!("Could not parse the /home answer, running no commands: {err}");
                        HomePostResponse::default()
                    }))
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
                    Err(None)
                }
                _ => {
                    error!(
                        "Posting failed with status: {:?} {:?}",
                        status_code, response
                    );
                    // Absent on older api builds, which fall back to plain backoff.
                    let retry_after = (status_code == StatusCode::SERVICE_UNAVAILABLE)
                        .then(|| response.headers().get(reqwest::header::RETRY_AFTER))
                        .flatten()
                        .and_then(|value| value.to_str().ok()?.trim().parse().ok())
                        .map(Duration::from_secs);
                    Err(retry_after)
                }
            },
            Err(err) => {
                error!("POST FAILURE: {err:#}");
                let now = Instant::now();
                let failing_for = now.duration_since(*self.failing_since.get_or_insert(now));
                let link_works =
                    failing_for <= TCP_WORKS_GRACE && can_connect_to_api(&self.hostname).await;
                if link_works {
                    if let Some(problem) = self.problems.take() {
                        warn!("api unreachable over HTTP but TCP connect works, disarming Police");
                        self.police.report_problem_solved(problem).await;
                    }
                } else if self.problems.is_none() {
                    self.problems = self.police.report_problem_starting().await;
                }
                Err(None)
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
    last_poll: LastPoll,
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
        let (poll_sender, last_poll) = watch::channel(None);
        let mut actor = Postman::new(
            shutdown,
            police,
            receiver,
            commander,
            magic,
            session,
            poll_sender,
        );
        tokio::spawn(async move { actor.run().await });

        Self { _sender, last_poll }
    }

    /// Follows the time the api last answered a home poll.
    pub fn last_poll(&self) -> LastPoll {
        self.last_poll.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::schema::SafeCommandRx;

    fn pong(id: i32, status: i32) -> SafeCommandResponse {
        SafeCommandResponse {
            id,
            command: SafeCommandRx::Pong,
            status,
        }
    }

    #[test]
    fn a_fresh_snapshot_replaces_the_one_put_back_after_a_failed_post() {
        let mut responses = vec![pong(CMD_ID_UPDATE_SYSTEM_INFO, 1), pong(7, 0)];
        push_latest(&mut responses, pong(CMD_ID_UPDATE_SYSTEM_INFO, 0));
        let ids: Vec<_> = responses.iter().map(|r| (r.id, r.status)).collect();
        assert_eq!(ids, [(7, 0), (CMD_ID_UPDATE_SYSTEM_INFO, 0)]);
    }

    #[test]
    fn backoff_doubles_to_the_cap_and_honours_retry_after() {
        let secs = Duration::from_secs;
        let cases = [
            ("first", 1, None, 0.999, Duration::from_millis(19990)),
            ("third, mid jitter", 3, None, 0.5, secs(60)),
            ("capped", 50, None, 0.5, secs(225)),
            ("retry-after floor", 1, Some(secs(60)), 0.0, secs(60)),
            ("retry-after capped", 1, Some(secs(3600)), 0.0, BACKOFF_CAP),
            (
                "jitter beats retry-after",
                5,
                Some(secs(10)),
                0.5,
                secs(225),
            ),
        ];
        for (name, failed_posts, retry_after, jitter, want) in cases {
            let got = backoff(failed_posts, retry_after, jitter);
            assert_eq!(got, want, "{name}");
        }
    }

    fn received(socket: &std::os::unix::net::UnixDatagram) -> String {
        let mut buf = [0; 64];
        let len = socket.recv(&mut buf).unwrap_or_default();
        String::from_utf8_lossy(&buf[..len]).into_owned()
    }

    #[test]
    fn the_watchdog_is_fed_on_a_path_and_an_abstract_socket() {
        use std::os::linux::net::SocketAddrExt;
        use std::os::unix::net::{SocketAddr, UnixDatagram};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notify");
        let socket = UnixDatagram::bind(&path).unwrap();
        notify_systemd(path.as_os_str(), "WATCHDOG=1").unwrap();
        assert_eq!(received(&socket), "WATCHDOG=1");

        let name = format!("smithd-test-{}", std::process::id());
        let addr = SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
        let socket = UnixDatagram::bind_addr(&addr).unwrap();
        notify_systemd(format!("@{name}").as_ref(), "WATCHDOG=1").unwrap();
        assert_eq!(received(&socket), "WATCHDOG=1");
    }
}
