use crate::shutdown::ShutdownSignals;
use crate::utils::network::{NetworkStats, get_primary_interface_name, read_network_stats};
use crate::utils::schema::NetworkMetrics;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info};

struct NetworkMonitor {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<NetworkMonitorMessage>,
    last_stats: Option<NetworkStats>,
    last_check: Option<tokio::time::Instant>,
    interface: String,
}

enum NetworkMonitorMessage {
    GetMetrics {
        sender: oneshot::Sender<NetworkMetrics>,
    },
}

impl NetworkMonitor {
    async fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<NetworkMonitorMessage>,
    ) -> Self {
        let interface = get_primary_interface_name()
            .await
            .inspect_err(|e| error!("Failed to get primary interface name: {}", e))
            .unwrap_or_else(|_| "eth0".to_string());

        Self {
            shutdown,
            receiver,
            last_stats: None,
            last_check: None,
            interface,
        }
    }

    async fn handle_message(&mut self, msg: NetworkMonitorMessage) {
        match msg {
            NetworkMonitorMessage::GetMetrics { sender } => {
                let metrics = self.calculate_metrics().await;
                _ = sender.send(metrics);
            }
        }
    }

    async fn calculate_metrics(&mut self) -> NetworkMetrics {
        let current_stats = match read_network_stats().await {
            Ok(stats) => match stats.get(&self.interface).cloned() {
                Some(s) => s,
                None => {
                    error!("Interface {} not found", self.interface);
                    return NetworkMetrics::default();
                }
            },
            Err(e) => {
                error!("Failed to read network stats: {}", e);
                return NetworkMetrics::default();
            }
        };

        let now = tokio::time::Instant::now();

        if let (Some(last_stats), Some(last_check)) = (&self.last_stats, self.last_check) {
            let interval_seconds = now.duration_since(last_check).as_secs();

            if interval_seconds > 0 {
                let rx_bytes_delta = current_stats.rx_bytes.saturating_sub(last_stats.rx_bytes);
                let tx_bytes_delta = current_stats.tx_bytes.saturating_sub(last_stats.tx_bytes);

                self.last_stats = Some(current_stats);
                self.last_check = Some(now);

                return NetworkMetrics {
                    rx_bytes_delta,
                    tx_bytes_delta,
                    interval_seconds,
                };
            }
        }

        self.last_stats = Some(current_stats);
        self.last_check = Some(now);
        NetworkMetrics::default()
    }

    async fn run(&mut self) {
        loop {
            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    self.handle_message(msg).await;
                }
                _ = self.shutdown.token.cancelled() => {
                    break;
                }
            }
        }

        info!("NetworkMonitor task shut down");
    }
}

#[derive(Clone)]
pub struct NetworkMonitorHandle {
    sender: mpsc::Sender<NetworkMonitorMessage>,
}

impl NetworkMonitorHandle {
    pub fn new(shutdown: ShutdownSignals) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        tokio::spawn(async move {
            let mut actor = NetworkMonitor::new(shutdown, receiver).await;
            actor.run().await;
        });

        Self { sender }
    }

    pub async fn get_metrics(&self) -> NetworkMetrics {
        let (sender, receiver) = oneshot::channel();
        let msg = NetworkMonitorMessage::GetMetrics { sender };
        _ = self.sender.send(msg).await;
        receiver.await.unwrap_or_default()
    }
}
