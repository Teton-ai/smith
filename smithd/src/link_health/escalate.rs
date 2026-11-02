//! Pure decision logic for escalating Wi-Fi recovery during Police's reboot
//! countdown: no I/O, Police's own `elapsed_seconds` is the clock.

use crate::police::RebootStatus;
use tokio::time::Duration;

pub(super) const REASSOCIATE_AT: Duration = Duration::from_secs(5 * 60);
pub(super) const RELOAD_AT: Duration = Duration::from_secs(15 * 60);

/// Furthest escalation step attempted for the current `schedule_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Step {
    #[default]
    NotStarted,
    Reassociated,
    Reloaded,
}

/// What `on_tick` should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Due {
    Nothing,
    Reassociate,
    Reload,
}

/// Tracks escalation progress for whichever reboot Police currently has
/// pending. Keyed on `schedule_id` so a cancelled-and-rescheduled reboot (or
/// one solved and later re-triggered) starts escalation over.
#[derive(Debug, Default)]
pub(super) struct Escalation {
    schedule_id: Option<u32>,
    step: Step,
}

impl Escalation {
    pub(super) fn due(&mut self, status: &RebootStatus) -> Due {
        let Some(schedule_id) = status.schedule_id else {
            self.schedule_id = None;
            self.step = Step::NotStarted;
            return Due::Nothing;
        };
        if self.schedule_id != Some(schedule_id) {
            self.schedule_id = Some(schedule_id);
            self.step = Step::NotStarted;
        }

        let elapsed = Duration::from_secs(status.elapsed_seconds);
        if self.step == Step::NotStarted && elapsed >= REASSOCIATE_AT {
            return Due::Reassociate;
        }
        if self.step != Step::Reloaded && elapsed >= RELOAD_AT {
            return Due::Reload;
        }
        Due::Nothing
    }

    /// Records a step as attempted, regardless of its outcome: one-shot per
    /// `schedule_id`, same convention as `recover_roam_stall`/`recover_lockout`.
    pub(super) fn mark(&mut self, step: Step) {
        self.step = step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(schedule_id: Option<u32>, elapsed_seconds: u64) -> RebootStatus {
        RebootStatus {
            reboot_pending: schedule_id.is_some(),
            schedule_id,
            seconds_remaining: 0,
            elapsed_seconds,
            delay_seconds: 0,
            held: false,
            hold_seconds_remaining: 0,
        }
    }

    #[test]
    fn nothing_due_before_five_minutes() {
        let mut escalation = Escalation::default();
        assert_eq!(escalation.due(&status(Some(1), 299)), Due::Nothing);
    }

    #[test]
    fn reassociate_due_at_five_minutes_when_no_step_taken_yet() {
        let mut escalation = Escalation::default();
        assert_eq!(escalation.due(&status(Some(1), 300)), Due::Reassociate);
    }

    #[test]
    fn reload_due_at_fifteen_minutes_after_reassociation_was_marked() {
        let mut escalation = Escalation::default();
        escalation.due(&status(Some(1), 300));
        escalation.mark(Step::Reassociated);
        assert_eq!(escalation.due(&status(Some(1), 900)), Due::Reload);
    }

    #[test]
    fn nothing_due_once_both_steps_are_marked() {
        let mut escalation = Escalation::default();
        escalation.due(&status(Some(1), 900));
        escalation.mark(Step::Reloaded);
        assert_eq!(escalation.due(&status(Some(1), 3600)), Due::Nothing);
    }

    #[test]
    fn a_new_schedule_id_resets_the_step() {
        let mut escalation = Escalation::default();
        escalation.due(&status(Some(1), 900));
        escalation.mark(Step::Reloaded);
        assert_eq!(escalation.due(&status(Some(2), 300)), Due::Reassociate);
    }

    #[test]
    fn no_pending_reboot_resets_state() {
        let mut escalation = Escalation::default();
        escalation.due(&status(Some(1), 900));
        escalation.mark(Step::Reloaded);
        assert_eq!(escalation.due(&status(None, 0)), Due::Nothing);
        assert_eq!(escalation.due(&status(Some(1), 300)), Due::Reassociate);
    }
}
