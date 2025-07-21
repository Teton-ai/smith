use super::actor::{Actor, ActorMessage, RemoteLogin};
use crate::magic::MagicHandle;
use crate::shutdown::ShutdownSignals;
use tokio::sync::{mpsc, oneshot};

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

    pub async fn start_tunnel(
        &self,
        port: Option<u16>,
        user: Option<String>,
        pub_key: Option<String>,
    ) -> u16 {
        let local = port.unwrap_or(22);
        let (sender, receiver) = oneshot::channel();

        let remote_login = if let (Some(user), Some(pub_key)) = (user, pub_key) {
            Some(RemoteLogin { user, pub_key })
        } else {
            None
        };

        let msg = ActorMessage::ForwardPort {
            local,
            remote_login,
            remote: sender,
        };
        _ = self.sender.send(msg).await;
        receiver.await.unwrap()
    }

    pub async fn stop_ssh_tunnel(&self) {
        let local = 22;
        let msg = ActorMessage::ClosePort { local };
        _ = self.sender.send(msg).await;
    }
}
