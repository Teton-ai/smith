//! Daemon↔API end-to-end scenarios. Each test is self-sufficient (own seed +
//! device check, own commands/releases) so they tolerate any order and dirty
//! databases; run them serially since they share the single device container:
//!
//!     cargo test -p smith-e2e -- --ignored --test-threads=1

use anyhow::{Context as _, Result, ensure};
use smith_e2e::{
    Ctx, UPGRADE_TIMEOUT, enqueue, ensure_device_online, seed_minimum, wait_for_api,
    wait_for_response, wait_until,
};
use sqlx::Row;

async fn setup() -> Result<(Ctx, i32)> {
    let ctx = Ctx::connect().await?;
    wait_for_api(&ctx).await?;
    seed_minimum(&ctx).await?;
    let device_id = ensure_device_online(&ctx).await?;
    Ok((ctx, device_id))
}

#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn device_registers_and_comes_online() -> Result<()> {
    let (ctx, device_id) = setup().await?;

    let row = sqlx::query(
        "SELECT approved, token, last_ping IS NOT NULL AS has_ping
         FROM device WHERE id = $1",
    )
    .bind(device_id)
    .fetch_one(&ctx.db)
    .await
    .context("reading device row")?;

    ensure!(row.get::<bool, _>("approved"), "device should be approved");
    let token = row
        .get::<Option<String>, _>("token")
        .context("device should hold a token")?;
    ensure!(!token.is_empty(), "device token should not be empty");
    ensure!(row.get::<bool, _>("has_ping"), "device should have pinged");
    Ok(())
}

#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn freeform_command_round_trip() -> Result<()> {
    let (ctx, device_id) = setup().await?;

    let marker = format!("e2e-{}", uuid::Uuid::new_v4());
    let command_id = enqueue(
        &ctx,
        device_id,
        &format!(r#"{{"FreeForm":{{"cmd":"echo {marker}"}}}}"#),
    )
    .await?;

    let (response, status) = wait_for_response(&ctx, command_id).await?;
    ensure!(status == 0, "expected status 0, got {status}: {response}");
    let stdout = response["FreeForm"]["stdout"]
        .as_str()
        .with_context(|| format!("expected FreeForm response, got {response}"))?;
    ensure!(
        stdout.contains(&marker),
        "stdout should contain {marker}, got {stdout:?}"
    );

    let fetched = sqlx::query("SELECT fetched FROM command_queue WHERE id = $1")
        .bind(command_id)
        .fetch_one(&ctx.db)
        .await
        .context("reading command_queue row")?
        .get::<bool, _>("fetched");
    ensure!(fetched, "command should be marked fetched");
    Ok(())
}

#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn ping_pong_round_trip() -> Result<()> {
    let (ctx, device_id) = setup().await?;

    let command_id = enqueue(&ctx, device_id, r#""Ping""#).await?;
    let (response, status) = wait_for_response(&ctx, command_id).await?;

    ensure!(status == 0, "expected status 0, got {status}: {response}");
    ensure!(
        response == serde_json::json!("Pong"),
        "expected \"Pong\", got {response}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires running compose stack; use make test.e2e"]
async fn empty_release_upgrade_reports_release_id() -> Result<()> {
    let (ctx, device_id) = setup().await?;

    // A release with no packages exercises the full upgrade loop (manifest
    // fetch, updater run, release_id report-back) without needing S3.
    let version = format!("0.0.0-e2e-{}", uuid::Uuid::new_v4());
    let release_id = sqlx::query(
        "INSERT INTO release (distribution_id, version, draft)
         SELECT id, $1, false FROM distribution WHERE name = 'e2e'
         RETURNING id",
    )
    .bind(&version)
    .fetch_one(&ctx.db)
    .await
    .context("creating empty release")?
    .get::<i32, _>("id");

    sqlx::query("UPDATE device SET target_release_id = $1 WHERE id = $2")
        .bind(release_id)
        .bind(device_id)
        .execute(&ctx.db)
        .await
        .context("targeting release")?;

    // The Upgrade command makes the daemon upgrade immediately instead of
    // waiting for the updater's 60s check tick.
    enqueue(&ctx, device_id, r#""Upgrade""#).await?;

    wait_until(
        &format!("device to report release_id {release_id}"),
        UPGRADE_TIMEOUT,
        || async {
            let row = sqlx::query("SELECT id FROM device WHERE id = $1 AND release_id = $2")
                .bind(device_id)
                .bind(release_id)
                .fetch_optional(&ctx.db)
                .await
                .context("polling device release_id")?;
            Ok(row.map(|_| ()))
        },
    )
    .await?;
    Ok(())
}
