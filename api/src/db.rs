use crate::handlers::devices::types::Variable;
use anyhow::Result;
use serde_json::Value;
use serde_json::json;
use smith::utils::schema;
use smith::utils::schema::SafeCommandTx::{UpdateNetwork, UpdateVariables};
use smith::utils::schema::{HomePost, NetworkType, SafeCommandRequest, SafeCommandRx};
use sqlx::PgPool;
use thiserror::Error;
use tracing::{error, info};

#[derive(Debug)]
pub struct DeviceWithToken {
    pub id: i32,
    pub serial_number: String,
}

pub struct CommandsDB {
    id: i32,
    cmd: Value,
    continue_on_error: bool,
}

pub struct DBHandler;

impl DBHandler {
    pub async fn validate_token(
        token: &str,
        pool: &PgPool,
    ) -> Result<DeviceWithToken, AuthorizationError> {
        let device = sqlx::query_as!(
            DeviceWithToken,
            "SELECT id, serial_number FROM device WHERE token is not null AND token = $1",
            token
        )
        .fetch_optional(pool)
        .await
        .map_err(|err| {
            error!("Failed to fetch device information {err}");
            AuthorizationError::DatabaseError(err)
        })?;

        if let Some(device) = device {
            return Ok(device);
        }

        Err(AuthorizationError::UnauthorizedDevice)
    }

    pub async fn save_responses(
        device: &DeviceWithToken,
        payload: HomePost,
        pool: &PgPool,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;

        for response in payload.responses {
            match response.command {
                SafeCommandRx::GetVariables => {
                    let variables = sqlx::query_as!(
                        Variable,
                        "
                        SELECT id, device, name, value
                        FROM variable
                        WHERE device = $1
                        ORDER BY device, name
                        ",
                        device.id
                    )
                    .fetch_all(&mut *tx)
                    .await?;
                    let update_variables = UpdateVariables {
                        variables: variables
                            .into_iter()
                            .map(|variable| (variable.name, variable.value))
                            .collect(),
                    };
                    DBHandler::add_commands(
                        &device.serial_number,
                        vec![SafeCommandRequest {
                            id: -1,
                            command: update_variables,
                            continue_on_error: false,
                        }],
                        pool,
                    )
                    .await?;
                }
                SafeCommandRx::GetNetwork => {
                    let network = sqlx::query_as!(
                        schema::Network,
                        r#"
                        SELECT
                            n.id,
                            n.network_type::TEXT,
                            n.is_network_hidden,
                            n.ssid,
                            n.name,
                            n.description,
                            n.password
                        FROM network n
                        JOIN device d ON n.id = d.network_id
                        WHERE d.id = $1"#,
                        &device.id
                    )
                    .fetch_optional(&mut *tx)
                    .await?;

                    if let Some(network) = network {
                        if network.network_type == NetworkType::Wifi {
                            DBHandler::add_commands(
                                &device.serial_number,
                                vec![SafeCommandRequest {
                                    id: -4,
                                    command: UpdateNetwork { network },
                                    continue_on_error: false,
                                }],
                                pool,
                            )
                            .await?;
                        }
                    }
                }
                SafeCommandRx::UpdateSystemInfo { ref system_info } => {
                    sqlx::query!(
                        "UPDATE device SET system_info = $2 WHERE id = $1",
                        device.id,
                        system_info
                    )
                    .execute(pool)
                    .await?;
                }
                _ => {}
            }
            let _response_id = sqlx::query_scalar!(
                "INSERT INTO command_response (device_id, command_id, response, status)
                VALUES (
                    $1,
                    CASE WHEN $2 < 0 THEN NULL ELSE $2 END,
                    $3::jsonb,
                    $4
                )
                RETURNING id",
                device.id,
                response.id,
                json!(response.command),
                response.status
            )
            .fetch_one(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn get_commands(device: &DeviceWithToken, pool: &PgPool) -> Vec<SafeCommandRequest> {
        if let Ok(mut tx) = pool.begin().await {
            let fetched_commands: Vec<CommandsDB> = sqlx::query_as!(
                CommandsDB,
                "SELECT id, cmd, continue_on_error
                 FROM command_queue
                 WHERE device_id = $1 AND fetched = false AND canceled = false",
                device.id
            )
            .fetch_all(&mut *tx)
            .await
            .unwrap_or_else(|err| {
                error!("Failed to get commands for device {err}");
                Vec::new()
            });

            // If commands are fetched successfully, update fetched_at timestamp
            if !fetched_commands.is_empty() {
                let ids: Vec<i32> = fetched_commands.iter().map(|cmd| cmd.id).collect();
                let _update_query = sqlx::query!(
                    "UPDATE command_queue SET fetched_at = CURRENT_TIMESTAMP, fetched = true WHERE id = ANY($1)",
                    &ids
                )
                .execute(&mut *tx)
                .await;
            }

            tx.commit().await.unwrap_or_else(|err| {
                error!("Failed to commit transaction: {err}");
            });

            fetched_commands
                .into_iter()
                .filter_map(|cmd| match serde_json::from_value(cmd.cmd) {
                    Ok(command) => Some(SafeCommandRequest {
                        id: cmd.id,
                        command,
                        continue_on_error: cmd.continue_on_error,
                    }),
                    Err(err) => {
                        error!(
                            serial_number = device.serial_number,
                            "Failed to deserialize command from database: {err}"
                        );
                        None
                    }
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    pub async fn add_commands(
        serial_number: &str,
        commands: Vec<SafeCommandRequest>,
        pool: &PgPool,
    ) -> Result<Vec<i32>> {
        info!("Adding commands to device {}", serial_number);
        info!("Commands: {:?}", commands);
        let mut command_ids = Vec::new();

        let mut tx = pool.begin().await?;

        let bundle_id =
            sqlx::query!(r#"INSERT INTO command_bundles DEFAULT VALUES RETURNING uuid"#)
                .fetch_one(&mut *tx)
                .await?;

        for command in commands {
            let command_id: i32 = sqlx::query_scalar!(
                "INSERT INTO command_queue (device_id, cmd, continue_on_error, canceled, bundle)
                VALUES (
                    (SELECT id FROM device WHERE serial_number = $1),
                    $2::jsonb,
                    $3,
                    false,
                    $4
                )
                RETURNING id;",
                serial_number,
                json!(command.command),
                command.continue_on_error,
                bundle_id.uuid
            )
            .fetch_one(&mut *tx)
            .await?;

            command_ids.push(command_id);
        }

        tx.commit().await?;
        Ok(command_ids)
    }
}

#[derive(Error, Debug)]
pub enum AuthorizationError {
    #[error("Database error")]
    DatabaseError(#[from] sqlx::Error),
    #[error("Device is not authorized to access the API")]
    UnauthorizedDevice,
}
