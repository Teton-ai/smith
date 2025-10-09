use super::actor::{Actor, ActorMessage};
use crate::shutdown::ShutdownSignals;
use tokio::sync::{mpsc, oneshot};

#[derive(Clone)]
pub struct Handler {
    sender: mpsc::Sender<ActorMessage>,
}

impl Handler {
    pub fn new(shutdown: ShutdownSignals) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let mut actor = Actor::new(shutdown, receiver);
        tokio::spawn(async move { actor.run().await });

        Self { sender }
    }

    pub async fn start_stream(&self, service: Option<String>) -> Result<u16, String> {
        let (sender, receiver) = oneshot::channel();

        let msg = ActorMessage::StartStream {
            service,
            response: sender,
        };

        self.sender
            .send(msg)
            .await
            .map_err(|_| "Failed to send message to actor".to_string())?;

        receiver
            .await
            .map_err(|_| "Failed to receive response from actor".to_string())?
    }

    pub async fn stop_stream(&self) {
        let msg = ActorMessage::StopStream;
        let _ = self.sender.send(msg).await;
    }
}
