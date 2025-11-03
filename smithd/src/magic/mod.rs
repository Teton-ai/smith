pub mod structure;

use crate::shutdown::ShutdownSignals;
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

struct Magic {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<MagicMessage>,
    configuration: Option<structure::MagicFile>,
    path: Option<PathBuf>,
}

enum MagicMessage {
    Load {
        signal: oneshot::Sender<()>,
        path: Option<String>,
    },
    GetTunnelDetails {
        sender: oneshot::Sender<structure::ConfigTunnel>,
    },
    GetPackages {
        sender: oneshot::Sender<Vec<structure::ConfigPackage>>,
    },
    SetPackages {
        packages: Vec<structure::ConfigPackage>,
    },
    GetServer {
        sender: oneshot::Sender<String>,
    },
    GetReleaseId {
        rpc: oneshot::Sender<Option<i32>>,
    },
    SetReleaseId {
        release_id: Option<i32>,
    },
    GetTargetReleaseId {
        rpc: oneshot::Sender<Option<i32>>,
    },
    SetTargetReleaseId {
        target_release_id: Option<i32>,
    },
    GetToken {
        rpc: oneshot::Sender<Option<String>>,
    },
    SetToken {
        token: Option<String>,
    },
}

impl Magic {
    fn new(shutdown: ShutdownSignals, receiver: mpsc::Receiver<MagicMessage>) -> Self {
        Self {
            shutdown,
            receiver,
            configuration: None,
            path: None,
        }
    }
    async fn handle_message(&mut self, msg: MagicMessage) {
        match msg {
            MagicMessage::Load { path, signal } => {
                match structure::MagicFile::load(path) {
                    Ok((conf, path)) => {
                        self.configuration = Some(conf);
                        self.path = path;
                    }
                    Err(err) => {
                        error!("Failed to load Magic from file: {:?}", err);
                    }
                }
                signal.send(()).unwrap();
            }
            MagicMessage::GetTunnelDetails { sender } => {
                debug!("Getting Magic Tunnel Details");
                if let Some(conf) = &self.configuration {
                    _ = sender.send(conf.get_tunnel_details());
                } else {
                    _ = sender.send(structure::ConfigTunnel::default());
                }
            }
            MagicMessage::GetPackages { sender } => {
                debug!("Getting Magic Packages");
                if let Some(conf) = &self.configuration {
                    _ = sender.send(conf.get_packages());
                } else {
                    _ = sender.send(vec![]);
                }
            }
            MagicMessage::GetServer { sender } => {
                debug!("Getting Magic Server");
                if let Some(conf) = &self.configuration {
                    _ = sender.send(conf.get_server());
                } else {
                    warn!("No server configured, using default");
                    _ = sender.send("https://api.smith.teton.ai/smith".to_string());
                }
            }
            MagicMessage::GetReleaseId { rpc } => {
                debug!("Getting Magic Release Id");
                if let Some(conf) = &self.configuration {
                    _ = rpc.send(conf.get_release_id());
                } else {
                    warn!("No release id configured, using default");
                    _ = rpc.send(None);
                }
            }
            MagicMessage::SetReleaseId { release_id } => {
                if let Some(conf) = &mut self.configuration {
                    let current_release_id = conf.meta.release_id;
                    if current_release_id == release_id {
                        return;
                    }
                    debug!("Setting Magic Release Id");
                    conf.set_release_id(release_id);
                    match &self.path {
                        Some(path) => {
                            _ = conf.write_to_file(path.to_str().unwrap()).await;
                        }
                        None => {
                            warn!("No path to write to");
                        }
                    }
                }
            }
            MagicMessage::GetTargetReleaseId { rpc } => {
                debug!("Getting Magic Target Release Id");
                if let Some(conf) = &self.configuration {
                    _ = rpc.send(conf.get_target_release_id());
                } else {
                    warn!("No target release id configured, using default");
                    _ = rpc.send(None);
                }
            }
            MagicMessage::SetTargetReleaseId { target_release_id } => {
                if let Some(conf) = &mut self.configuration {
                    let current_target_release_id = conf.meta.target_release_id;
                    if current_target_release_id == target_release_id {
                        return;
                    }
                    debug!("Setting Magic Target Release Id");
                    conf.set_target_release_id(target_release_id);
                    match &self.path {
                        Some(path) => {
                            _ = conf.write_to_file(path.to_str().unwrap()).await;
                        }
                        None => {
                            warn!("No path to write to");
                        }
                    }
                }
            }
            MagicMessage::SetPackages { packages } => {
                debug!("Setting Magic Packages");
                if let Some(conf) = &mut self.configuration {
                    conf.set_packages(packages);
                    match &self.path {
                        Some(path) => {
                            _ = conf.write_to_file(path.to_str().unwrap()).await;
                        }
                        None => {
                            warn!("No path to write to");
                        }
                    }
                }
            }
            MagicMessage::GetToken { rpc } => {
                debug!("Getting Magic Token From Magic File");
                if let Some(conf) = &self.configuration {
                    _ = rpc.send(conf.get_token());
                } else {
                    warn!("No token configured, using default");
                    _ = rpc.send(None);
                }
            }
            MagicMessage::SetToken { token } => {
                debug!("Setting Magic Token");
                if let Some(conf) = &mut self.configuration {
                    conf.set_token(token);
                    match &self.path {
                        Some(path) => {
                            _ = conf.write_to_file(path.to_str().unwrap()).await;
                        }
                        None => {
                            warn!("No path to write to");
                        }
                    }
                }
            }
        }
    }

