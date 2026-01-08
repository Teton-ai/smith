use crate::State;
use crate::command::{
    BundleCommands, BundleWithCommands, BundleWithCommandsPaginated,
    BundleWithRawResponsesExplicit, DeviceCommandResponse,
};
use axum::Json;
use axum::extract::{Host, Query};
use axum::{Extension, http::StatusCode, response::Result};
use sentry::types::Uuid;
use serde::Deserialize;
use smith::utils::schema::SafeCommandTx;
use std::collections::HashMap;
use tracing::error;

const COMMANDS_TAG: &str = "commands";

#[utoipa::path(
    get,
    path = "/commands",
    responses(
        (status = 200, description = "List of available commands"),
    ),
    tag = COMMANDS_TAG
)]
pub async fn available_commands() -> Result<Json<Vec<SafeCommandTx>>, StatusCode> {
    Ok(Json(vec![
        SafeCommandTx::Ping,
        SafeCommandTx::Upgrade,
        SafeCommandTx::Restart,
        SafeCommandTx::FreeForm {
            cmd: "echo 'Hello, World!'".to_string(),
        },
        SafeCommandTx::OpenTunnel {
            port: None,
            pub_key: None,
            user: None,
        },
        SafeCommandTx::CloseTunnel,
        SafeCommandTx::DownloadOTA {
            tools: "ota_tools.tbz2".to_string(),
            payload: "ota_payload_package.tar.gz".to_string(),
            rate: 1.0,
        },
        SafeCommandTx::CheckOTAStatus,
        SafeCommandTx::StartOTA,
        SafeCommandTx::TestNetwork,
    ]))
}

#[utoipa::path(
    post,
    path = "/commands/bundles",
    request_body = BundleCommands,
    responses(
        (status = 201, description = "Commands issued successfully"),
        (status = 500, description = "Failed to issue commands", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = COMMANDS_TAG
)]
pub async fn issue_commands_to_devices(
    Extension(state): Extension<State>,
    Json(bundle_commands): Json<BundleCommands>,
) -> Result<StatusCode, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(|err| {
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

    for device_id in &bundle_commands.devices {
        for command in &bundle_commands.commands {
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
                serde_json::to_value(command.command.clone())
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
    }

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::CREATED)
}

#[derive(Deserialize, Debug)]
pub struct PaginationUuid {
    pub starting_after: Option<Uuid>,
    pub ending_before: Option<Uuid>,
    pub limit: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/commands/bundles",
    responses(
        (status = 200, description = "List of command bundles", body = BundleWithCommandsPaginated),
        (status = 400, description = "Invalid pagination parameters"),
        (status = 500, description = "Failed to retrieve command bundles", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = COMMANDS_TAG
)]
pub async fn get_bundle_commands(
    host: Host,
    Extension(state): Extension<State>,
    pagination: Query<PaginationUuid>,
) -> Result<Json<BundleWithCommandsPaginated>, StatusCode> {
    if pagination.starting_after.is_some() && pagination.ending_before.is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let limit = pagination.limit.unwrap_or(10).clamp(0, 10);

    let where_clause = if let Some(starting_after) = pagination.starting_after {
        format!(
            "WHERE created_on <= (SELECT created_on FROM command_bundles WHERE uuid = '{}') ORDER BY created_on DESC",
            starting_after
        )
    } else if let Some(ending_before) = pagination.ending_before {
        format!(
            "WHERE created_on > (SELECT created_on FROM command_bundles WHERE uuid = '{}') ORDER BY created_on ASC",
            ending_before
        )
    } else {
        "ORDER BY created_on DESC".to_string()
    };

    let raw_bundles: Vec<BundleWithRawResponsesExplicit> = sqlx::query_as(&format!(
        r#"WITH latest_bundles AS (
            SELECT *
            FROM command_bundles
            {where_clause}
            LIMIT $1
        )
        SELECT
            b.uuid,
            b.created_on,
            cq.device_id as device,
            d.serial_number as serial_number,
            cq.id as cmd_id,
            cq.created_at as issued_at,
            cq.cmd as cmd_data,
            cq.canceled as cancelled,
            cq.fetched as fetched,
            cq.fetched_at as fetched_at,
            cr.id as response_id,
            cr.created_at as response_at,
            cr.response as response,
            cr.status as status
        FROM latest_bundles b
        LEFT JOIN command_queue cq ON b.uuid = cq.bundle
        LEFT JOIN command_response cr ON cq.id = cr.command_id
        LEFT JOIN device d ON cq.device_id = d.id
        ORDER BY b.created_on DESC;"#,
    ))
    .bind(limit) // Bind the limit parameter
    .fetch_all(&mut *tx)
    .await
    .unwrap_or_default();

    let mut map_responses = HashMap::new();

    raw_bundles.into_iter().for_each(|raw_bundle| {
        // check if we have already seen this bundle
        let response = DeviceCommandResponse {
            device: raw_bundle.device,
            serial_number: raw_bundle.serial_number,
            cmd_id: raw_bundle.cmd_id,
            issued_at: raw_bundle.issued_at,
            cmd_data: raw_bundle.cmd_data,
            cancelled: raw_bundle.cancelled,
            fetched: raw_bundle.fetched,
            fetched_at: raw_bundle.fetched_at,
            response_id: raw_bundle.response_id,
            response_at: raw_bundle.response_at,
            response: raw_bundle.response,
            status: raw_bundle.status,
        };

        map_responses
            .entry((raw_bundle.uuid, raw_bundle.created_on))
            .and_modify(|responses: &mut Vec<DeviceCommandResponse>| {
                responses.push(response.clone());
            })
            .or_insert(vec![response]);
    });

    let mut bundles: Vec<BundleWithCommands> = Vec::new();

    for (uuid, created_on) in map_responses.keys() {
        bundles.push(BundleWithCommands {
            uuid: *uuid,
            created_on: *created_on,
            responses: map_responses
                .get(&(*uuid, *created_on))
                .expect("error: failed to get device command responses for (UUID, creation date)")
                .clone(),
        });
    }

    // Sort by timestamp (most recent first).
    bundles.sort_by(|a, b| b.created_on.cmp(&a.created_on));

    let first_id = bundles.first().map(|c| c.uuid);
    let last_id = bundles.last().map(|c| c.uuid);

    let has_more_first_id = if let Some(first_id) = first_id {
        let more = sqlx::query_scalar!(
            r#"select exists(
                select 1 from command_bundles
                where created_on > (
                    select created_on from command_bundles where uuid = $1
                )
                order by created_on asc
                limit 1
            )"#,
            first_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to check if there is more command bundles {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        more.unwrap_or(false)
    } else {
        false
    };

    let has_more_last_id = if let Some(last_id) = last_id {
        let more = sqlx::query_scalar!(
            r#"select exists(
                select 1 from command_bundles
                where created_on < (
                    select created_on from command_bundles where uuid = $1
                )
                order by created_on desc
                limit 1
            )"#,
            last_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to check if there is more command bundles {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        more.unwrap_or(false)
    } else {
        false
    };

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let next = if has_more_last_id {
        Some(format!(
            "https://{}/commands/bundles?starting_after={}&limit={}",
            host.0,
            last_id.expect("error: failed to get last id"),
            limit
        ))
    } else {
        None
    };

    let previous = if has_more_first_id {
        Some(format!(
            "https://{}/commands/bundles?ending_before={}&limit={}",
            host.0,
            first_id.expect("error: failed to get first id"),
            limit
        ))
    } else {
        None
    };

    let bundles_paginated = BundleWithCommandsPaginated {
        bundles,
        next,
        previous,
    };

    Ok(Json(bundles_paginated))
}
