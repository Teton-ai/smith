//! Reads the device's network and system settings and reports them to the api
//! (`POST /smith/settings`). Phase 1 only reads: nothing here writes to the
//! system.

mod actor;
pub mod read;

use crate::magic::MagicHandle;
use crate::session::SessionHandle;
use crate::shutdown::ShutdownSignals;
use actor::{Actor, ActorMessage};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error};

#[derive(Clone)]
pub struct SettingsHandle {
    sender: mpsc::Sender<ActorMessage>,
}

impl SettingsHandle {
    pub fn new(shutdown: ShutdownSignals, magic: MagicHandle, session: SessionHandle) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let mut actor = Actor::new(shutdown, receiver, magic, session);
        tokio::spawn(async move { actor.run().await });

        Self { sender }
    }

    /// Re-read now instead of waiting for the next tick; reports only if
    /// something changed. A full queue means a check is already pending.
    pub fn check_now(&self) {
        if let Err(err) = self.sender.try_send(ActorMessage::CheckNow) {
            debug!("Settings check already queued: {err}");
        }
    }

    /// Read and send a full report now. True once the api acknowledged it.
    pub async fn report_now(&self) -> bool {
        let (reply, receiver) = oneshot::channel();
        if let Err(err) = self.sender.send(ActorMessage::ReportNow { reply }).await {
            error!("Failed to queue settings report: {err}");
            return false;
        }
        receiver.await.unwrap_or(false)
    }
}
