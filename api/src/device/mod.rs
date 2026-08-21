use crate::config::Config;
use crate::slack::send_slack_notification;
use models::release::Release;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Value, json};
use smith::utils::schema::{DeviceRegistration, DeviceRegistrationResponse};
use sqlx::PgPool;
use sqlx::types::chrono::{DateTime, Utc};
use sqlx::types::{Json as SqlxJson, chrono, ipnetwork};
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub mod route;

// TODO: Change this, this needs to be device and the other is PublicDevice, API type
#[derive(Debug, Serialize, utoipa::ToSchema, Clone)]
pub struct RawDevice {
    pub id: i32,
    pub serial_number: String,
    #[schema(value_type = HashMap<String, String>)]
    pub labels: SqlxJson<HashMap<String, String>>,
    pub last_ping: Option<DateTime<Utc>>,
    pub wifi_mac: Option<String>,
    pub modified_on: DateTime<Utc>,
    pub created_on: DateTime<Utc>,
    pub note: Option<String>,
    pub approved: bool,
    #[serde(serialize_with = "serialize_token_presence")]
    pub token: Option<String>,
    pub release_id: Option<i32>,
    pub target_release_id: Option<i32>,
    pub target_release_id_set_at: Option<DateTime<Utc>>,
    pub system_info: Option<serde_json::Value>,
    pub network_id: Option<i32>,
    pub current_network_id: Option<i32>,
    pub modem_id: Option<i32>,
    pub archived: bool,
    pub ip_address_id: Option<i64>,
    pub intent_version: i32,
    pub observed_intent_version: Option<i32>,
    pub network_conditions: Option<serde_json::Value>,
}

fn serialize_token_presence<S>(token: &Option<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match token {
        Some(_) => serializer.serialize_str("[REDACTED]"),
        None => serializer.serialize_none(),
    }
}