    async fn run(&mut self) {
        loop {
            tokio::select! {
                Some(msg) = self.receiver.recv() => {
                    self.handle_message(msg).await;
                }
                _ = self.shutdown.token.cancelled() => {
                    break;
                }
            }
        }

        info!("Magic task shut down");
    }
}

#[derive(Clone)]
pub struct MagicHandle {
    sender: mpsc::Sender<MagicMessage>,
}

impl MagicHandle {
    pub fn new(shutdown: ShutdownSignals) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let mut actor = Magic::new(shutdown, receiver);
        tokio::spawn(async move { actor.run().await });

        Self { sender }
    }

    pub async fn load(&self, path: Option<String>) {
        let (signal, done) = oneshot::channel();
        let message = MagicMessage::Load { path, signal };
        _ = self.sender.send(message).await;
        done.await.unwrap();
    }

    pub async fn get_release_id(&self) -> Option<i32> {
        let (rpc, fut) = oneshot::channel();
        let msg = MagicMessage::GetReleaseId { rpc };
        _ = self.sender.send(msg).await;
        fut.await.unwrap()
    }

    pub async fn set_release_id(&self, release_id: Option<i32>) {
        let msg = MagicMessage::SetReleaseId { release_id };
        _ = self.sender.send(msg).await;
    }

    pub async fn get_target_release_id(&self) -> Option<i32> {
        let (rpc, fut) = oneshot::channel();
        let msg = MagicMessage::GetTargetReleaseId { rpc };
        _ = self.sender.send(msg).await;
        fut.await.unwrap()
    }

    pub async fn set_target_release_id(&self, target_release_id: Option<i32>) {
        let msg = MagicMessage::SetTargetReleaseId { target_release_id };
        _ = self.sender.send(msg).await;
    }

    pub async fn get_tunnel_details(&self) -> structure::ConfigTunnel {
        let (sender, receiver) = oneshot::channel();
        let msg = MagicMessage::GetTunnelDetails { sender };
        _ = self.sender.send(msg).await;
        receiver.await.unwrap()
    }

    pub async fn get_packages(&self) -> Vec<structure::ConfigPackage> {
        let (sender, receiver) = oneshot::channel();
        let msg = MagicMessage::GetPackages { sender };
        _ = self.sender.send(msg).await;
        receiver.await.unwrap()
    }

    pub async fn set_packages(&self, packages: Vec<structure::ConfigPackage>) {
        let msg = MagicMessage::SetPackages { packages };
        _ = self.sender.send(msg).await;
    }

    pub async fn get_server(&self) -> String {
        let (sender, receiver) = oneshot::channel();
        let msg = MagicMessage::GetServer { sender };
        _ = self.sender.send(msg).await;
        receiver.await.unwrap()
    }

    pub async fn get_token(&self) -> Option<String> {
        let (rpc, receiver) = oneshot::channel();
        let msg = MagicMessage::GetToken { rpc };
        _ = self.sender.send(msg).await;
        receiver.await.unwrap()
    }

    pub async fn set_token(&self, token: &str) {
        let msg = MagicMessage::SetToken {
            token: Some(token.to_owned()),
        };
        _ = self.sender.send(msg).await;
    }

    pub async fn delete_token(&self) {
        let msg = MagicMessage::SetToken { token: None };
        _ = self.sender.send(msg).await;
    }

    pub async fn wait_while_not_registered(&self) {
        loop {
            if self.get_token().await.is_some() {
                break;
            }
            warn!("No token found, waiting...");
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
}
