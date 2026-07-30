use super::actor::{Actor, ActorMessage};
use crate::magic::MagicHandle;
use crate::session::SessionHandle;
use crate::shutdown::ShutdownSignals;
use anyhow::Result;
use tokio::sync::{mpsc, oneshot};
use tracing::error;

#[derive(Clone)]
pub struct FileBrowserHandle {
    sender: mpsc::Sender<ActorMessage>,
}

impl FileBrowserHandle {
    pub fn new(shutdown: ShutdownSignals, magic: MagicHandle, session: SessionHandle) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let mut actor = Actor::new(shutdown, receiver, sender.clone(), magic, session);
        tokio::spawn(async move { actor.run().await });

        Self { sender }
    }

    /// Resolves once the session task is *spawned*, not once browsing finishes.
    /// The commander executes commands sequentially, so waiting for completion
    /// here would stall every other command on the device — including Restart.
    pub async fn open_session(&self, session_id: String) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ActorMessage::OpenSession {
                session_id,
                result: tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("FileBrowser actor is not running"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("FileBrowser actor dropped the request"))?
    }

    pub async fn close_session(&self, session_id: String) {
        self.sender
            .send(ActorMessage::CloseSession { session_id })
            .await
            .inspect_err(|e| error!("Failed to close file session: {e}"))
            .ok();
    }
}
