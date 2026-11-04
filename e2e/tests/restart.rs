//! The restart channel end to end: with smithd frozen, a Restart still reaches
//! smith-rescue, the device container reboots, and the api records the reboot.
//!
//!     cargo test -p smith-e2e -- --ignored --test-threads=1

use anyhow::{Context as _, Result, ensure};
use smith_e2e::{
    Ctx, enqueue, ensure_device_online, seed_minimum, wait_for_api, wait_for_response, wait_until,
};
use sqlx::Row;
use std::time::Duration;

/// A poll each minute, the reboot, the container start, and the first poll of
/// the new boot.
const REBOOT_TIMEOUT: Duration = Duration::from_secs(300);
/// smithd polls /home at least every 20 s, so a stale last_ping means it is frozen.
const FROZEN_AFTER: Duration = Duration::from_secs(45);

/// The restart channel tables do not exist in a released api image.
async fn has_restart_channel(ctx: &Ctx) -> Result<bool> {
    sqlx::query_scalar("SELECT to_regclass('public.device_restart_channel') IS NOT NULL")
        .fetch_one(&ctx.db)
        .await
        .context("checking for device_restart_channel")
}

/// A device image built from a release before smith-rescue does not have it.
async fn has_smith_rescue(ctx: &Ctx, device_id: i32) -> Result<bool> {
    let id = enqueue(
        ctx,
        device_id,
        r#"{"FreeForm":{"cmd":"test -x /usr/bin/smith-rescue"}}"#,
    )
    .await?;
    let (_, status) = wait_for_response(ctx, id).await?;
    Ok(status == 0)
}

/// The boot id of smith-rescue's last poll, once it polled in the last minutes.
async fn polled_boot(ctx: &Ctx, device_id: i32) -> Result<String> {
    wait_until(
        "smith-rescue to poll the restart channel",
        REBOOT_TIMEOUT,
        || async {
            let row = sqlx::query(
                "SELECT boot_id::text AS boot_id FROM device_restart_channel
                 WHERE device_id = $1 AND last_poll_at > now() - interval '2 minutes'",
            )
            .bind(device_id)
            .fetch_optional(&ctx.db)
            .await
            .context("reading the restart channel")?;
            Ok(row.map(|row| row.get::<String, _>("boot_id")))
        },
    )
    .await
}

#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn a_restart_reboots_a_device_whose_smithd_is_frozen() -> Result<()> {
    let ctx = Ctx::connect().await?;
    wait_for_api(&ctx).await?;
    seed_minimum(&ctx).await?;
    let device_id = ensure_device_online(&ctx).await?;

    if !has_restart_channel(&ctx).await? {
        println!("skipping: the api under test has no restart channel");
        return Ok(());
    }
    if !has_smith_rescue(&ctx, device_id).await? {
        println!("skipping: the device under test has no smith-rescue");
        return Ok(());
    }
    let boot_before = polled_boot(&ctx, device_id).await?;

    // SIGSTOP leaves smithd running but stuck: no poll loop, no executor.
    enqueue(
        &ctx,
        device_id,
        r#"{"FreeForm":{"cmd":"kill -STOP $(systemctl show --property=MainPID --value smithd)"}}"#,
    )
    .await?;
    wait_until("smithd to stop polling /home", REBOOT_TIMEOUT, || async {
        let stale: bool = sqlx::query_scalar(
            "SELECT last_ping < now() - make_interval(secs => $2) FROM device WHERE id = $1",
        )
        .bind(device_id)
        .bind(FROZEN_AFTER.as_secs_f64())
        .fetch_one(&ctx.db)
        .await
        .context("reading last_ping")?;
        Ok(stale.then_some(()))
    })
    .await?;

    let restart = enqueue(&ctx, device_id, r#""Restart""#).await?;
    let (message, status) = wait_until("the api to record the reboot", REBOOT_TIMEOUT, || async {
        let row = sqlx::query(
            "SELECT response->'Restart'->>'message' AS message, status
                 FROM command_response WHERE command_id = $1",
        )
        .bind(restart)
        .fetch_optional(&ctx.db)
        .await
        .context("polling the Restart result")?;
        Ok(row.map(|row| {
            (
                row.get::<Option<String>, _>("message"),
                row.get::<i32, _>("status"),
            )
        }))
    })
    .await?;
    ensure!(status == 0, "Restart finished with status {status}");
    let message = message.unwrap_or_default();
    ensure!(
        message.contains(&boot_before),
        "the result does not name the boot that ended: {message}"
    );

    let boot_after = polled_boot(&ctx, device_id).await?;
    ensure!(
        boot_after != boot_before,
        "the device polls from the same boot"
    );

    // The new boot runs a fresh smithd that answers commands again.
    ensure_device_online(&ctx).await?;
    Ok(())
}
