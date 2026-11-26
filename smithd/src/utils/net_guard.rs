//! Shared "network busy" flag between the writers that change the device's
//! networks (the commander's network commands, the settings actor's driver
//! reloads and Wi-Fi re-activations) and link health's recoveries, so neither
//! starts while the other runs. NM keeps moving for a while after nmcli
//! returns, so each side also waits a settling period after the other's
//! release.
//!
//! The NM watcher's read-only `nmcli` calls do not take it.

use std::sync::{Arc, Mutex, PoisonError};
use tokio::time::{Duration, Instant};

const RECOVERY_POLL: Duration = Duration::from_secs(1);

#[derive(Debug, Default)]
struct Side {
    held: u32,
    last_release: Option<Instant>,
}

impl Side {
    fn quiet(&self, settle: Duration) -> bool {
        self.held == 0
            && self
                .last_release
                .is_none_or(|released| released.elapsed() >= settle)
    }
}

#[derive(Debug, Default)]
struct State {
    change: Side,
    recovery: Side,
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Change,
    Recovery,
}

impl State {
    fn side(&mut self, kind: Kind) -> &mut Side {
        match kind {
            Kind::Change => &mut self.change,
            Kind::Recovery => &mut self.recovery,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct NetGuard {
    state: Arc<Mutex<State>>,
}

/// Marks the network busy until dropped.
#[must_use = "the network is only marked busy while the hold is alive"]
pub struct NetGuardHold {
    state: Arc<Mutex<State>>,
    kind: Kind,
}

impl NetGuard {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic while holding the lock cannot leave `State` inconsistent, so
        // a poisoned lock is still safe to use.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn take(&self, state: &mut State, kind: Kind) -> NetGuardHold {
        state.side(kind).held += 1;
        NetGuardHold {
            state: Arc::clone(&self.state),
            kind,
        }
    }

    /// Holds the guard for a network change, whatever else holds it.
    pub fn hold(&self) -> NetGuardHold {
        let mut state = self.lock();
        self.take(&mut state, Kind::Change)
    }

    /// True when no change holds the guard and the last one was released at
    /// least `settle` ago.
    pub fn change_quiet(&self, settle: Duration) -> bool {
        self.lock().change.quiet(settle)
    }

    /// A hold for a link recovery, or `None` while a change holds the guard or
    /// was released less than `settle` ago.
    pub fn try_recovery(&self, settle: Duration) -> Option<NetGuardHold> {
        let mut state = self.lock();
        state
            .change
            .quiet(settle)
            .then(|| self.take(&mut state, Kind::Recovery))
    }

    /// Holds the guard for a change once no recovery holds it and the last one
    /// was released at least `settle` ago. Checked and taken under one lock,
    /// so a recovery cannot start in between.
    pub async fn hold_after_recovery(&self, settle: Duration) -> NetGuardHold {
        loop {
            {
                let mut state = self.lock();
                if state.recovery.quiet(settle) {
                    return self.take(&mut state, Kind::Change);
                }
            }
            tokio::time::sleep(RECOVERY_POLL).await;
        }
    }
}

impl Drop for NetGuardHold {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let side = state.side(self.kind);
        side.held = side.held.saturating_sub(1);
        side.last_release = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;

    const SETTLE: Duration = Duration::from_secs(60);

    #[tokio::test(start_paused = true)]
    async fn quiet_when_never_held() {
        assert!(NetGuard::new().change_quiet(SETTLE));
    }

    #[tokio::test(start_paused = true)]
    async fn busy_while_held() {
        let guard = NetGuard::new();
        let _hold = guard.hold();
        assert!(!guard.change_quiet(SETTLE));
        tokio::time::advance(SETTLE * 2).await;
        assert!(!guard.change_quiet(SETTLE));
    }

    #[tokio::test(start_paused = true)]
    async fn quiet_only_after_settling() {
        let guard = NetGuard::new();
        drop(guard.hold());
        assert!(!guard.change_quiet(SETTLE));
        tokio::time::advance(SETTLE - Duration::from_secs(1)).await;
        assert!(!guard.change_quiet(SETTLE));
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(guard.change_quiet(SETTLE));
    }

    #[tokio::test(start_paused = true)]
    async fn nested_holds_stay_busy_until_the_last_drops() {
        let guard = NetGuard::new();
        let first = guard.hold();
        let second = guard.clone().hold();
        drop(first);
        tokio::time::advance(SETTLE * 2).await;
        assert!(!guard.change_quiet(SETTLE));
        drop(second);
        tokio::time::advance(SETTLE).await;
        assert!(guard.change_quiet(SETTLE));
    }

    #[tokio::test(start_paused = true)]
    async fn no_recovery_starts_while_a_change_holds_the_guard() {
        let guard = NetGuard::new();
        let change = guard.hold();
        assert!(guard.try_recovery(SETTLE).is_none());
        drop(change);
        tokio::time::advance(SETTLE).await;
        assert!(guard.try_recovery(SETTLE).is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn a_change_waits_for_a_recovery_and_its_settling() {
        let guard = NetGuard::new();
        let recovery = guard.try_recovery(SETTLE);
        let waiting = tokio::spawn({
            let guard = guard.clone();
            async move { guard.hold_after_recovery(SETTLE).await }
        });
        tokio::time::advance(SETTLE * 2).await;
        assert!(!waiting.is_finished(), "recovery still running");

        drop(recovery);
        let started = Instant::now();
        let _hold = waiting.await.unwrap();
        assert!(started.elapsed() >= SETTLE);
        assert!(!guard.change_quiet(SETTLE), "the change holds the guard");
    }

    #[tokio::test(start_paused = true)]
    async fn a_change_does_not_wait_on_another_change() {
        let guard = NetGuard::new();
        let _commander = guard.hold();
        assert!(guard.hold_after_recovery(SETTLE).now_or_never().is_some());
    }
}
