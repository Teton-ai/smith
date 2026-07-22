//! Harness for the daemon↔API end-to-end tests.
//!
//! The tests assume the compose stack is running (`make test.e2e`): postgres on
//! localhost:5432, the API on localhost:8080, and one device container running
//! the real smithd under systemd. They drive scenarios the way the dashboard
//! does — directly through Postgres — because dashboard routes need Auth0.
//!
//! Compatibility contract: the API container may be a released image with an
//! older schema (the version-skew CI job), so every query in this crate must
//! only touch tables/columns that already exist in released versions. Keep the
//! SQL boring and old.

use anyhow::{Context as _, Result, bail};
use sqlx::Row;
use sqlx::postgres::PgPool;
use std::future::Future;
use std::time::Duration;
use tokio::time::Instant;

/// One 20s idle poll tick plus generous headroom for slow CI runners.
pub const ROUND_TRIP_TIMEOUT: Duration = Duration::from_secs(90);
/// The upgrade scenario chains two round trips plus updater work.
pub const UPGRADE_TIMEOUT: Duration = Duration::from_secs(150);
/// After a token clear the device must re-register (one idle tick), then
/// fetch and answer the queued liveness ping.
const UNSTICK_TIMEOUT: Duration = Duration::from_secs(120);

const POLL_INTERVAL: Duration = Duration::from_secs(1);

pub struct Ctx {
    pub db: PgPool,
    pub http: reqwest::Client,
    pub api_url: String,
    pub serial: String,
}

