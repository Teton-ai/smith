use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::info;

pub struct LogSession {
    pub device_serial: String,
    pub service_name: String,
    pub dashboard_tx: mpsc::Sender<String>,
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

    pub async fn create_session(
        &self,
        session_id: String,
        device_serial: String,
        service_name: String,
        dashboard_tx: mpsc::Sender<String>,
    ) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(
            session_id.clone(),
            LogSession {
                device_serial,
                service_name,
                dashboard_tx,
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
        sessions.get(session_id).map(|s| s.dashboard_tx.clone())
    }

    pub async fn validate_device_for_session(
        &self,
        session_id: &str,
        device_serial: &str,
    ) -> bool {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .map(|s| s.device_serial == device_serial)
            .unwrap_or(false)
    }
}
