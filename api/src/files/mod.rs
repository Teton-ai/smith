pub mod route;
pub mod session;

use crate::relay;
use crate::storage::Storage;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::{MissedTickBehavior, sleep};
use tracing::{error, info};

/// Interval between cleanup passes. S3 lifecycle rules are day-granularity, so
/// removing staged device files within the hour has to happen here.
const SWEEP_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// Delete staged download objects past their TTL, close abandoned sessions of
/// every kind and drop their relay backlog.
///
/// Safe to run on every replica: `claim_expired_objects` claims the rows it
/// returns in one statement, so exactly one replica gets each object key, and
/// the session sweep is idempotent.
pub fn spawn_sweeper(pool: PgPool, bucket: &'static str) {
    tokio::spawn(async move {
        // Every replica boots at once during a rolling deploy, and `interval`
        // fires its first tick immediately, so stagger the start rather than
        // have the whole fleet sweep in lockstep.
        sleep(SWEEP_INTERVAL / 2).await;

        let mut ticker = tokio::time::interval(SWEEP_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;

            match session::claim_expired_objects(&pool).await {
                Ok(claimed) => {
                    for (upload_token, key) in claimed {
                        // The row stays claimed if this fails, so a later pass
                        // retries it instead of orphaning the object.
                        if let Err(e) = Storage::delete_from_s3(bucket, &key).await {
                            error!("Failed to delete staged file {key}: {e}");
                            continue;
                        }
                        if let Err(e) = session::finish_sweep(&pool, &upload_token).await {
                            error!("Failed to drop swept download row for {key}: {e}");
                        }
                    }
                }
                Err(e) => error!("Failed to claim expired staged files: {e}"),
            }

            match relay::sweep_stale_sessions(&pool).await {
                Ok(dropped) if dropped > 0 => {
                    info!("Dropped {dropped} orphaned session messages")
                }
                Ok(_) => {}
                Err(e) => error!("Failed to sweep stale sessions: {e}"),
            }
        }
    });
}
