use crate::config::Config;
use crate::db::DeviceWithToken;
use crate::ip_address::IpAddressInfo;
use crate::modem::Modem;
use crate::release::Release;
use axum::Extension;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use smith::utils::schema::{DeviceRegistration, DeviceRegistrationResponse};
use sqlx::types::chrono::{DateTime, Utc};
use sqlx::types::{chrono, ipnetwork};
use sqlx::{PgPool, Pool, Postgres};
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, error, warn};

mod helpers;
pub mod route;

#[derive(Clone)]
pub struct AuthDevice(pub RawDevice);

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DeviceLabels(pub HashMap<String, String>);

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeviceNetwork {
    pub network_score: Option<i32>,
    pub source: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
}

// TODO: Change this, this needs to be device and the other is PublicDevice, API type
#[derive(Debug, Serialize, utoipa::ToSchema, Clone)]
pub struct RawDevice {
    pub id: i32,
    pub serial_number: String,
    pub labels: Value,
    pub last_ping: Option<DateTime<Utc>>,
    pub wifi_mac: Option<String>,
    pub modified_on: DateTime<Utc>,
    pub created_on: DateTime<Utc>,
    pub note: Option<String>,
    pub approved: bool,
    pub token: Option<String>,
    pub release_id: Option<i32>,
    pub target_release_id: Option<i32>,
    pub system_info: Option<Value>,
    pub network_id: Option<i32>,
    pub modem_id: Option<i32>,
    pub archived: bool,
    pub ip_address_id: Option<i32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Device {
    pub id: i32,
    pub serial_number: String,
    pub note: Option<String>,
    pub last_seen: Option<DateTime<Utc>>,
    pub created_on: DateTime<Utc>,
    pub approved: bool,
    pub has_token: Option<bool>,
    pub release_id: Option<i32>,
    pub target_release_id: Option<i32>,
    pub system_info: Option<Value>,
    pub modem_id: Option<i32>,
    pub ip_address_id: Option<i32>,
    pub ip_address: Option<IpAddressInfo>,
    pub modem: Option<Modem>,
    pub release: Option<Release>,
    pub target_release: Option<Release>,
    pub network: Option<DeviceNetwork>,
    pub labels: DeviceLabels,
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

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub struct LeanResponse {
    pub limit: i64,
    pub reverse: bool,
    pub devices: Vec<LeanDevice>,
}

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub struct LeanDevice {
    pub id: i32,
    pub serial_number: String,
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
    pub approved: bool,
    pub up_to_date: Option<bool>,
    pub ip_address_id: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct UpdateDeviceRelease {
    pub target_release_id: i32,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct UpdateDevicesRelease {
    pub target_release_id: i32,
    pub devices: Vec<i32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Tag {
    pub id: i32,
    pub device: i32,
    pub name: String,
    pub color: Option<String>,
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

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DeviceCommandResponse {
    pub device: i32,
    pub serial_number: String,
    pub cmd_id: i32,
    pub issued_at: DateTime<Utc>,
    pub cmd_data: Value,
    pub cancelled: bool,
    pub fetched: bool,
    pub fetched_at: Option<DateTime<Utc>>,
    pub response_id: Option<i32>,
    pub response_at: Option<DateTime<Utc>>,
    pub response: Option<Value>,
    pub status: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CommandsPaginated {
    pub commands: Vec<DeviceCommandResponse>,
    pub next: Option<String>,
    pub previous: Option<String>,
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

async fn update_ip_geolocation(
    ip_address: IpAddr,
    ip_id: i32,
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

const BEARER: &str = "Bearer ";

impl Device {
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
                    "text": format!("Device {} registered via API", result.serial_number),
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
                let client = reqwest::Client::new();
                let _res = client
                    .post(slack_hook_url)
                    .header("Content-Type", "application/json")
                    .json(&message)
                    .send()
                    .await;
            }
        }

        if result.approved == Some(true) {
            match result.token {
                Some(_) => {
                    tx.rollback().await?;
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

                    #[derive(sqlx::FromRow)]
                    struct VariablesPresetRow {
                        variables: Value,
                    }

                    let result_vars = sqlx::query_as!(
                        VariablesPresetRow,
                        "SELECT variables FROM variable_preset WHERE title = 'DEFAULT'"
                    )
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|err| {
                        error!("Failed to fetch variables preset {err}");
                        RegistrationError::DatabaseError(err)
                    })?;

                    for (name, value) in result_vars
                        .variables
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

    /// Retrieves a device by its authentication token.
    ///
    /// # Arguments
    /// * `token` - The authentication token assigned to the device
    /// * `pg_pool` - PostgreSQL connection pool
    ///
    /// # Returns
    /// * `Ok(Some(RawDevice))` if a device with the token exists
    /// * `Ok(None)` if no device matches the token
    /// * `Err` if the database query fails
    pub async fn get_device_from_token(
        token: String,
        pg_pool: &Pool<Postgres>,
    ) -> anyhow::Result<Option<RawDevice>> {
        Ok(
            sqlx::query_as!(RawDevice, "SELECT * FROM device WHERE token = $1;", token)
                .fetch_optional(pg_pool)
                .await?,
        )
    }

    /// Axum middleware that authenticates devices using Bearer tokens.
    ///
    /// Extracts the Bearer token from the Authorization header, validates it against
    /// the database, and injects the authenticated device into request extensions as `AuthDevice`.
    ///
    /// # Arguments
    /// * `state` - Application state containing the PostgreSQL connection pool
    /// * `request` - Incoming HTTP request
    /// * `next` - Next middleware/handler in the chain
    ///
    /// # Returns
    /// * `Ok(Response)` if authentication succeeds, with `AuthDevice` in request extensions
    /// * `Err(StatusCode::UNAUTHORIZED)` if the token is missing, invalid, or not found
    pub async fn middleware(
        Extension(state): Extension<crate::State>,
        mut request: Request,
        next: Next,
    ) -> Result<Response, StatusCode> {
        let headers = request.headers();

        let authorization_header = headers
            .get(header::AUTHORIZATION)
            .ok_or(StatusCode::UNAUTHORIZED)?;
        let authorization = authorization_header
            .to_str()
            .map_err(|_| StatusCode::UNAUTHORIZED)?;
        if !authorization.starts_with(BEARER) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let bearer_token = authorization.trim_start_matches(BEARER);
        let device = Self::get_device_from_token(bearer_token.to_string(), &state.pg_pool)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?
            .ok_or(StatusCode::UNAUTHORIZED)?;

        request.extensions_mut().insert(AuthDevice(device));

        Ok(next.run(request).await)
    }

    pub async fn save_last_ping_with_ip(
        device: &DeviceWithToken,
        ip_address: Option<IpAddr>,
        pool: &PgPool,
        config: &Config,
    ) -> anyhow::Result<()> {
        let mut tx = pool.begin().await?;
        match ip_address {
            Some(ip) => {
                let ip_network: ipnetwork::IpNetwork = ip.into();

                // Insert IP address if it doesn't exist, or get existing ID
                let insert_result = sqlx::query!(
                    "INSERT INTO ip_address (ip_address, created_at) VALUES ($1, NOW()) ON CONFLICT (ip_address) DO NOTHING RETURNING id",
                    ip_network
                )
                .fetch_optional(&mut *tx)
                .await?;

                let (ip_id, should_update_geolocation) = match insert_result {
                    Some(record) => {
                        // New IP was inserted, mark for geolocation update
                        (record.id, true)
                    }
                    None => {
                        // IP already exists, get ID and check if geolocation needs updating
                        let existing_record = sqlx::query!(
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
                        .fetch_one(&mut *tx)
                        .await?;
                        (
                            existing_record.id,
                            existing_record.needs_update.unwrap_or(false),
                        )
                    }
                };

                // Update device with IP address ID
                sqlx::query!(
                    "UPDATE device SET last_ping = NOW(), ip_address_id = $2 WHERE id = $1",
                    device.id,
                    ip_id
                )
                .execute(&mut *tx)
                .await?;

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
                sqlx::query!(
                    "UPDATE device SET last_ping = NOW() WHERE id = $1",
                    device.id
                )
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
            }
        }
        Ok(())
    }

    pub async fn get_target_release(device: &DeviceWithToken, pool: &PgPool) -> Option<i32> {
        if let Ok(device) = sqlx::query!(
            "SELECT target_release_id FROM device WHERE id = $1",
            &device.id
        )
        .fetch_one(pool)
        .await
        {
            return device.target_release_id;
        }
        None
    }

    pub async fn save_release_id(
        device: &DeviceWithToken,
        release_id: Option<i32>,
        pool: &PgPool,
    ) -> anyhow::Result<()> {
        if let Some(new_release_id) = release_id {
            let mut tx = pool.begin().await?;

            let current = sqlx::query!("SELECT release_id FROM device WHERE id = $1", device.id)
                .fetch_one(&mut *tx)
                .await?;

            if current.release_id != Some(new_release_id) {
                sqlx::query!(
                    "UPDATE device SET release_id = $1 WHERE id = $2",
                    new_release_id,
                    device.id,
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
                        device.id,
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
