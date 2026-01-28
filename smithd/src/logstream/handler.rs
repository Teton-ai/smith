use super::actor::{Actor, ActorMessage};
use crate::magic::MagicHandle;
use crate::shutdown::ShutdownSignals;
use anyhow::Result;
use tokio::sync::{mpsc, oneshot};

#[derive(Clone)]
pub struct LogStreamHandle {
    sender: mpsc::Sender<ActorMessage>,
}

impl LogStreamHandle {
    pub fn new(shutdown: ShutdownSignals, magic: MagicHandle) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let mut actor = Actor::new(shutdown, receiver, magic);
        tokio::spawn(async move { actor.run().await });

        Self { sender }
    }

    pub async fn start_stream(
        &self,
        session_id: String,
        service_name: String,
        ws_url: String,
    ) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        let msg = ActorMessage::StartStream {
            session_id,
            service_name,
            ws_url,
            result: tx,
        };
        self.sender
            .send(msg)
            .await
            .map_err(|_| anyhow::anyhow!("Failed to send start stream message"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("Failed to receive result"))?
    }

    pub async fn stop_stream(&self, session_id: String) {
        let msg = ActorMessage::StopStream { session_id };
        let _ = self.sender.send(msg).await;
    }
}
