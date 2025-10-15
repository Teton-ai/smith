use crate::device::Variable;
use axum::http::StatusCode;
use smith::utils::schema::{SafeCommandRequest, SafeCommandTx};
use sqlx::PgPool;
use tracing::error;

// TODO: This file needs to be gone...

pub async fn refresh_device(pg_pool: &PgPool, device_id: i32) -> Result<StatusCode, StatusCode> {
    let mut tx = pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let bundle_id = sqlx::query!(r#"INSERT INTO command_bundles DEFAULT VALUES RETURNING uuid"#)
        .fetch_one(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to insert command bundle {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let variables = sqlx::query_as!(
        Variable,
        r#"SELECT
            id,
            device,
            name,
            value
        FROM variable
        WHERE device = $1
        ORDER BY device, name"#,
        device_id
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to get variables for device {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let commands: Vec<SafeCommandRequest> = vec![
        SafeCommandRequest {
            id: -1,
            command: SafeCommandTx::UpdateVariables {
                variables: variables
                    .into_iter()
                    .map(|variable| (variable.name, variable.value))
                    .collect(),
            },
            continue_on_error: false,
        },
        SafeCommandRequest {
            id: -2,
            command: SafeCommandTx::FreeForm {
                cmd: "systemctl restart capture-and-detect".to_string(),
            },
            continue_on_error: false,
        },
        SafeCommandRequest {
            id: -3,
            command: SafeCommandTx::FreeForm {
                cmd: "systemctl restart snakebrain".to_string(),
            },
            continue_on_error: false,
        },
    ];

    for command in commands {
        sqlx::query!(
            r#"INSERT INTO command_queue (device_id, cmd, continue_on_error, canceled, bundle)
            VALUES (
                $1,
                $2::jsonb,
                $3,
                false,
                $4
            )"#,
            device_id,
            serde_json::to_value(command.command)
                .expect("error: failed to serialize command into JSON"),
            command.continue_on_error,
            bundle_id.uuid
        )
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to insert command for device {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::NO_CONTENT)
}
