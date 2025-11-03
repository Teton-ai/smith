use super::actor::Actor;
use super::actor::ActorMessage;
use crate::magic::MagicHandle;
use crate::shutdown::ShutdownSignals;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{timeout, Duration};
use tracing::warn;

#[derive(Clone)]
pub struct Handler {
    sender: mpsc::Sender<ActorMessage>,
}

impl Handler {
    pub fn new(shutdown: ShutdownSignals, magic: MagicHandle) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let mut actor = Actor::new(shutdown, receiver, magic);
        tokio::spawn(async move { actor.run().await });

        Self { sender }
    }

    pub async fn check_for_updates(&self) -> bool {
        // unwrap because if this fails then we are in a bad state
        self.sender.send(ActorMessage::Update).await.unwrap();
        true
    }

    pub async fn upgrade_device(&self) {
        // unwrap because if this fails then we are in a bad state
        self.sender.send(ActorMessage::Upgrade).await.unwrap();
    }

    pub async fn status(&self) -> String {
        let (rpc, receiver) = oneshot::channel();

        // Send status request
        if self.sender
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
                warn!("Status request timed out after 5 seconds - system may be busy with update/upgrade");
                "Status unavailable (system busy - update or upgrade in progress)".to_string()
            }
        }
    }
}
