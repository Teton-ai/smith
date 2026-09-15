use crate::magic::MagicHandle;
use crate::shutdown::ShutdownSignals;
use crate::utils::files::{add_key, ensure_ssh_dir, remove_key};
use bore_cli::client::Client;
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{self, Duration, Instant};
use tracing::{error, info};
use uuid::Uuid;

pub struct RemoteLogin {
    pub user: String,
    pub pub_key: String,
}

struct ForwardConnection {
    created_at: time::Instant,
    tag: String,
    remote_login: Option<RemoteLogin>,
    remote: u16,
    task: tokio::task::JoinHandle<()>,
}

impl ForwardConnection {
    async fn remove(&self) {
        self.task.abort();
        if let Some(remote_login) = &self.remote_login {
            info!("Removing remote login info");
            let res = remove_key(&remote_login.user, &self.tag).await;
            if let Err(err) = res {
                error!("Failed to remove key: {err}");
            }
        }
    }
}

pub enum ActorMessage {
    ForwardPort {
        local: u16,
        remote_login: Option<RemoteLogin>,
        remote: oneshot::Sender<u16>,
    },
    ClosePort {
        local: u16,
    },
}

// Tunnel actor
pub struct Actor {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<ActorMessage>,
    magic: MagicHandle,
    ports: HashMap<u16, ForwardConnection>,
}

impl Actor {
    pub fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<ActorMessage>,
        magic: MagicHandle,
    ) -> Self {
        Self {
            shutdown,
            receiver,
            magic,
            ports: HashMap::new(),
        }
    }

    async fn handle_message(&mut self, msg: ActorMessage, server: &str, secret: &str) {
        match msg {
            ActorMessage::ForwardPort {
                local,
                remote_login,
                remote,
            } => {
                self.timeout_old_tunnels().await;
                let created_at = Instant::now();

                // check if there is already a ForwardConnection for this port
                if self.ports.contains_key(&local) {
                    error!("Port {} is already forwarded", local);
                    let remote_port = self.ports.get(&local).unwrap().remote;
                    remote.send(remote_port).unwrap();
                    return;
                }

                let tag = format!("{}-smith", Uuid::new_v4());
                if let Some(ref remote_login) = remote_login {
                    info!("Received remote_login info for user: {}", remote_login.user);
                    let res = ensure_ssh_dir(&remote_login.user).await;
                    if let Err(err) = res {
                        error!(
                            "Failed to ensure ssh dir for user '{}': {}",
                            remote_login.user, err
                        );
                        _ = remote.send(0);
                        return;
                    }
                    info!("SSH directory ensured for user: {}", remote_login.user);
                }

                let server = server.to_owned();
                let secret = secret.to_owned();
                let (tx, rx) = oneshot::channel();
                let handle = tokio::spawn(async move {
                    let secret_opt = if secret.is_empty() {
                        None
                    } else {
                        Some(secret.as_str())
                    };
                    let mut client = Client::new("localhost", local, &server, 0, secret_opt).await;
                    for attempt in 1..3 {
                        if client.is_ok() {
                            break;
                        }
                        tracing::warn!(attempt, "Retrying tunnel connection");
                        time::sleep(Duration::from_secs(1)).await;
                        client = Client::new("localhost", local, &server, 0, secret_opt).await;
                    }

                    match client {
                        Ok(client) => {
                            info!("Forwarding port {} to {}", local, client.remote_port());
                            _ = tx.send(client.remote_port());
                            // this will block until the connection is closed
                            _ = client.listen().await;
                        }
                        Err(e) => {
                            error!("Failed to forward port {}: {}", local, e);
                            _ = tx.send(0);
                        }
                    }
                });

                let port = rx.await.unwrap_or_default();

                if port == 0 {
                    _ = remote.send(port);
                    return;
                }

                if let Some(ref remote_login) = remote_login {
                    let res = add_key(&remote_login.user, &remote_login.pub_key, tag.clone()).await;
                    if let Err(err) = res {
                        error!(
                            "Failed to add key for user '{}': {}",
                            remote_login.user, err
                        );
                        handle.abort();
                        _ = remote.send(0);
                        return;
                    }
                    info!("SSH key added successfully for user: {}", remote_login.user);
                }

                _ = remote.send(port);

                self.ports.insert(
                    local,
                    ForwardConnection {
                        remote: port,
                        remote_login,
                        task: handle,
                        created_at,
                        tag,
                    },
                );
            }
            ActorMessage::ClosePort { local } => {
                if let Some(conn) = self.ports.remove(&local) {
                    conn.remove().await;
                }
            }
        }
    }

    async fn timeout_old_tunnels(&mut self) {
        let now = time::Instant::now();
        let mut to_remove = Vec::new();
        let timeout_duration = Duration::from_secs(60 * 30);

        for (port, conn) in &self.ports {
            if conn.task.is_finished() || now.duration_since(conn.created_at) > timeout_duration {
                to_remove.push(*port);
            }
        }

        for port in to_remove {
            info!("Closing inactive tunnel on port {}", port);
            if let Some(conn) = self.ports.remove(&port) {
                conn.remove().await;
            }
        }
    }

    pub async fn run(&mut self) {
        info!("Tunnel task is runnning");

        let details = self.magic.get_tunnel_details().await;

        // check tunnels still open every 10 minutes
        let mut timeout_tunnels = time::interval(Duration::from_secs(60 * 10));
        timeout_tunnels.tick().await;

        loop {
            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    self.handle_message(msg, &details.server, &details.secret).await;
                }
                _ = timeout_tunnels.tick() => {
                    self.timeout_old_tunnels().await;
                }
                _ = self.shutdown.token.cancelled() => {
                    break;
                }
            }
        }

        info!("Tunnel task shutting down");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shutdown::ShutdownHandler;

    #[tokio::test]
    async fn closed_tunnel_is_removed_before_reuse() -> anyhow::Result<()> {
        let shutdown = ShutdownHandler::new();
        let magic = MagicHandle::new(shutdown.signals());
        let (_sender, receiver) = mpsc::channel(1);
        let mut actor = Actor::new(shutdown.signals(), receiver, magic);
        let task = tokio::spawn(async {});
        while !task.is_finished() {
            tokio::task::yield_now().await;
        }
        actor.ports.insert(
            22,
            ForwardConnection {
                created_at: Instant::now(),
                tag: "test".into(),
                remote_login: None,
                remote: 12345,
                task,
            },
        );
        actor.timeout_old_tunnels().await;
        assert!(actor.ports.is_empty());
        shutdown.signals().token.cancel();
        Ok(())
    }
}
