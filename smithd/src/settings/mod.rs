//! Reads the device's network and system settings and reports them to the api
//! as a `ReportSettings` response on the next home poll. Read only: nothing
//! here writes to the system.

mod actor;
pub(crate) mod driver;
mod nm;
mod ntp;
mod read;

use crate::commander::CommanderHandle;
use crate::shutdown::ShutdownSignals;
use crate::utils::schema::SettingsReport;
use actor::{Actor, ActorMessage};
use read::SettingsReader;
use tokio::sync::mpsc;
use tokio::time::Duration;
use tracing::{debug, warn};

/// Helper processes are capped on their own, D-Bus calls are not; a hung
/// service must not stall the reporter or the commander. Under the 60 s check
/// interval.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

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
/// hold since the actor queues its reports on the commander. `None` on timeout.
pub async fn read_now() -> Option<SettingsReport> {
    let mut reader = SettingsReader::default();
    match tokio::time::timeout(READ_TIMEOUT, reader.read(actor::check_interval_s())).await {
        Ok(reading) => Some(reading.report),
        Err(_) => {
            warn!("Settings read timed out after {READ_TIMEOUT:?}");
            None
        }
    }
}
