use super::read::SettingsReader;
use crate::magic::MagicHandle;
use crate::session::SessionHandle;
use crate::shutdown::ShutdownSignals;
use crate::utils::files::{load_last_settings_report, save_last_settings_report};
use crate::utils::network::NetworkClient;
use crate::utils::schema::SettingsReport;
use anyhow::{Result, bail};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{self, Duration, Instant};
use tracing::{error, info, warn};

/// Phase 2 makes this a setting; keep it within the bounds the api will accept.
const CHECK_INTERVAL_S: u64 = 60;
const FULL_REPORT_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

fn check_interval(secs: u64) -> Duration {
    Duration::from_secs(secs.clamp(10, 3600))
}

pub enum ActorMessage {
    CheckNow,
    ReportNow { reply: oneshot::Sender<bool> },
}

/// Send when something changed since the last acknowledged report, when the
/// daily full report is due, or when asked to.
fn should_send(
    current: &SettingsReport,
    last_acked: Option<&SettingsReport>,
    since_last_ack: Duration,
    forced: bool,
) -> bool {
    forced || last_acked != Some(current) || since_last_ack >= FULL_REPORT_EVERY
}

/// A driver reload makes the vendor interface vanish for a few seconds; skip
/// the first check where it is gone so a reload doesn't report a burst of read
/// errors. If it is still gone on the next check, that one is reported.
fn hold_for_reload(
    previous_vendor_present: Option<bool>,
    vendor_present: bool,
    forced: bool,
) -> bool {
    !forced && previous_vendor_present == Some(true) && !vendor_present
}

pub struct Actor {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<ActorMessage>,
    magic: MagicHandle,
    session: SessionHandle,
    network: NetworkClient,
    reader: SettingsReader,
    last_acked: Option<SettingsReport>,
    last_acked_at: Instant,
    previous_vendor_present: Option<bool>,
}

impl Actor {
    pub fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<ActorMessage>,
        magic: MagicHandle,
        session: SessionHandle,
    ) -> Self {
        Self {
            shutdown,
            receiver,
            magic,
            session,
            network: NetworkClient::new(),
            reader: SettingsReader::default(),
            last_acked: None,
            last_acked_at: Instant::now(),
            previous_vendor_present: None,
        }
    }

    pub async fn run(&mut self) {
        info!("Settings reporter starting");

        self.network.set_hostname(self.magic.get_server().await);
        match load_last_settings_report().await {
            Ok(last) => self.last_acked = last,
            Err(err) => warn!("Ignoring last settings report: {err:#}"),
        }

        let interval_secs = check_interval(CHECK_INTERVAL_S);
        let mut interval = time::interval(interval_secs);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = self.shutdown.token.cancelled() => break,
                _ = interval.tick() => {
                    self.check(false).await;
                }
                Some(msg) = self.receiver.recv() => match msg {
                    ActorMessage::CheckNow => {
                        self.check(false).await;
                    }
                    ActorMessage::ReportNow { reply } => {
                        let acknowledged = self.check(true).await;
                        if reply.send(acknowledged).is_err() {
                            warn!("ReportSettings caller went away before the result");
                        }
                    }
                },
            }
        }

        info!("Settings reporter shut down");
    }

    /// Returns whether the api holds the current values (sent and acknowledged
    /// now, or unchanged since the last acknowledgement).
    async fn check(&mut self, forced: bool) -> bool {
        let interval_s = check_interval(CHECK_INTERVAL_S).as_secs() as i32;
        let reading = self.reader.read(interval_s).await;
        let hold = hold_for_reload(self.previous_vendor_present, reading.vendor_present, forced);
        self.previous_vendor_present = Some(reading.vendor_present);
        if hold {
            info!("Wi-Fi driver interface missing; waiting one check before reporting");
            return false;
        }

        if !should_send(
            &reading.report,
            self.last_acked.as_ref(),
            self.last_acked_at.elapsed(),
            forced,
        ) {
            return true;
        }

        match self.send(&reading.report).await {
            Ok(()) => {
                info!("Settings report acknowledged");
                if let Err(err) = save_last_settings_report(&reading.report).await {
                    error!("Failed to persist the settings report: {err:#}");
                }
                self.last_acked = Some(reading.report);
                self.last_acked_at = Instant::now();
                true
            }
            Err(err) => {
                warn!("Settings report not acknowledged, retrying next check: {err:#}");
                false
            }
        }
    }

    async fn send(&self, report: &SettingsReport) -> Result<()> {
        // Prefer the short-lived JWT; falls back to the opaque token (see
        // SessionHandle::bearer_token).
        let token = match self.session.bearer_token().await {
            Some(token) => token,
            None => match self.magic.get_token().await {
                Some(token) => token,
                None => bail!("device not registered yet"),
            },
        };
        let (status, _) = self
            .network
            .send_compressed_post(&token, "/settings", report)
            .await?;
        if !status.is_success() {
            bail!("api answered {status}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(hostname: &str) -> SettingsReport {
        SettingsReport {
            hostname: Some(hostname.to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn sends_only_on_change_daily_or_when_forced() {
        let a = report("a");
        let hour = Duration::from_secs(3600);

        assert!(!should_send(&a, Some(&a), hour, false), "unchanged");
        assert!(should_send(&report("b"), Some(&a), hour, false), "changed");
        assert!(should_send(&a, None, hour, false), "never acknowledged");
        assert!(should_send(&a, Some(&a), FULL_REPORT_EVERY, false), "daily");
        assert!(should_send(&a, Some(&a), hour, true), "forced");
    }

    #[test]
    fn failed_send_is_retried_because_nothing_was_acknowledged() {
        // A failed send leaves last_acked untouched, so the same report still
        // differs from it on the next tick.
        let previous = report("old");
        let current = report("new");
        assert!(should_send(
            &current,
            Some(&previous),
            Duration::ZERO,
            false
        ));
        assert!(should_send(
            &current,
            Some(&previous),
            Duration::ZERO,
            false
        ));
    }

    #[test]
    fn reload_hold_skips_only_the_first_check_without_the_interface() {
        assert!(hold_for_reload(Some(true), false, false), "just vanished");
        assert!(
            !hold_for_reload(Some(false), false, false),
            "still gone: report"
        );
        assert!(
            !hold_for_reload(None, false, false),
            "first check (e.g. no vendor driver)"
        );
        assert!(!hold_for_reload(Some(true), true, false), "present");
        assert!(!hold_for_reload(Some(true), false, true), "forced");
    }

    #[test]
    fn interval_is_clamped() {
        assert_eq!(check_interval(60), Duration::from_secs(60));
        assert_eq!(check_interval(1), Duration::from_secs(10));
        assert_eq!(check_interval(100_000), Duration::from_secs(3600));
    }
}