#[derive(Deserialize, Debug)]
struct IpApiResponse {
    status: String,
    country: Option<String>,
    #[serde(rename = "countryCode")]
    country_code: Option<String>,
    region: Option<String>,
    city: Option<String>,
    isp: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    proxy: Option<bool>,
    hosting: Option<bool>,
    continent: Option<String>,
    #[serde(rename = "continentCode")]
    continent_code: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct UpdateDeviceRelease {
    pub target_release_id: i32,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ApproveDeviceBody {
    pub target_release_id: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct UpdateDevicesRelease {
    pub target_release_id: i32,
    pub devices: Vec<i32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Variable {
    pub id: i32,
    pub device: i32,
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewVariable {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct Note {
    pub note: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeviceLedgerItem {
    pub id: i32,
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub class: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeviceLedgerItemPaginated {
    pub ledger: Vec<DeviceLedgerItem>,
    pub next: Option<String>,
    pub previous: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeviceRelease {
    pub previous_release: Option<Release>,
    pub release: Option<Release>,
    pub target_release: Option<Release>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeviceHealth {
    pub id: i32,
    pub serial_number: String,
    pub last_ping: Option<chrono::DateTime<chrono::Utc>>,
    pub is_healthy: Option<bool>,
}

/// One recorded gap in a service's availability. `ended_at` is null while the
/// outage is still open.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ServiceOutage {
    pub service_name: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

/// Availability of a device's services over a time window.
///
/// The outages are returned as intervals rather than a per-bucket up/down
/// series: the client draws the bands directly, so the payload stays a handful
/// of rows instead of one per service per bucket.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeviceUptime {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// Every service to draw a lane for, including ones that never went down.
    pub services: Vec<String>,
    /// Outages overlapping the window, clipped to it by the caller when drawing.
    pub outages: Vec<ServiceOutage>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LabelWithValues {
    pub key: String,
    pub values: Vec<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ConfiguredNetwork {
    pub network_id: i32,
    pub profile_name: String,
    pub ssid: Option<String>,
    pub name: String,
    pub password: Option<String>,
    pub security_type: Option<String>,
    pub is_network_hidden: bool,
    /// Full credential envelope; `password` above only mirrors its `psk` key.
    pub credentials: Value,
    /// EAP username, set only for enterprise profiles.
    pub identity: Option<Value>,
    pub is_active: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct WifiScanResult {
    pub ssid: Option<String>,
    pub bssid: String,
    pub signal: Option<i32>,
    pub rate: Option<i32>,
    pub security: Option<String>,
    pub channel: Option<i32>,
    pub band: Option<String>,
    pub scanned_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DeviceNetworkIntent {
    pub id: i32,
    pub device_id: i32,
    pub network_id: i32,
    pub ssid: Option<String>,
    pub name: String,
    pub network_type: String,
    pub security_type: Option<String>,
    pub is_network_hidden: bool,
    /// Full credential envelope from the network's `credentials` column.
    pub credentials: Value,
    /// EAP username, set only for enterprise profiles.
    pub identity: Option<Value>,
    pub priority: i32,
    pub managed_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateIntentRequest {
    pub network_id: i32,
    pub managed_by: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PatchIntentRequest {
    pub priority: Option<i32>,
    pub managed_by: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ApplyIntentResponse {
    pub bundle_uuid: Uuid,
    pub command_id: i32,
}

async fn update_ip_geolocation(
    ip_address: IpAddr,
    ip_id: i64,
    api_key: &str,
    pool: &PgPool,
) -> anyhow::Result<()> {
    // Build URL with HTTPS and field filtering
    let fields =
        "status,continent,continentCode,country,countryCode,region,city,lat,lon,isp,proxy,hosting";
    let url = format!(
        "https://pro.ip-api.com/json/{}?key={}&fields={}",
        ip_address, api_key, fields
    );

    // Build client with sensible timeouts
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()?;

    // Simple retry logic with exponential backoff
    let mut retry_count = 0;
    const MAX_RETRIES: u32 = 3;
    const BASE_DELAY_MS: u64 = 500;

    loop {
        match client.get(&url).send().await {
            Ok(response) => {
                // Check HTTP status before parsing JSON
                if let Err(e) = response.error_for_status_ref() {
                    error!(
                        "IP-API returned HTTP error for {} (attempt {}): {}",
                        ip_address,
                        retry_count + 1,
                        e
                    );

                    if retry_count < MAX_RETRIES {
                        retry_count += 1;
                        let delay = Duration::from_millis(BASE_DELAY_MS * (1 << (retry_count - 1)));
                        tokio::time::sleep(delay).await;
                        continue;
                    } else {
                        return Err(anyhow::anyhow!(
                            "IP-API HTTP error after {} retries: {}",
                            MAX_RETRIES,
                            e
                        ));
                    }
                }

                // Parse JSON response
                match response.json::<IpApiResponse>().await {
                    Ok(api_response) => {
                        if api_response.status == "success" {
                            // Update the database with geolocation data using sqlx::query for POINT support
                            let query = r#"
                                UPDATE ip_address
                                SET
                                    continent = $2,
                                    continent_code = $3,
                                    country_code = $4,
                                    country = $5,
                                    region = $6,
                                    city = $7,
                                    isp = $8,
                                    coordinates = CASE
                                        WHEN $9::float8 IS NOT NULL AND $10::float8 IS NOT NULL
                                        THEN point($9, $10)
                                        ELSE NULL
                                    END,
                                    proxy = $11,
                                    hosting = $12,
                                    updated_at = NOW()
                                WHERE id = $1
                            "#;

                            sqlx::query(query)
                                .bind(ip_id)
                                .bind(&api_response.continent)
                                .bind(&api_response.continent_code)
                                .bind(&api_response.country_code)
                                .bind(&api_response.country)
                                .bind(&api_response.region)
                                .bind(&api_response.city)
                                .bind(&api_response.isp)
                                .bind(api_response.lon)
                                .bind(api_response.lat)
                                .bind(api_response.proxy)
                                .bind(api_response.hosting)
                                .execute(pool)
                                .await?;

                            debug!("Updated geolocation for IP {} (ID: {})", ip_address, ip_id);
                            return Ok(());
                        } else {
                            warn!(
                                "IP-API returned error status for {}: {}",
                                ip_address, api_response.status
                            );
                            return Ok(()); // Don't retry on API-level errors (e.g., invalid IP, quota exceeded)
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse IP-API JSON response for {} (attempt {}): {}",
                            ip_address,
                            retry_count + 1,
                            e
                        );

                        if retry_count < MAX_RETRIES {
                            retry_count += 1;
                            let delay =
                                Duration::from_millis(BASE_DELAY_MS * (1 << (retry_count - 1)));
                            tokio::time::sleep(delay).await;
                            continue;
                        } else {
                            return Err(anyhow::anyhow!(
                                "Failed to parse IP-API response after {} retries: {}",
                                MAX_RETRIES,
                                e
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                warn!(
                    "Network error calling IP-API for {} (attempt {}): {}",
                    ip_address,
                    retry_count + 1,
                    e
                );

                if retry_count < MAX_RETRIES {
                    retry_count += 1;
                    let delay = Duration::from_millis(BASE_DELAY_MS * (1 << (retry_count - 1)));
                    tokio::time::sleep(delay).await;
                    continue;
                } else {
                    return Err(anyhow::anyhow!(
                        "Network error calling IP-API after {} retries: {}",
                        MAX_RETRIES,
                        e
                    ));
                }
            }
        }
    }
}

pub async fn register_device(
    payload: DeviceRegistration,
    pool: &PgPool,
    config: &Config,
) -> anyhow::Result<DeviceRegistrationResponse, RegistrationError> {
    let mut tx = pool.begin().await?;

    let serial_sanitized = payload
        .serial_number
        .trim()
        .trim_matches(char::is_whitespace)
        .trim_matches(char::from(0));

    let query = r#"
            WITH existing_device AS (
                SELECT id, serial_number, token, approved, false AS was_inserted
                FROM device
                WHERE serial_number = $1
            ),
            insert_if_missing AS (
                INSERT INTO device (serial_number, token)
                SELECT $1, NULL
                WHERE NOT EXISTS (SELECT 1 FROM existing_device)
                RETURNING id, serial_number, token, NULL::boolean AS approved, true AS was_inserted
            )
            SELECT id, serial_number, token, approved, was_inserted
            FROM existing_device
            UNION ALL
            SELECT id, serial_number, token, approved, was_inserted
            FROM insert_if_missing;
        "#;

    #[derive(sqlx::FromRow)]
    struct DeviceRow {
        id: i32,
        serial_number: String,
        token: Option<String>,
        approved: Option<bool>,
        was_inserted: bool,
    }

    let result: DeviceRow = sqlx::query_as::<_, DeviceRow>(query)
        .bind(serial_sanitized)
        .fetch_one(&mut *tx)
        .await?;

    if result.was_inserted {
        sqlx::query!(
            "INSERT INTO ledger (device_id, class, text) VALUES ($1, $2, $3);",
            result.id,
            "registration",
            format!("Registered {}", result.serial_number)
        )
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to log registration to ledger {err}");
            RegistrationError::FailedToLogInLedger
        })?;

        if let Some(slack_hook_url) = &config.slack_hook_url {
            let message = json!({
                "blocks": [
                    {
                        "type": "section",
                        "text": {
                            "type": "mrkdwn",
                            "text": format!(
                                "New device *{}* has registered via the API. Welcome to the fleet! :tada: :hardware:",
                                result.serial_number,
                            )
                        }
                    },
                ]
            });
            send_slack_notification(slack_hook_url, message).await;
        }
    }

    if result.approved == Some(true) {
        match result.token {
            Some(_) => {
                tx.rollback().await?;
                info!(
                    "Device {} is already registered, and has a token",
                    result.serial_number
                );
                return Err(RegistrationError::NotNullTokenError);
            }
            None => {
                let update_query = r#"
                    UPDATE device
                    SET token = gen_random_uuid()::text
                    WHERE serial_number = $1
                    RETURNING token;
                    "#;

                let updated_result: (String,) = sqlx::query_as(update_query)
                    .bind(serial_sanitized)
                    .fetch_one(&mut *tx)
                    .await?;

                let result_vars: Value = sqlx::query_scalar!(
                    "SELECT variables FROM variable_preset WHERE title = 'DEFAULT'"
                )
                .fetch_one(&mut *tx)
                .await
                .map_err(|err| {
                    error!("Failed to fetch variables preset {err}");
                    RegistrationError::DatabaseError(err)
                })?;

                for (name, value) in result_vars
                    .as_array()
                    .expect("error: failed to get variable as array")
                    .iter()
                    .map(|json_value| {
                        (
                            json_value
                                .get("name")
                                .and_then(|n| n.as_str())
                                .expect("error: failed to access name as string"),
                            json_value
                                .get("value")
                                .and_then(|v| v.as_str())
                                .expect("error: failed to access value as string"),
                        )
                    })
                {
                    sqlx::query!(
                        r#"INSERT INTO variable (name, value, device)
                            VALUES ($1, $2, $3)
                            ON CONFLICT (device, name)
                            DO UPDATE SET value = EXCLUDED.value"#,
                        name,
                        value,
                        result.id,
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(|err| {
                        error!("Failed to insert variable for device {err}");
                        RegistrationError::DatabaseError(err)
                    })?;
                }

                tx.commit().await?;
                return Ok(DeviceRegistrationResponse {
                    token: updated_result.0,
                });
            }
        }
    }

    tx.commit().await?;
    Err(RegistrationError::NotApprovedDevice)
}

/// A device pings `/smith/home` every 20s when idle (`smithd/src/postman/mod.rs`),
/// so this tolerates several consecutive misses before calling it down. Outages
/// shorter than this are deliberately not recorded.
const DOWNTIME_STALE_AFTER_SECS: f64 = 90.0;

/// Detection latency does not affect the recorded outage, because `started_at`
/// is backdated to the last successful ping. Only alerting freshness suffers,
/// so the sweep can be infrequent.
const DOWNTIME_SWEEP_INTERVAL_SECS: u64 = 300;

/// After a restart every device looks stale until it pings again. Wait out
/// several ping cycles before concluding anything.
const DOWNTIME_STARTUP_GRACE_SECS: u64 = 120;

/// If more than this fraction of the observable fleet looks stale at once, the
/// API or its network was down rather than the devices. Skipping the sweep is
/// far better than writing one bogus outage per device on every deploy.
const DOWNTIME_MASS_STALE_RATIO: f64 = 0.5;

/// Below this fleet size the ratio check is meaningless — one dev device being
/// switched off would trip it — so it only applies above this many devices.
const DOWNTIME_MASS_STALE_MIN_FLEET: i64 = 10;

/// Downtime is only meaningful from the moment the API began observing it.
/// A device last seen before that was never witnessed up, so claiming an
/// outage for it would be fabrication. This migration's own `installed_on` is
/// exactly that moment, which beats a config knob that can be set wrong and a
/// startup timestamp that would drift on every restart.
const DOWNTIME_EPOCH_MIGRATION_VERSION: i64 = 20260727000000;

/// Device reachability is stored as an outage of smithd itself, which is a real
/// systemd unit (`smithd/debian/smithd.service`), so one table and one set of
/// queries cover both reachability and per-service health.
///
/// These rows are owned exclusively by the sweeper below. smithd cannot report
/// its own death, so its state is the only one inferred from silence rather than
/// read from a device report — see `save_service_statuses`, which refuses to
/// write this name for exactly that reason.
pub const SMITHD_SERVICE_NAME: &str = "smithd";

/// Opens an outage row for every non-archived device that has gone silent and
/// does not already have one open. `started_at` is backdated to the device's
/// last ping so a coarse sweep interval still yields accurate history.
///
/// Safe to run concurrently from several API replicas: the partial unique index
/// on open rows turns a lost race into a no-op instead of a duplicate.
pub async fn open_downtime_for_silent_devices(pool: &PgPool) -> anyhow::Result<u64> {
    let counts = sqlx::query!(
        r#"
        SELECT
            COUNT(*) FILTER (
                WHERE d.last_ping IS NOT NULL AND d.last_ping >= m.installed_on
            ) AS "observable!",
            COUNT(*) FILTER (
                WHERE d.last_ping IS NOT NULL
                  AND d.last_ping >= m.installed_on
                  AND d.last_ping < NOW() - make_interval(secs => $2::double precision)
            ) AS "stale!"
        FROM device d
        CROSS JOIN (
            SELECT installed_on FROM _sqlx_migrations WHERE version = $1
        ) m
        WHERE d.archived = false
        "#,
        DOWNTIME_EPOCH_MIGRATION_VERSION,
        DOWNTIME_STALE_AFTER_SECS,
    )
    .fetch_one(pool)
    .await?;

    if counts.observable >= DOWNTIME_MASS_STALE_MIN_FLEET {
        let stale_ratio = counts.stale as f64 / counts.observable as f64;
        if stale_ratio > DOWNTIME_MASS_STALE_RATIO {
            error!(
                stale = counts.stale,
                observable = counts.observable,
                "Skipping downtime sweep: {:.0}% of the fleet is stale at once, which means the API or its network was down, not the devices",
                stale_ratio * 100.0
            );
            return Ok(0);
        }
    }

    let opened = sqlx::query!(
        r#"
        INSERT INTO device_service_outage (device_id, service_name, started_at)
        SELECT d.id, $3, d.last_ping
        FROM device d
        CROSS JOIN (
            SELECT installed_on FROM _sqlx_migrations WHERE version = $1
        ) m
        WHERE d.archived = false
          AND d.last_ping IS NOT NULL
          AND d.last_ping >= m.installed_on
          AND d.last_ping < NOW() - make_interval(secs => $2::double precision)
          AND NOT EXISTS (
              SELECT 1 FROM device_service_outage o
              WHERE o.device_id = d.id
                AND o.service_name = $3
                AND o.ended_at IS NULL
          )
        ON CONFLICT DO NOTHING
        "#,
        DOWNTIME_EPOCH_MIGRATION_VERSION,
        DOWNTIME_STALE_AFTER_SECS,
        SMITHD_SERVICE_NAME,
    )
    .execute(pool)
    .await?
    .rows_affected();

    Ok(opened)
}

/// Runs the downtime sweep on its own task so it never sits in the path of a
/// device's `/smith/home` request or its command fetch.
pub fn spawn_downtime_sweeper(pool: PgPool) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(DOWNTIME_STARTUP_GRACE_SECS)).await;

        let mut ticker = tokio::time::interval(Duration::from_secs(DOWNTIME_SWEEP_INTERVAL_SECS));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            ticker.tick().await;
            match open_downtime_for_silent_devices(&pool).await {
                Ok(0) => debug!("Downtime sweep found no newly silent devices"),
                Ok(opened) => info!("Downtime sweep opened {opened} outage(s)"),
                Err(err) => error!("Downtime sweep failed: {err:?}"),
            }
        }
    });
}

/// Reconciles the outage a device just recovered from, given the ping timestamp
/// it replaced.
///
/// Runs inside the caller's transaction on purpose: the `UPDATE device` that
/// produced `previous_last_ping` holds a row lock on that device, which
/// serializes concurrent pings for the same device and keeps the one-open-row
/// invariant intact without any extra locking.
async fn close_open_downtime(
    device_id: i32,
    previous_last_ping: DateTime<Utc>,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> anyhow::Result<()> {
    let closed = sqlx::query!(
        "UPDATE device_service_outage SET ended_at = NOW()
         WHERE device_id = $1 AND service_name = $2 AND ended_at IS NULL",
        device_id,
        SMITHD_SERVICE_NAME,
    )
    .execute(&mut **tx)
    .await?
    .rows_affected();

    if closed == 0 {
        // The device recovered before any sweep noticed it was gone, so record
        // the already-finished outage rather than losing it entirely — but only
        // from the observation epoch onwards, on the same terms as the sweeper.
        // A ping that replaces a pre-epoch one closes a gap nobody witnessed.
        sqlx::query!(
            "INSERT INTO device_service_outage (device_id, service_name, started_at, ended_at)
             SELECT $1, $2, $3, NOW()
             WHERE $3 >= (SELECT installed_on FROM _sqlx_migrations WHERE version = $4)",
            device_id,
            SMITHD_SERVICE_NAME,
            previous_last_ping,
            DOWNTIME_EPOCH_MIGRATION_VERSION,
        )
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

pub async fn save_last_ping_with_ip(
    device_id: i32,
    ip_address: Option<IpAddr>,
    pool: &PgPool,
    config: &Config,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    match ip_address {
        Some(ip) => {
            let ip_network: ipnetwork::IpNetwork = ip.into();

            // SELECT first to avoid burning an IDENTITY sequence value on every ping —
            // ON CONFLICT DO NOTHING still advances the sequence on conflict.
            let existing = sqlx::query!(
                r#"
                    SELECT id,
                           CASE
                               WHEN updated_at < NOW() - INTERVAL '24 hours' THEN true
                               ELSE false
                           END as needs_update
                    FROM ip_address
                    WHERE ip_address = $1
                "#,
                ip_network
            )
            .fetch_optional(&mut *tx)
            .await?;

            let (ip_id, should_update_geolocation) = match existing {
                Some(record) => (record.id, record.needs_update.unwrap_or(false)),
                None => {
                    let inserted = sqlx::query!(
                        "INSERT INTO ip_address (ip_address, created_at) VALUES ($1, NOW()) ON CONFLICT (ip_address) DO NOTHING RETURNING id",
                        ip_network
                    )
                    .fetch_optional(&mut *tx)
                    .await?;
                    match inserted {
                        Some(record) => (record.id, true),
                        None => {
                            // Concurrent insert won the race; fetch the now-existing row.
                            let row = sqlx::query!(
                                "SELECT id FROM ip_address WHERE ip_address = $1",
                                ip_network
                            )
                            .fetch_one(&mut *tx)
                            .await?;
                            (row.id, true)
                        }
                    }
                }
            };

            // The CTE reads the pre-UPDATE snapshot, so this hands back both the
            // ping timestamp being replaced and whether the gap was long enough
            // to have been recorded as an outage — decided on the database
            // clock, with no extra round trip. For a device pinging on cadence
            // nothing further runs, which matters at hundreds of pings/sec.
            let ping = sqlx::query!(
                r#"
                WITH prev AS (SELECT last_ping FROM device WHERE id = $1)
                UPDATE device SET last_ping = NOW(), ip_address_id = $2
                WHERE id = $1
                RETURNING
                    (SELECT last_ping FROM prev) AS previous_last_ping,
                    (SELECT last_ping < NOW() - make_interval(secs => $3::double precision)
                     FROM prev) AS was_stale
                "#,
                device_id,
                ip_id,
                DOWNTIME_STALE_AFTER_SECS,
            )
            .fetch_optional(&mut *tx)
            .await?;

            if let Some(ping) = ping
                && ping.was_stale.unwrap_or(false)
                && let Some(previous) = ping.previous_last_ping
            {
                close_open_downtime(device_id, previous, &mut tx).await?;
            }

            tx.commit().await?;

            // If geolocation data needs updating and API key is available, spawn a background task
            if should_update_geolocation {
                if let Some(api_key) = &config.ip_api_key {
                    let pool_clone = pool.clone();
                    let api_key_clone = api_key.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            update_ip_geolocation(ip, ip_id, &api_key_clone, &pool_clone).await
                        {
                            error!("Failed to update geolocation for IP {}: {}", ip, e);
                        }
                    });
                } else {
                    debug!(
                        "IP-API key not configured, skipping geolocation update for IP {}",
                        ip
                    );
                }
            }
        }
        None => {
            let ping = sqlx::query!(
                r#"
                WITH prev AS (SELECT last_ping FROM device WHERE id = $1)
                UPDATE device SET last_ping = NOW()
                WHERE id = $1
                RETURNING
                    (SELECT last_ping FROM prev) AS previous_last_ping,
                    (SELECT last_ping < NOW() - make_interval(secs => $2::double precision)
                     FROM prev) AS was_stale
                "#,
                device_id,
                DOWNTIME_STALE_AFTER_SECS,
            )
            .fetch_optional(&mut *tx)
            .await?;

            if let Some(ping) = ping
                && ping.was_stale.unwrap_or(false)
                && let Some(previous) = ping.previous_last_ping
            {
                close_open_downtime(device_id, previous, &mut tx).await?;
            }

            tx.commit().await?;
        }
    }
    Ok(())
}

pub async fn get_target_release(device_id: i32, pool: &PgPool) -> Option<i32> {
    if let Ok(device) = sqlx::query!(
        "SELECT target_release_id FROM device WHERE id = $1",
        &device_id
    )
    .fetch_one(pool)
    .await
    {
        return device.target_release_id;
    }
    None
}

pub async fn save_release_id(
    device_id: i32,
    release_id: Option<i32>,
    pool: &PgPool,
) -> anyhow::Result<()> {
    if let Some(new_release_id) = release_id {
        let mut tx = pool.begin().await?;

        let current = sqlx::query!("SELECT release_id FROM device WHERE id = $1", device_id)
            .fetch_one(&mut *tx)
            .await?;

        if current.release_id != Some(new_release_id) {
            sqlx::query!(
                "UPDATE device SET release_id = $1 WHERE id = $2",
                new_release_id,
                device_id,
            )
            .execute(&mut *tx)
            .await?;

            if let Some(previous_release_id) = current.release_id {
                sqlx::query!(
                    "
                        INSERT INTO device_release_upgrades
                        (device_id, previous_release_id, upgraded_release_id)
                        VALUES ($1, $2, $3)
                        ",
                    device_id,
                    previous_release_id,
                    new_release_id
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        tx.commit().await?;
    }
    Ok(())
}

#[derive(Error, Debug)]
pub enum RegistrationError {
    #[error("Database error")]
    DatabaseError(#[from] sqlx::Error),
    #[error("Failed to update identifiers")]
    UpdateIdentifiersError(#[from] anyhow::Error),
    #[error("Token is not null")]
    NotNullTokenError,
    #[error("Device is not approved to get a token")]
    NotApprovedDevice,
    #[error("Failed to log in ledger")]
    FailedToLogInLedger,
}