impl Ctx {
    pub async fn connect() -> Result<Self> {
        let database_url = std::env::var("E2E_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string());
        let api_url =
            std::env::var("E2E_API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
        // With DEVICE_REPLICAS=1, smithd derives its serial from the docker
        // container name, which compose names deterministically.
        let serial =
            std::env::var("E2E_DEVICE_SERIAL").unwrap_or_else(|_| "smith-device-1".to_string());

        let db = PgPool::connect(&database_url)
            .await
            .with_context(|| format!("connecting to {database_url}"))?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .context("building http client")?;

        Ok(Self {
            db,
            http,
            api_url,
            serial,
        })
    }
}

/// Poll `probe` every second until it yields a value or `timeout` elapses.
pub async fn wait_until<T, F, Fut>(what: &str, timeout: Duration, mut probe: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Option<T>>>,
{
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = probe().await? {
            return Ok(value);
        }
        if Instant::now() >= deadline {
            bail!("timed out after {timeout:?} waiting for {what}");
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Wait for the API to answer `/health` and log which version we are testing
/// against (identifies the released image in the version-skew CI job).
pub async fn wait_for_api(ctx: &Ctx) -> Result<()> {
    let url = format!("{}/health", ctx.api_url);
    let body = wait_until("API /health to answer", ROUND_TRIP_TIMEOUT, || {
        let (http, url) = (ctx.http.clone(), url.clone());
        async move {
            let Ok(response) = http.get(&url).send().await else {
                return Ok(None);
            };
            if !response.status().is_success() {
                return Ok(None);
            }
            Ok(response.text().await.ok())
        }
    })
    .await?;
    println!("API under test: {body}");
    Ok(())
}

/// Idempotently seed what registration needs: token minting reads the
/// 'DEFAULT' variable preset, and the upgrade test needs a distribution to
/// hang releases off.
pub async fn seed_minimum(ctx: &Ctx) -> Result<()> {
    sqlx::query(
        "INSERT INTO variable_preset (title, description, variables)
         SELECT 'DEFAULT', 'DEFAULT', '[]'::jsonb
         WHERE NOT EXISTS (SELECT 1 FROM variable_preset WHERE title = 'DEFAULT')",
    )
    .execute(&ctx.db)
    .await
    .context("seeding variable_preset")?;

    sqlx::query(
        "INSERT INTO distribution (name, description, architecture)
         SELECT 'e2e', 'e2e', 'amd64'
         WHERE NOT EXISTS (SELECT 1 FROM distribution WHERE name = 'e2e')",
    )
    .execute(&ctx.db)
    .await
    .context("seeding distribution")?;

    Ok(())
}

/// Wait for the device to self-register, approve it, and prove it is online
/// with a Ping→Pong round trip through the real daemon. Returns the device id.
///
/// The round trip is the only trustworthy liveness signal: `last_ping` can be
/// fresh from a previous device container right after a stack restart, while
/// the current container is actually stuck registering. If the ping goes
/// unanswered, the device is assumed deadlocked on 409 (a recreated container
/// loses its token file while the DB keeps the old token), so the token is
/// cleared to let registration mint a new one, and the same queued ping is
/// awaited again — it gets delivered once the device is back.
pub async fn ensure_device_online(ctx: &Ctx) -> Result<i32> {
    let device_id = wait_until(
        &format!("device '{}' to self-register", ctx.serial),
        ROUND_TRIP_TIMEOUT,
        || async {
            let row = sqlx::query("SELECT id FROM device WHERE serial_number = $1")
                .bind(&ctx.serial)
                .fetch_optional(&ctx.db)
                .await
                .context("looking up device row")?;
            Ok(row.map(|row| row.get::<i32, _>("id")))
        },
    )
    .await?;

    sqlx::query("UPDATE device SET approved = true WHERE id = $1")
        .bind(device_id)
        .execute(&ctx.db)
        .await
        .context("approving device")?;

    let ping_id = enqueue(ctx, device_id, r#""Ping""#).await?;
    if wait_for_pong(ctx, ping_id, ROUND_TRIP_TIMEOUT)
        .await
        .is_ok()
    {
        return Ok(device_id);
    }

    sqlx::query("UPDATE device SET token = NULL WHERE id = $1")
        .bind(device_id)
        .execute(&ctx.db)
        .await
        .context("clearing token to unstick registration")?;

    wait_for_pong(ctx, ping_id, UNSTICK_TIMEOUT)
        .await
        .with_context(|| {
            format!(
                "device '{}' still unresponsive after clearing its token",
                ctx.serial
            )
        })?;
    Ok(device_id)
}

async fn wait_for_pong(ctx: &Ctx, ping_id: i32, timeout: Duration) -> Result<()> {
    wait_until(
        &format!("Pong response to liveness ping {ping_id}"),
        timeout,
        || async {
            let row = sqlx::query("SELECT id FROM command_response WHERE command_id = $1")
                .bind(ping_id)
                .fetch_optional(&ctx.db)
                .await
                .context("polling for liveness pong")?;
            Ok(row.map(|_| ()))
        },
    )
    .await
}

/// Enqueue a command for the device, exactly as the dashboard does.
/// `cmd_json` is the externally-tagged SafeCommandTx JSON, e.g. `"Ping"` or
/// `{"FreeForm":{"cmd":"echo hi"}}`. Returns the command id.
pub async fn enqueue(ctx: &Ctx, device_id: i32, cmd_json: &str) -> Result<i32> {
    let row = sqlx::query(
        "WITH bundle AS (
             INSERT INTO command_bundles DEFAULT VALUES RETURNING uuid
         )
         INSERT INTO command_queue (device_id, cmd, continue_on_error, bundle)
         SELECT $1, $2::json, false, bundle.uuid FROM bundle
         RETURNING id",
    )
    .bind(device_id)
    .bind(cmd_json)
    .fetch_one(&ctx.db)
    .await
    .with_context(|| format!("enqueuing command {cmd_json}"))?;
    Ok(row.get::<i32, _>("id"))
}

/// Wait for the device's response to a queued command.
/// Returns the response JSON and its status code.
pub async fn wait_for_response(ctx: &Ctx, command_id: i32) -> Result<(serde_json::Value, i32)> {
    wait_until(
        &format!("response to command {command_id}"),
        ROUND_TRIP_TIMEOUT,
        || async {
            let row = sqlx::query(
                "SELECT response::text AS response, status
                 FROM command_response WHERE command_id = $1",
            )
            .bind(command_id)
            .fetch_optional(&ctx.db)
            .await
            .context("polling command_response")?;
            let Some(row) = row else {
                return Ok(None);
            };
            let response = match row.get::<Option<String>, _>("response") {
                Some(text) => {
                    serde_json::from_str(&text).context("parsing command response JSON")?
                }
                None => serde_json::Value::Null,
            };
            Ok(Some((response, row.get::<i32, _>("status"))))
        },
    )
    .await
}
