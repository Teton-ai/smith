//! Background garbage collection sweep for `network` rows nothing references
//! anymore.
//!
//! Deliberately not part of the same deploy as the rest of the reference
//! ledger. It only ships once App API has deployed its client and pushed at
//! least one full reconcile, so every pre-existing synced network already has
//! a `network_reference` row before this sweep can ever run against it. No
//! runtime flag gates this: the sweep's presence in the deployed binary *is*
//! the on/off state.

use crate::network::ledger::{collect_network, lock_network};
use rand::RngExt;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::{MissedTickBehavior, sleep};
use tracing::{error, info};

/// Delay before the first tick, so the sweep doesn't compete with everything
/// else starting up right after boot.
const STARTUP_GRACE: Duration = Duration::from_secs(60);

/// Added on top of `STARTUP_GRACE`, randomized per instance: `smith-api` runs
/// several replicas, and a fixed delay alone still has all of them land on
/// the same candidate set at once. The interval is 2 days by default, so a
/// few extra minutes of spread costs nothing.
const STARTUP_JITTER_MAX: Duration = Duration::from_secs(300);

/// Finds every network with zero `network_reference` rows and zero internal
/// FK references, taking each candidate's per-id advisory lock and running it
/// through `collect_network` - the exact same check `acquire`/`release`/
/// `reconcile` use, so this can never drift into a different opinion of what
/// counts as referenced. Returns the number actually collected.
async fn sweep_once(pool: &PgPool) -> Result<i64, sqlx::Error> {
    // A network's collection state can only change between this SELECT and
    // its own row's `collect_network` call, not because of another candidate
    // in the same batch - each candidate is locked and re-checked
    // independently below, so a stale list is harmless.
    let candidates: Vec<i32> = sqlx::query_scalar!(
        r#"
        SELECT n.id
        FROM network n
        WHERE NOT EXISTS (SELECT 1 FROM network_reference r WHERE r.network_id = n.id)
          AND NOT network_has_internal_reference(n.id)
        "#
    )
    .fetch_all(pool)
    .await?;

    let mut collected = 0;
    for network_id in candidates {
        let mut tx = pool.begin().await?;
        lock_network(&mut tx, network_id).await?;
        if collect_network(&mut tx, network_id).await? {
            collected += 1;
        }
        tx.commit().await?;
    }
    Ok(collected)
}

/// Runs the sweep on its own task, so periodic deletion work never blocks a
/// request handler, on `config.network_gc_interval_seconds`.
pub fn spawn_sweeper(pool: PgPool, interval_seconds: u64) {
    tokio::spawn(async move {
        let jitter = rand::rng().random_range(Duration::ZERO..=STARTUP_JITTER_MAX);
        sleep(STARTUP_GRACE + jitter).await;

        let mut ticker = tokio::time::interval(Duration::from_secs(interval_seconds));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            ticker.tick().await;
            match sweep_once(&pool).await {
                Ok(0) => {}
                Ok(collected) => {
                    info!("Network garbage collection sweep collected {collected} network(s)")
                }
                Err(err) => error!("Network garbage collection sweep failed: {err:?}"),
            }
        }
    });
}
