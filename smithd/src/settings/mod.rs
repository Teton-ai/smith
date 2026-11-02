//! Reads the device's network and system settings and reports them to the api
//! as a `ReportSettings` response on the next home poll, and applies the set
//! of the last `ApplySettings` received, keeping it applied on every check.

mod actor;
mod apply;
pub(crate) mod driver;
mod nm;
mod ntp;
mod plan;
mod read;

use crate::commander::CommanderHandle;
use crate::postman::LastPoll;
use crate::shutdown::ShutdownSignals;
use crate::utils::net_guard::NetGuard;
use crate::utils::schema::{SafeCommandResponse, SafeCommandRx, WantedSettings};
use actor::{Actor, ActorMessage};
use apply::DeviceSystem;
use tokio::sync::{mpsc, oneshot};
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

/// The actor's inbox. Created with the handle before the commander, which
/// holds the handle, and spawned once the commander exists, since the actor
/// queues its reports on it.
pub struct SettingsInbox {
    receiver: mpsc::Receiver<ActorMessage>,
}

impl SettingsInbox {
    pub fn spawn(
        self,
        shutdown: ShutdownSignals,
        commander: CommanderHandle,
        net_guard: NetGuard,
        last_poll: LastPoll,
    ) {
        let system = DeviceSystem::new(commander, last_poll);
        tokio::spawn(async move {
            let mut actor = Actor::load(shutdown, self.receiver, system, net_guard).await;
            actor.run().await
        });
    }
}

/// Status -1 with an empty report, which the api does not store.
fn unanswered(id: i32, command: SafeCommandRx) -> SafeCommandResponse {
    SafeCommandResponse {
        id,
        command,
        status: -1,
    }
}

impl SettingsHandle {
    pub fn new() -> (Self, SettingsInbox) {
        let (sender, receiver) = mpsc::channel(8);
        (Self { sender }, SettingsInbox { receiver })
    }

    /// Asks the actor and waits for its answer, behind any check or apply in
    /// progress. `None` when the actor is gone.
    async fn ask(
        &self,
        what: &str,
        message: impl FnOnce(oneshot::Sender<SafeCommandResponse>) -> ActorMessage,
    ) -> Option<SafeCommandResponse> {
        let (reply, answer) = oneshot::channel();
        self.sender
            .send(message(reply))
            .await
            .inspect_err(|err| warn!("Settings actor is gone: {err}"))
            .ok()?;
        answer
            .await
            .inspect_err(|err| warn!("Settings actor dropped {what}: {err}"))
            .ok()
    }

    /// Stores and applies `settings`, answering command `id` once the device
    /// has been read back.
    pub async fn apply(
        &self,
        id: i32,
        version: i32,
        settings: WantedSettings,
    ) -> SafeCommandResponse {
        let answer = self
            .ask("ApplySettings", |reply| ActorMessage::Apply {
                id,
                version,
                settings,
                reply,
            })
            .await;
        answer.unwrap_or_else(|| {
            unanswered(
                id,
                SafeCommandRx::ApplySettingsResult {
                    applied_version: version,
                    reverted: false,
                    report: Box::default(),
                },
            )
        })
    }

    /// Answers command `id` with the report of a check run now.
    pub async fn report(&self, id: i32) -> SafeCommandResponse {
        let answer = self
            .ask("ReportSettings", |reply| ActorMessage::Report { id, reply })
            .await;
        answer.unwrap_or_else(|| {
            unanswered(
                id,
                SafeCommandRx::ReportSettings {
                    report: Box::default(),
                },
            )
        })
    }

    /// Re-read now instead of waiting for the next tick; reports only if
    /// something changed. A full queue means a check is already pending.
    pub fn check_now(&self) {
        if let Err(err) = self.sender.try_send(ActorMessage::CheckNow) {
            debug!("Settings check already queued: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_apply_the_actor_never_answers_fails_under_its_command_id() {
        let (handle, inbox) = SettingsHandle::new();
        drop(inbox);
        let answer = handle.apply(42, 4, WantedSettings::default()).await;
        assert!(
            matches!(
                answer,
                SafeCommandResponse {
                    id: 42,
                    command: SafeCommandRx::ApplySettingsResult {
                        applied_version: 4,
                        reverted: false,
                        ..
                    },
                    status: -1,
                }
            ),
            "{answer:?}"
        );
    }
}
