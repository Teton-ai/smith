mod report;

use crate::magic::MagicHandle;
use crate::police::PoliceHandle;
use crate::shutdown::ShutdownSignals;
use report::InitialCheck;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info};

struct Bouncer {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<BouncerMessage>,
    magic: MagicHandle,
    police: PoliceHandle,
    problems: Option<u32>,
    checks: Option<Vec<report::InitialCheck>>,
}
enum BouncerMessage {
    RunChecks { sender: oneshot::Sender<bool> },
}

impl Bouncer {
    fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<BouncerMessage>,
        magic: MagicHandle,
        police: PoliceHandle,
    ) -> Self {
        Self {
            shutdown,
            receiver,
            magic,
            checks: None,
            police,
            problems: None,
        }
    }

    async fn run_checks(&mut self) -> bool {
        info!("Bouncer Running Checks");
        let mut all_checks_passed = true;
        let mut checks = self
            .magic
            .get_checks()
            .await
            .into_iter()
            .map(InitialCheck::from)
            .collect::<Vec<_>>();

        for check in checks.iter_mut() {
            check.execute().await.unwrap_or_else(|_| {
                all_checks_passed = false;
            });
        }

        self.checks = Some(checks);

        all_checks_passed
    }

    async fn handle_message(&mut self, msg: BouncerMessage) {
        match msg {
            BouncerMessage::RunChecks { sender } => {
                let all_ok = self.run_checks().await;

                _ = sender.send(all_ok);

                if !all_ok && self.problems.is_none() {
                    self.problems = self.police.report_problem_starting().await;
                } else if all_ok && let Some(problems) = self.problems.take() {
                    self.police.report_problem_solved(problems).await;
                }
            }
        }
    }

    async fn run(&mut self) {
        info!("Bouncer runnning");

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

        info!("Bouncer task shut down");
    }
}

#[derive(Clone)]
pub struct BouncerHandle {
    sender: mpsc::Sender<BouncerMessage>,
}

impl BouncerHandle {
    pub fn new(shutdown: ShutdownSignals, magic: MagicHandle, police: PoliceHandle) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let mut actor = Bouncer::new(shutdown, receiver, magic, police);
        tokio::spawn(async move { actor.run().await });

        Self { sender }
    }

    pub async fn ok(&self) {
        loop {
            let (sender, receiver) = oneshot::channel();
            let msg = BouncerMessage::RunChecks { sender };
            _ = self.sender.send(msg).await;
            if receiver.await.unwrap() {
                info!("All checks passed");
                break;
            } else {
                error!("Some checks failed, retrying in 10 seconds");
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        }
    }
}
