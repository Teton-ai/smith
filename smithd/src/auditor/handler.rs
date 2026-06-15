use super::actor::{Actor, ActorMessage};
use crate::commander::CommanderHandle;
use crate::shutdown::ShutdownSignals;
use tokio::sync::mpsc;
use tracing::error;

#[derive(Clone)]
pub struct AuditorHandle {
    sender: mpsc::Sender<ActorMessage>,
}

impl AuditorHandle {
    pub fn new(shutdown: ShutdownSignals, commander: CommanderHandle) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let mut actor = Actor::new(shutdown, receiver, commander);
        tokio::spawn(async move { actor.run().await });

        Self { sender }
    }

    /// Trigger an audit run on demand (daemon start, after SSH hardening). The
    /// result is staged on the commander and reported on the next poll.
    pub async fn run_audit(&self) {
        if let Err(err) = self.sender.send(ActorMessage::RunAudit).await {
            error!("Failed to queue audit run: {err}");
        }
    }
}
