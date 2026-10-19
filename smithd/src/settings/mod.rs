//! Reads the device's network and system settings and reports them to the api
//! as a `ReportSettings` response on the next home poll. Read only: nothing
//! here writes to the system.

mod actor;
mod driver;
mod nm;
mod ntp;
mod read;

use crate::commander::CommanderHandle;
use crate::shutdown::ShutdownSignals;
use crate::utils::schema::SettingsReport;
use actor::{Actor, ActorMessage};
use read::SettingsReader;
use tokio::sync::mpsc;
use tracing::debug;

#[derive(Clone)]
pub struct SettingsHandle {
    sender: mpsc::Sender<ActorMessage>,
}

impl SettingsHandle {
    pub fn new(shutdown: ShutdownSignals, commander: CommanderHandle) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let mut actor = Actor::new(shutdown, receiver, commander);
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
}

/// One full read for the `ReportSettings` command, which carries the report as
/// its response. Independent of the reporter actor, which the commander cannot
/// hold since the actor queues its reports on the commander.
pub async fn read_now() -> SettingsReport {
    SettingsReader::default()
        .read(actor::check_interval_s())
        .await
        .report
}
