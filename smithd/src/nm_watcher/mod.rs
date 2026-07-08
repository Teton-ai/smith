use crate::commander::{CommanderHandle, network};
use crate::shutdown::ShutdownSignals;
use futures::StreamExt;
use tokio::time::{Duration, Instant, sleep, sleep_until};
use tracing::{info, warn};
use zbus::Connection;
use zbus::zvariant::OwnedObjectPath;

const CMD_ID_REPORT_NM_PROFILES: i32 = -6;
const DEBOUNCE: Duration = Duration::from_secs(2);

// NM fires DISCONNECTING(30) and CONNECTING(40) as transient steps; skip them
// to avoid triggering a report mid-transition. All other states are stable enough.
const NM_STATE_DISCONNECTING: u32 = 30;
const NM_STATE_CONNECTING: u32 = 40;

const RETRY_INITIAL: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(60);

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager"
)]
trait NetworkManager {
    #[zbus(signal)]
    fn state_changed(&self, state: u32) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager.Settings",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager/Settings"
)]
trait NetworkManagerSettings {
    #[zbus(signal)]
    fn new_connection(&self, connection: OwnedObjectPath) -> zbus::Result<()>;

    #[zbus(signal)]
    fn connection_removed(&self, connection: OwnedObjectPath) -> zbus::Result<()>;
}

pub struct NMWatcherHandle;

impl NMWatcherHandle {
    pub fn new(shutdown: ShutdownSignals, commander: CommanderHandle) -> Self {
        tokio::spawn(async move {
            run(shutdown, commander).await;
        });
        Self
    }
}

async fn run(shutdown: ShutdownSignals, commander: CommanderHandle) {
    let mut backoff = RETRY_INITIAL;

    // Carry pending debounce state across reconnects.
    let mut deadline: Instant = Instant::now();
    let mut pending = false;

    'reconnect: loop {
        if shutdown.token.is_cancelled() {
            break;
        }

        // Macro for setup steps: on error, wait backoff then retry the outer loop.
        // Needs to be a macro so `continue 'reconnect` applies to the outer loop.
        macro_rules! setup {
            ($expr:expr, $label:literal) => {
                match $expr {
                    Ok(v) => v,
                    Err(e) => {
                        warn!(
                            "NM watcher: {} failed: {e:#}; retrying in {backoff:?}",
                            $label
                        );
                        tokio::select! {
                            biased;
                            _ = shutdown.token.cancelled() => break 'reconnect,
                            _ = sleep(backoff) => {}
                        }
                        backoff = (backoff * 2).min(RETRY_MAX);
                        continue 'reconnect;
                    }
                }
            };
        }

        // Macro for stream-end handling inside the inner loop.
        macro_rules! on_stream_end {
            ($why:literal) => {{
                warn!(
                    "NM watcher: {} stream ended; reconnecting in {backoff:?}",
                    $why
                );
                tokio::select! {
                    biased;
                    _ = shutdown.token.cancelled() => break 'reconnect,
                    _ = sleep(backoff) => {}
                }
                backoff = (backoff * 2).min(RETRY_MAX);
                continue 'reconnect;
            }};
        }

        let conn = setup!(Connection::system().await, "D-Bus connect");
        let nm_proxy = setup!(NetworkManagerProxy::new(&conn).await, "NM proxy");
        let settings_proxy = setup!(
            NetworkManagerSettingsProxy::new(&conn).await,
            "Settings proxy"
        );
        let mut state_stream = setup!(
            nm_proxy.receive_state_changed().await,
            "StateChanged subscribe"
        );
        let mut new_conn_stream = setup!(
            settings_proxy.receive_new_connection().await,
            "NewConnection subscribe"
        );
        let mut conn_removed_stream = setup!(
            settings_proxy.receive_connection_removed().await,
            "ConnectionRemoved subscribe"
        );

        // We intentionally do NOT subscribe to Settings.Connection.Updated: nmcli --show-secrets
        // (which execute_report_nm_profiles calls) causes NM to fire Updated on every profile it
        // reads, creating a feedback loop where each report triggers the next one.

        backoff = RETRY_INITIAL;
        info!("NM watcher listening for NetworkManager state, profile add, and profile remove");

        loop {
            tokio::select! {
                biased;
                _ = shutdown.token.cancelled() => break 'reconnect,
                signal = state_stream.next() => match signal {
                    None => on_stream_end!("StateChanged"),
                    Some(sig) => match sig.args() {
                        Ok(args) => {
                            let state = *args.state();
                            if state != NM_STATE_DISCONNECTING && state != NM_STATE_CONNECTING {
                                info!("NM state -> {state}, scheduling profile report");
                                schedule(&mut deadline, &mut pending);
                            }
                        }
                        Err(e) => warn!("Failed to parse NM StateChanged args: {e}"),
                    },
                },
                signal = new_conn_stream.next() => match signal {
                    None => on_stream_end!("NewConnection"),
                    Some(_) => {
                        info!("NM profile added, scheduling profile report");
                        schedule(&mut deadline, &mut pending);
                    }
                },
                signal = conn_removed_stream.next() => match signal {
                    None => on_stream_end!("ConnectionRemoved"),
                    Some(_) => {
                        info!("NM profile removed, scheduling profile report");
                        schedule(&mut deadline, &mut pending);
                    }
                },
                _ = sleep_until(deadline), if pending => {
                    pending = false;
                    let result = network::execute_report_nm_profiles(CMD_ID_REPORT_NM_PROFILES).await;
                    commander.insert_result(vec![result]).await;
                }
            }
        }
    }

    info!("NM watcher shut down");
}

#[inline]
fn schedule(deadline: &mut Instant, pending: &mut bool) {
    *deadline = Instant::now() + DEBOUNCE;
    *pending = true;
}
