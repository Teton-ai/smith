use crate::config::Config;
use crate::db::DeviceWithToken;
pub(crate) use crate::device::schema::Device;
use serde_json::{Value, json};
use smith::utils::schema::{DeviceRegistration, DeviceRegistrationResponse};
use sqlx::PgPool;
use sqlx::types::ipnetwork;
use thiserror::Error;
use tracing::error;

pub mod routes;
pub mod schema;

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

    pub async fn save_last_ping_with_ip(
        device: &DeviceWithToken,
        ip_address: Option<std::net::IpAddr>,
        pool: &PgPool,
    ) -> anyhow::Result<()> {
        let mut tx = pool.begin().await?;
        match ip_address {
            Some(ip) => {
                let ip_network: ipnetwork::IpNetwork = ip.into();
                
                // Insert or get existing IP address record
                let ip_record = sqlx::query!(
                    "INSERT INTO ip_address (ip_address, created_at) VALUES ($1, NOW()) 
                     ON CONFLICT (ip_address) DO UPDATE SET updated_at = NOW()
                     RETURNING id",
                    ip_network
                )
                .fetch_one(&mut *tx)
                .await?;

                // Update device with IP address ID
                sqlx::query!(
                    "UPDATE device SET last_ping = NOW(), ip_address_id = $2 WHERE id = $1",
                    device.id,
                    ip_record.id
                )
                .execute(&mut *tx)
                .await?;
            }
            None => {
                sqlx::query!(
                    "UPDATE device SET last_ping = NOW() WHERE id = $1",
                    device.id
                )
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
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
