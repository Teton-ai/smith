use super::READ_TIMEOUT;
use super::read::SettingsReader;
use crate::commander::CommanderHandle;
use crate::shutdown::ShutdownSignals;
use crate::utils::schema::{SafeCommandResponse, SafeCommandRx, SettingsReport};
use tokio::sync::mpsc;
use tokio::time::{self, Duration, Instant};
use tracing::{info, warn};

/// Synthetic command id for reports smithd sends without being asked.
const CMD_ID_REPORT_SETTINGS: i32 = -7;

/// Fixed for now; clamped to the bounds below once it becomes configurable.
const CHECK_INTERVAL_S: u64 = 60;
const MIN_CHECK_INTERVAL_S: u64 = 10;
const MAX_CHECK_INTERVAL_S: u64 = 60 * 60;
/// Reports are not acknowledged, so this bounds how long a lost one leaves the
/// api stale.
const FULL_REPORT_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

fn check_interval() -> Duration {
    Duration::from_secs(CHECK_INTERVAL_S.clamp(MIN_CHECK_INTERVAL_S, MAX_CHECK_INTERVAL_S))
}

pub fn check_interval_s() -> i32 {
    i32::try_from(check_interval().as_secs()).unwrap_or(i32::MAX)
}

/// The part of a report that counts as a change. The NTP server in use moves on
/// its own (chrony hops between pool servers), so it is sent with every report
/// but never triggers one. The sync flag does count: the first report after a
/// boot is usually taken before the first sync and would otherwise stay
/// unsynchronized until the daily report.
fn change_key(report: &SettingsReport) -> SettingsReport {
    SettingsReport {
        ntp_server_in_use: None,
        ..report.clone()
    }
}

pub enum ActorMessage {
    CheckNow,
}

fn should_send(
    current: &SettingsReport,
    last_sent: Option<&SettingsReport>,
    since_last_sent: Duration,
) -> bool {
    last_sent.map(change_key) != Some(change_key(current)) || since_last_sent >= FULL_REPORT_EVERY
}

/// A driver reload makes the vendor interface vanish for a few seconds; skip
/// the first check where it is gone so a reload doesn't report a burst of read
/// errors. If it is still gone on the next check, that one is reported.
fn hold_for_reload(previous_vendor_present: Option<bool>, vendor_present: bool) -> bool {
    previous_vendor_present == Some(true) && !vendor_present
}

pub struct Actor {
    shutdown: ShutdownSignals,
    receiver: mpsc::Receiver<ActorMessage>,
    commander: CommanderHandle,
    reader: SettingsReader,
    last_sent: Option<SettingsReport>,
    last_sent_at: Instant,
    previous_vendor_present: Option<bool>,
}

impl Actor {
    pub fn new(
        shutdown: ShutdownSignals,
        receiver: mpsc::Receiver<ActorMessage>,
        commander: CommanderHandle,
    ) -> Self {
        Self {
            shutdown,
            receiver,
            commander,
            reader: SettingsReader::default(),
            last_sent: None,
            last_sent_at: Instant::now(),
            previous_vendor_present: None,
        }
    }

    pub async fn run(&mut self) {
        info!("Settings reporter starting");

        let mut interval = time::interval(check_interval());
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = self.shutdown.token.cancelled() => break,
                _ = interval.tick() => self.check().await,
                Some(ActorMessage::CheckNow) = self.receiver.recv() => self.check().await,
            }
        }

        info!("Settings reporter shut down");
    }

    async fn check(&mut self) {
        let reading = match time::timeout(READ_TIMEOUT, self.reader.read(check_interval_s())).await
        {
            Ok(reading) => reading,
            Err(_) => {
                warn!("Settings read timed out after {READ_TIMEOUT:?}; skipping this check");
                return;
            }
        };
        let hold = hold_for_reload(self.previous_vendor_present, reading.vendor_present);
        self.previous_vendor_present = Some(reading.vendor_present);
        if hold {
            info!("Wi-Fi driver interface missing; waiting one check before reporting");
            return;
        }

        if !should_send(
            &reading.report,
            self.last_sent.as_ref(),
            self.last_sent_at.elapsed(),
        ) {
            return;
        }

        self.commander
            .insert_result(vec![SafeCommandResponse {
                id: CMD_ID_REPORT_SETTINGS,
                command: SafeCommandRx::ReportSettings {
                    report: reading.report.clone(),
                },
                status: 0,
            }])
            .await;
        info!("Settings report queued for the next poll");
        self.last_sent = Some(reading.report);
        self.last_sent_at = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sends_on_change_or_daily_but_not_on_ntp_hops() {
        let a = SettingsReport {
            hostname: Some("a".to_owned()),
            ntp_server_in_use: Some("185.125.190.122".to_owned()),
            ntp_synchronized: Some(false),
            ..Default::default()
        };
        let synced = SettingsReport {
            ntp_synchronized: Some(true),
            ..a.clone()
        };
        let renamed = SettingsReport {
            hostname: Some("b".to_owned()),
            ..a.clone()
        };
        let hopped = SettingsReport {
            ntp_server_in_use: Some("185.125.190.123".to_owned()),
            ..a.clone()
        };
        let hour = Duration::from_secs(3600);

        assert!(!should_send(&a, Some(&a), hour), "unchanged");
        assert!(!should_send(&hopped, Some(&a), hour), "NTP hop");
        assert!(should_send(&renamed, Some(&a), hour), "changed");
        assert!(should_send(&synced, Some(&a), hour), "sync flip");
        assert!(should_send(&a, None, hour), "never sent");
        assert!(should_send(&a, Some(&a), FULL_REPORT_EVERY), "daily");
    }

    #[test]
    fn reload_hold_skips_only_the_first_check_without_the_interface() {
        assert!(hold_for_reload(Some(true), false), "just vanished");
        assert!(!hold_for_reload(Some(false), false), "still gone");
        assert!(!hold_for_reload(None, false), "never had it");
    }
}
