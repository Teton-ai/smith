//! The restart channel. smith-rescue, a process apart from smithd, polls
//! `/smith/restart` with the device's boot id. It takes all Restart commands:
//! each one is bound to the boot of the poll that takes it, and is done when a
//! poll comes from another boot. So a lost answer only delays the reboot, and
//! no Restart reboots a device twice.

use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

/// First key of the two key advisory lock that serializes one device's polls.
/// Shares the key space of `home::COMMAND_RESULT_LOCK` (2).
const RESTART_POLL_LOCK: i32 = 3;

/// Bounds each wait of a poll's transaction, so an overload fails fast.
const POLL_TX_TIMEOUTS: &str =
    "SELECT set_config('lock_timeout', '1s', true), set_config('statement_timeout', '3s', true)";

/// Finishes the Restarts that a reboot ended and hands out the waiting ones.
/// Returns the boot that the device must end, if any.
pub async fn poll(pool: &PgPool, device_id: i32, boot_id: Uuid) -> Result<Option<Uuid>> {
    let mut tx = pool.begin().await?;
    sqlx::query(POLL_TX_TIMEOUTS).execute(&mut *tx).await?;
    sqlx::query!(
        "SELECT pg_advisory_xact_lock($1, $2)",
        RESTART_POLL_LOCK,
        device_id
    )
    .execute(&mut *tx)
    .await?;

    // A poll from another boot proves that the reboot happened.
    sqlx::query!(
        r#"INSERT INTO command_response (device_id, command_id, response, status)
           SELECT q.device_id, q.id,
                  json_build_object('Restart', json_build_object(
                      'message', 'Rebooted: boot ' || q.target_boot || ' ended, now ' || $2::uuid)),
                  0
           FROM command_queue q
           WHERE q.device_id = $1 AND q.target_boot IS NOT NULL AND q.target_boot <> $2
             AND NOT EXISTS (SELECT 1 FROM command_response r WHERE r.command_id = q.id)"#,
        device_id,
        boot_id
    )
    .execute(&mut *tx)
    .await?;

    // All waiting Restarts end the same boot, so two clicks give one reboot.
    sqlx::query!(
        r#"UPDATE command_queue SET fetched = true, fetched_at = now(), target_boot = $2
           WHERE device_id = $1 AND fetched = false AND canceled = false
             AND cmd::jsonb = '"Restart"'::jsonb AND available_at <= now()"#,
        device_id,
        boot_id
    )
    .execute(&mut *tx)
    .await?;

    let open = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM command_queue q
               WHERE q.device_id = $1 AND q.target_boot = $2
                 AND NOT EXISTS (SELECT 1 FROM command_response r WHERE r.command_id = q.id)
           ) AS "open!""#,
        device_id,
        boot_id
    )
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(open.then_some(boot_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn insert_device(pool: &PgPool) -> Result<i32> {
        Ok(
            sqlx::query_scalar("INSERT INTO device (serial_number) VALUES ('d') RETURNING id")
                .fetch_one(pool)
                .await?,
        )
    }

    async fn queue(pool: &PgPool, device_id: i32, cmd: &str) -> Result<i32> {
        Ok(sqlx::query_scalar(
            "WITH bundle AS (INSERT INTO command_bundles DEFAULT VALUES RETURNING uuid)
             INSERT INTO command_queue (device_id, cmd, continue_on_error, bundle)
             SELECT $1, $2::json, false, uuid FROM bundle RETURNING id",
        )
        .bind(device_id)
        .bind(cmd)
        .fetch_one(pool)
        .await?)
    }

    async fn responses(pool: &PgPool, command_id: i32) -> Result<Vec<String>> {
        Ok(
            sqlx::query_scalar("SELECT response::text FROM command_response WHERE command_id = $1")
                .bind(command_id)
                .fetch_all(pool)
                .await?,
        )
    }

    async fn fetched(pool: &PgPool, command_id: i32) -> Result<bool> {
        Ok(
            sqlx::query_scalar("SELECT fetched FROM command_queue WHERE id = $1")
                .bind(command_id)
                .fetch_one(pool)
                .await?,
        )
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn a_restart_ends_the_boot_that_polled_once(pool: PgPool) -> Result<()> {
        let device = insert_device(&pool).await?;
        let (b1, b2) = (Uuid::new_v4(), Uuid::new_v4());
        let restart = queue(&pool, device, r#""Restart""#).await?;

        assert_eq!(poll(&pool, device, b1).await?, Some(b1));
        // The answer was lost: the same boot gets the same Restart again.
        assert_eq!(poll(&pool, device, b1).await?, Some(b1));
        assert!(responses(&pool, restart).await?.is_empty());

        // After the reboot nothing is handed out, and the result is stored once.
        assert_eq!(poll(&pool, device, b2).await?, None);
        assert_eq!(poll(&pool, device, b2).await?, None);
        let stored = responses(&pool, restart).await?;
        assert_eq!(stored.len(), 1);
        assert!(stored[0].contains(&b1.to_string()), "{stored:?}");
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn two_waiting_restarts_give_one_reboot(pool: PgPool) -> Result<()> {
        let device = insert_device(&pool).await?;
        let (b1, b2) = (Uuid::new_v4(), Uuid::new_v4());
        let first = queue(&pool, device, r#""Restart""#).await?;
        let second = queue(&pool, device, r#""Restart""#).await?;

        assert_eq!(poll(&pool, device, b1).await?, Some(b1));
        assert_eq!(poll(&pool, device, b2).await?, None);
        assert_eq!(responses(&pool, first).await?.len(), 1);
        assert_eq!(responses(&pool, second).await?.len(), 1);
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn an_old_restart_is_handed_out_and_other_commands_stay(pool: PgPool) -> Result<()> {
        let device = insert_device(&pool).await?;
        let boot = Uuid::new_v4();
        let restart = queue(&pool, device, r#""Restart""#).await?;
        let ping = queue(&pool, device, r#""Ping""#).await?;
        sqlx::query("UPDATE command_queue SET available_at = now() - interval '7 days'")
            .execute(&pool)
            .await?;

        assert_eq!(poll(&pool, device, boot).await?, Some(boot));
        assert!(fetched(&pool, restart).await?);
        assert!(
            !fetched(&pool, ping).await?,
            "the restart channel took a Ping"
        );
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn a_staggered_restart_waits_for_its_time(pool: PgPool) -> Result<()> {
        let device = insert_device(&pool).await?;
        let restart = queue(&pool, device, r#""Restart""#).await?;
        sqlx::query("UPDATE command_queue SET available_at = now() + interval '5 minutes'")
            .execute(&pool)
            .await?;

        assert_eq!(poll(&pool, device, Uuid::new_v4()).await?, None);
        assert!(!fetched(&pool, restart).await?);
        Ok(())
    }

    #[sqlx::test]
    #[ignore = "needs the compose Postgres; run with --test-threads=1"]
    async fn home_never_gives_a_restart_to_smithd(pool: PgPool) -> Result<()> {
        let device = insert_device(&pool).await?;
        let restart = queue(&pool, device, r#""Restart""#).await?;
        let ping = queue(&pool, device, r#""Ping""#).await?;

        let fetched_ids: Vec<i32> = crate::home::get_commands(device, "d", &pool)
            .await?
            .iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(fetched_ids, [ping]);
        assert!(!fetched(&pool, restart).await?);
        Ok(())
    }
}
