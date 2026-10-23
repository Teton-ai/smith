//! Shared "network busy" flag. The commander holds it while a command changes
//! the device's networks, and link health stays out of NetworkManager until it
//! has been released for a settling period: NM keeps moving for a while after
//! nmcli returns, and a recovery in that window would fight the command.
//!
//! Only covers the commander's queued network commands. The NM watcher's
//! read-only `nmcli` calls do not take it.

use std::sync::{Arc, Mutex, PoisonError};
use tokio::time::{Duration, Instant};

#[derive(Debug, Default)]
struct State {
    busy: u32,
    last_release: Option<Instant>,
}

#[derive(Clone, Debug, Default)]
pub struct NetGuard {
    state: Arc<Mutex<State>>,
}

/// Marks the network busy until dropped.
#[must_use = "the network is only marked busy while the hold is alive"]
pub struct NetGuardHold {
    state: Arc<Mutex<State>>,
}

impl NetGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn hold(&self) -> NetGuardHold {
        // A panic while holding the lock cannot leave `State` inconsistent, so
        // a poisoned lock is still safe to use.
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.busy += 1;
        NetGuardHold {
            state: Arc::clone(&self.state),
        }
    }

    /// True when nothing holds the guard and the last release is at least
    /// `settle` ago.
    pub fn is_quiet(&self, settle: Duration) -> bool {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.busy == 0
            && state
                .last_release
                .is_none_or(|released| released.elapsed() >= settle)
    }
}

impl Drop for NetGuardHold {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.busy = state.busy.saturating_sub(1);
        state.last_release = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTLE: Duration = Duration::from_secs(60);

    #[tokio::test(start_paused = true)]
    async fn quiet_when_never_held() {
        assert!(NetGuard::new().is_quiet(SETTLE));
    }

    #[tokio::test(start_paused = true)]
    async fn busy_while_held() {
        let guard = NetGuard::new();
        let _hold = guard.hold();
        assert!(!guard.is_quiet(SETTLE));
        tokio::time::advance(SETTLE * 2).await;
        assert!(!guard.is_quiet(SETTLE));
    }

    #[tokio::test(start_paused = true)]
    async fn quiet_only_after_settling() {
        let guard = NetGuard::new();
        drop(guard.hold());
        assert!(!guard.is_quiet(SETTLE));
        tokio::time::advance(SETTLE - Duration::from_secs(1)).await;
        assert!(!guard.is_quiet(SETTLE));
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(guard.is_quiet(SETTLE));
    }

    #[tokio::test(start_paused = true)]
    async fn nested_holds_stay_busy_until_the_last_drops() {
        let guard = NetGuard::new();
        let first = guard.hold();
        let second = guard.clone().hold();
        drop(first);
        tokio::time::advance(SETTLE * 2).await;
        assert!(!guard.is_quiet(SETTLE));
        drop(second);
        tokio::time::advance(SETTLE).await;
        assert!(guard.is_quiet(SETTLE));
    }
}
