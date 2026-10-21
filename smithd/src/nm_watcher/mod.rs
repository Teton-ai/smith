use crate::commander::{CommanderHandle, network};
use crate::link_health::DeviceStateEvent;
use crate::settings::SettingsHandle;
use crate::settings::driver::{SYS_CLASS_NET, wifi_interface};
use crate::shutdown::ShutdownSignals;
use futures::StreamExt;
use std::path::Path;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant, sleep, sleep_until};
use tracing::{debug, info, warn};
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

    #[zbus(signal)]
    fn device_added(&self, device_path: OwnedObjectPath) -> zbus::Result<()>;

    #[zbus(signal)]
    fn device_removed(&self, device_path: OwnedObjectPath) -> zbus::Result<()>;

    fn get_device_by_ip_iface(&self, iface: &str) -> zbus::Result<OwnedObjectPath>;
}

/// Own module: its `StateChanged` would clash with the manager's generated types.
mod device {
    #[zbus::proxy(
        interface = "org.freedesktop.NetworkManager.Device",
        default_service = "org.freedesktop.NetworkManager"
    )]
    pub trait Device {
        #[zbus(signal)]
        fn state_changed(&self, new_state: u32, old_state: u32, reason: u32) -> zbus::Result<()>;

        /// The current `(state, reason)`.
        #[zbus(property)]
        fn state_reason(&self) -> zbus::Result<(u32, u32)>;
    }
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
    pub fn new(
        shutdown: ShutdownSignals,
        commander: CommanderHandle,
        settings: SettingsHandle,
        device_events: mpsc::Sender<DeviceStateEvent>,
    ) -> Self {
        tokio::spawn(async move {
            run(shutdown, commander, settings, device_events).await;
        });
        Self
    }
}

async fn run(
    shutdown: ShutdownSignals,
    commander: CommanderHandle,
    settings: SettingsHandle,
    device_events: mpsc::Sender<DeviceStateEvent>,
) {
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
        let mut device_added_stream = setup!(
            nm_proxy.receive_device_added().await,
            "DeviceAdded subscribe"
        );
        let mut device_removed_stream = setup!(
            nm_proxy.receive_device_removed().await,
            "DeviceRemoved subscribe"
        );
        let mut wifi = WifiDevice::default();
        wifi.resolve(&conn, &nm_proxy, &device_events).await;

        // We intentionally do NOT subscribe to Settings.Connection.Updated: nmcli --show-secrets
        // (which execute_report_nm_profiles calls) causes NM to fire Updated on every profile it
        // reads, creating a feedback loop where each report triggers the next one.

        backoff = RETRY_INITIAL;
        info!(
            "NM watcher listening for NetworkManager state, Wi-Fi device state, profile add, and profile remove"
        );

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
                // A driver reload gives the Wi-Fi device a new object path.
                signal = device_added_stream.next() => match signal {
                    None => on_stream_end!("DeviceAdded"),
                    Some(_) => wifi.resolve(&conn, &nm_proxy, &device_events).await,
                },
                signal = device_removed_stream.next() => match signal {
                    None => on_stream_end!("DeviceRemoved"),
                    Some(_) => wifi.resolve(&conn, &nm_proxy, &device_events).await,
                },
                signal = async { wifi.stream.as_mut()?.next().await }, if wifi.stream.is_some() => match signal {
                    None => on_stream_end!("Device StateChanged"),
                    Some(sig) => match sig.args() {
                        Ok(args) => {
                            let event = DeviceStateEvent {
                                new: *args.new_state(),
                                old: *args.old_state(),
                                reason: *args.reason(),
                            };
                            forward(&device_events, event);
                        }
                        Err(e) => warn!("Failed to parse Device StateChanged args: {e}"),
                    },
                },
                _ = sleep_until(deadline), if pending => {
                    pending = false;
                    settings.check_now();
                    let result = network::execute_report_nm_profiles(CMD_ID_REPORT_NM_PROFILES).await;
                    commander.insert_result(vec![result]).await;
                }
            }
        }
    }

    info!("NM watcher shut down");
}

/// The Wi-Fi device link health watches, re-resolved whenever NM adds or
/// removes a device.
#[derive(Default)]
struct WifiDevice {
    resolved: bool,
    path: Option<OwnedObjectPath>,
    stream: Option<device::StateChangedStream>,
}

impl WifiDevice {
    async fn resolve(
        &mut self,
        conn: &Connection,
        nm: &NetworkManagerProxy<'_>,
        device_events: &mpsc::Sender<DeviceStateEvent>,
    ) {
        let path = match wifi_interface(Path::new(SYS_CLASS_NET)).await {
            Some(iface) => match nm.get_device_by_ip_iface(&iface.name).await {
                Ok(path) => Some(path),
                Err(e) => {
                    warn!("NM watcher: no NM device for {}: {e}", iface.name);
                    None
                }
            },
            None => None,
        };
        let same_device = self.resolved && path == self.path;
        let nothing_to_retry = self.stream.is_some() || path.is_none();
        if same_device && nothing_to_retry {
            return;
        }
        self.resolved = true;
        self.stream = None;
        self.path = path.clone();

        let Some(path) = path else {
            info!("NM watcher: no Wi-Fi device; not watching Wi-Fi state");
            return;
        };
        let proxy = match device::DeviceProxy::builder(conn).path(path.clone()) {
            Ok(builder) => builder.build().await,
            Err(e) => Err(e),
        };
        let proxy = match proxy {
            Ok(proxy) => proxy,
            Err(e) => {
                warn!("NM watcher: Device proxy failed: {e}");
                return;
            }
        };
        match proxy.receive_state_changed().await {
            Ok(stream) => {
                info!("NM watcher: watching Wi-Fi device {}", path.as_str());
                self.stream = Some(stream);
            }
            Err(e) => {
                warn!("NM watcher: Device StateChanged subscribe failed: {e}");
                return;
            }
        }

        // A lockout that began before we subscribed (at boot, or while D-Bus
        // was reconnecting) would otherwise never be seen.
        match proxy.state_reason().await {
            Ok((state, reason)) => forward(
                device_events,
                DeviceStateEvent {
                    new: state,
                    old: state,
                    reason,
                },
            ),
            Err(e) => warn!("NM watcher: reading Wi-Fi device StateReason failed: {e}"),
        }
    }
}

fn forward(device_events: &mpsc::Sender<DeviceStateEvent>, event: DeviceStateEvent) {
    if let Err(err) = device_events.try_send(event) {
        debug!("Dropping Wi-Fi device state {event:?}: {err}");
    }
}

#[inline]
fn schedule(deadline: &mut Instant, pending: &mut bool) {
    *deadline = Instant::now() + DEBOUNCE;
    *pending = true;
}
