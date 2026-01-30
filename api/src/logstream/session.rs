use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, mpsc, watch};
use tracing::info;

pub struct LogSession {
    pub dashboard_tx: mpsc::Sender<String>,
    pub device_connected_tx: watch::Sender<bool>,
    pub device_connected_rx: watch::Receiver<bool>,
}

#[derive(Clone, Default)]
pub struct LogStreamSessions {
    sessions: Arc<RwLock<HashMap<String, LogSession>>>,
}

impl LogStreamSessions {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create_session(&self, session_id: String, dashboard_tx: mpsc::Sender<String>) {
        let (device_connected_tx, device_connected_rx) = watch::channel(false);
        let mut sessions = self.sessions.write().await;
        sessions.insert(
            session_id.clone(),
            LogSession {
                dashboard_tx,
                device_connected_tx,
                device_connected_rx,
            },
        );
        info!("Created log session: {}", session_id);
    }

    pub async fn remove_session(&self, session_id: &str) {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
        info!("Removed log session: {}", session_id);
    }

    pub async fn get_session_tx(&self, session_id: &str) -> Option<mpsc::Sender<String>> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(session_id) {
            let _ = session.device_connected_tx.send(true);
            Some(session.dashboard_tx.clone())
        } else {
            None
        }
    }

    pub async fn wait_for_device(&self, session_id: &str, timeout: Duration) -> bool {
        let rx = {
            let sessions = self.sessions.read().await;
            sessions
                .get(session_id)
                .map(|s| s.device_connected_rx.clone())
        };

        let Some(mut rx) = rx else {
            return false;
        };

        let result = tokio::time::timeout(timeout, async {
            loop {
                if *rx.borrow() {
                    return true;
                }
                if rx.changed().await.is_err() {
                    return false;
                }
            }
        })
        .await;

        result.unwrap_or(false)
    }
}
