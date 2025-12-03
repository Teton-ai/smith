use crate::device::Variable;
use anyhow::Result;
use serde_json::Value;
use serde_json::json;
use smith::utils::schema;
use smith::utils::schema::SafeCommandTx::{UpdateNetwork, UpdateVariables};
use smith::utils::schema::{HomePost, NetworkType, SafeCommandRequest, SafeCommandRx};
use sqlx::PgPool;
use tracing::{debug, error};

// TODO: Get rid of this db.rs, legacy design and ugly

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
                SafeCommandRx::TestNetwork {
                    bytes_downloaded,
                    duration_ms,
                    bytes_uploaded,
                    upload_duration_ms,
                    timed_out,
                } => {
                    // Record results if we have valid data (even partial results from timeout)
                    if duration_ms > 0 {
                        let download_speed_mbps =
                            (bytes_downloaded as f64 * 8.0) / (duration_ms as f64 * 1000.0);

                        let upload_speed_mbps = match (bytes_uploaded, upload_duration_ms) {
                            (Some(bytes), Some(duration)) if duration > 0 => {
                                Some((bytes as f64 * 8.0) / (duration as f64 * 1000.0))
                            }
                            _ => None,
                        };

                        // If the test timed out, the network is too slow - cap score at 1
                        let network_score = if timed_out {
                            1
                        } else if download_speed_mbps >= 50.0 {
                            5
                        } else if download_speed_mbps >= 25.0 {
                            4
                        } else if download_speed_mbps >= 10.0 {
                            3
                        } else if download_speed_mbps >= 5.0 {
                            2
                        } else {
                            1
                        };

                        sqlx::query!(
                            "INSERT INTO device_network (device_id, network_score, download_speed_mbps, upload_speed_mbps, source, updated_at)
                            VALUES ($1, $2, $3, $4, $5, NOW())
                            ON CONFLICT (device_id)
                            DO UPDATE SET
                                network_score = EXCLUDED.network_score,
                                download_speed_mbps = EXCLUDED.download_speed_mbps,
                                upload_speed_mbps = EXCLUDED.upload_speed_mbps,
                                source = EXCLUDED.source,
                                updated_at = NOW()",
                            device.id,
                            network_score,
                            download_speed_mbps,
                            upload_speed_mbps,
                            "speed_test"
                        )
                        .execute(pool)
                        .await?;
                    }
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
        debug!("Adding commands to device {}", serial_number);
        debug!("Commands: {:?}", commands);
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
