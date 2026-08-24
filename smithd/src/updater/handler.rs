use super::actor::Actor;
use super::actor::ActorMessage;
use crate::downloader::DownloaderHandle;
use crate::magic::MagicHandle;
use crate::session::SessionHandle;
use crate::shutdown::ShutdownSignals;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, timeout};
use tracing::warn;

#[derive(Clone)]
pub struct Handler {
    sender: mpsc::Sender<ActorMessage>,
}

impl Handler {
    pub fn new(
        shutdown: ShutdownSignals,
        magic: MagicHandle,
        downloader: DownloaderHandle,
        session: SessionHandle,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let mut actor = Actor::new(shutdown, receiver, magic, downloader, session);
        tokio::spawn(async move { actor.run().await });

        Self { sender }
    }

    pub async fn apply_release(&self) -> bool {
        if let Err(err) = self.sender.send(ActorMessage::Apply).await {
            warn!("Unable to schedule release apply: {}", err);
            return false;
        }
        true
    }

    pub async fn prepare_release(&self) -> bool {
        if let Err(err) = self.sender.send(ActorMessage::Prepare).await {
            warn!("Unable to schedule release preparation: {}", err);
            return false;
        }
        true
    }

    pub async fn install_prepared_release(&self) -> bool {
        if let Err(err) = self.sender.send(ActorMessage::InstallPrepared).await {
            warn!("Unable to schedule prepared release installation: {}", err);
            return false;
        }
        true
    }

    pub async fn status(&self) -> String {
        let (rpc, receiver) = oneshot::channel();

        // Send status request
        if self
            .sender
            .send(ActorMessage::StatusReport { rpc })
            .await
            .is_err()
        {
            return "Error: Unable to send status request".to_string();
        }

        // Wait for response with 5 second timeout
        match timeout(Duration::from_secs(5), receiver).await {
            Ok(Ok(status)) => status,
            Ok(Err(_)) => {
                warn!("Status channel closed unexpectedly");
                "Error: Status response channel closed".to_string()
            }
            Err(_) => {
                warn!(
                    "Status request timed out after 5 seconds - system may be busy with update/upgrade"
                );
                "Status unavailable (system busy - update or upgrade in progress)".to_string()
            }
        }
    }
}
