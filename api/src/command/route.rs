use crate::State;
use crate::command::{
    BundleCommands, BundleWithCommandsPaginated, BundleWithRawResponsesExplicit, RecipeInput,
    TriggerRecipeInput,
};
use crate::middlewares::authorization;
use crate::user::CurrentUser;
use axum::Json;
use axum::extract::{Host, Path, Query};
use axum::{Extension, http::StatusCode, response::Result};
use models::command::{BundleReceipt, BundleWithCommands, CommandRecipe, QueuedCommand};
use models::device::DeviceCommandResponse;
use sentry::types::Uuid;
use serde::Deserialize;
use smith::utils::schema::{SafeCommandRequest, SafeCommandTx};
use sqlx::PgPool;
use std::collections::HashMap;
use tracing::error;

/// Queue `commands` against every device in `devices` as a single bundle.
/// Shared by raw bundle issuing and recipe triggering so both produce identical
/// `command_bundles` / `command_queue` rows and the same receipt shape.
async fn queue_commands_bundle(
    pg_pool: &PgPool,
    devices: &[i32],
    commands: &[SafeCommandRequest],
) -> Result<BundleReceipt, StatusCode> {
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

    let mut queued = Vec::with_capacity(devices.len() * commands.len());
    for device_id in devices {
        for command in commands {
            let cmd = serde_json::to_value(command.command.clone()).map_err(|err| {
                error!("Failed to serialize command into JSON {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            let row = sqlx::query!(
                r#"INSERT INTO command_queue (device_id, cmd, continue_on_error, canceled, bundle)
                VALUES (
                    $1,
                    $2::jsonb,
                    $3,
                    false,
                    $4
                )
                RETURNING id"#,
                device_id,
                cmd,
                command.continue_on_error,
                bundle_id.uuid
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(|err| {
                error!("Failed to insert command for device {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            queued.push(QueuedCommand {
                device: *device_id,
                cmd_id: row.id,
            });
        }
    }

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(BundleReceipt {
        uuid: bundle_id.uuid,
        commands: queued,
    })
}

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
        SafeCommandTx::RunAudit,
    ]))
}

#[utoipa::path(
    post,
    path = "/commands/bundles",
    request_body = BundleCommands,
    responses(
        (status = 201, description = "Commands issued successfully", body = BundleReceipt),
        (status = 400, description = "Empty devices or commands"),
        (status = 500, description = "Failed to issue commands", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = COMMANDS_TAG
)]
pub async fn issue_commands_to_devices(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Json(bundle_commands): Json<BundleCommands>,
) -> Result<(StatusCode, Json<BundleReceipt>), StatusCode> {
    // Never create a bundle with nothing queued: it would leave an orphan
    // `command_bundles` row that `get_bundle` cannot reconstruct.
    if bundle_commands.devices.is_empty() || bundle_commands.commands.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Gate each command kind by the caller's permissions (e.g. freeform,
    // tunnel). Recipes go through `trigger_recipe`, which is gated separately.
    if !authorization::authorize_commands(&current_user, &bundle_commands.commands) {
        return Err(StatusCode::FORBIDDEN);
    }

    let receipt = queue_commands_bundle(
        &state.pg_pool,
        &bundle_commands.devices,
        &bundle_commands.commands,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(receipt)))
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

    let limit = pagination.limit.unwrap_or(100).clamp(0, 100);

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
        let mut responses = map_responses
            .get(&(*uuid, *created_on))
            .expect("error: failed to get device command responses for (UUID, creation date)")
            .clone();
        // Keep commands in the order they were issued (queue id is serial), so
        // the displayed order matches how the bundle/recipe was defined.
        responses.sort_by_key(|response| response.cmd_id);
        bundles.push(BundleWithCommands {
            uuid: *uuid,
            created_on: *created_on,
            responses,
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

#[utoipa::path(
    get,
    path = "/commands/bundles/{uuid}",
    params(
        ("uuid" = String, Path),
    ),
    responses(
        (status = 200, description = "A single command bundle with its commands", body = BundleWithCommands),
        (status = 404, description = "Bundle not found"),
        (status = 500, description = "Failed to retrieve bundle"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = COMMANDS_TAG
)]
pub async fn get_bundle(
    Extension(state): Extension<State>,
    Path(uuid): Path<Uuid>,
) -> Result<Json<BundleWithCommands>, StatusCode> {
    let rows: Vec<BundleWithRawResponsesExplicit> = sqlx::query_as(
        r#"SELECT
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
        FROM command_bundles b
        LEFT JOIN command_queue cq ON b.uuid = cq.bundle
        LEFT JOIN command_response cr ON cq.id = cr.command_id
        LEFT JOIN device d ON cq.device_id = d.id
        WHERE b.uuid = $1
        ORDER BY cq.id;"#,
    )
    .bind(uuid)
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get bundle {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let first = rows.first().ok_or(StatusCode::NOT_FOUND)?;
    let created_on = first.created_on;

    let responses = rows
        .into_iter()
        .map(|raw| DeviceCommandResponse {
            device: raw.device,
            serial_number: raw.serial_number,
            cmd_id: raw.cmd_id,
            issued_at: raw.issued_at,
            cmd_data: raw.cmd_data,
            cancelled: raw.cancelled,
            fetched: raw.fetched,
            fetched_at: raw.fetched_at,
            response_id: raw.response_id,
            response_at: raw.response_at,
            response: raw.response,
            status: raw.status,
        })
        .collect();

    Ok(Json(BundleWithCommands {
        uuid,
        created_on,
        responses,
    }))
}

#[utoipa::path(
    get,
    path = "/commands/recipes",
    responses(
        (status = 200, description = "List of command recipes", body = Vec<CommandRecipe>),
        (status = 500, description = "Failed to retrieve recipes"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = COMMANDS_TAG
)]
pub async fn get_recipes(
    Extension(state): Extension<State>,
) -> Result<Json<Vec<CommandRecipe>>, StatusCode> {
    let recipes = sqlx::query_as::<_, CommandRecipe>(
        r#"SELECT id, name, description, commands, created_at, updated_at
        FROM command_recipes
        ORDER BY name"#,
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get recipes {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(recipes))
}

#[utoipa::path(
    post,
    path = "/commands/recipes",
    request_body = RecipeInput,
    responses(
        (status = 201, description = "Recipe created successfully"),
        (status = 409, description = "A recipe with that name already exists"),
        (status = 500, description = "Failed to create recipe"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = COMMANDS_TAG
)]
pub async fn create_recipe(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Json(recipe): Json<RecipeInput>,
) -> Result<StatusCode, StatusCode> {
    // Recipe contents are trusted at trigger time, so authoring them is a
    // privileged action even though triggering them is not.
    if !authorization::check(current_user, "recipes", "write") {
        return Err(StatusCode::FORBIDDEN);
    }

    let commands = serde_json::to_value(&recipe.commands).map_err(|err| {
        error!("Failed to serialize recipe commands into JSON {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    sqlx::query(
        r#"INSERT INTO command_recipes (name, description, commands)
        VALUES ($1, $2, $3::jsonb)"#,
    )
    .bind(&recipe.name)
    .bind(&recipe.description)
    .bind(&commands)
    .execute(&state.pg_pool)
    .await
    .map_err(|err| {
        if let sqlx::Error::Database(db_err) = &err
            && db_err.is_unique_violation()
        {
            return StatusCode::CONFLICT;
        }
        error!("Failed to create recipe {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::CREATED)
}

#[utoipa::path(
    put,
    path = "/commands/recipes/{recipe_id}",
    params(
        ("recipe_id" = i32, Path),
    ),
    request_body = RecipeInput,
    responses(
        (status = 204, description = "Recipe updated successfully"),
        (status = 404, description = "Recipe not found"),
        (status = 409, description = "A recipe with that name already exists"),
        (status = 500, description = "Failed to update recipe"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = COMMANDS_TAG
)]
pub async fn update_recipe(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Path(recipe_id): Path<i32>,
    Json(recipe): Json<RecipeInput>,
) -> Result<StatusCode, StatusCode> {
    if !authorization::check(current_user, "recipes", "write") {
        return Err(StatusCode::FORBIDDEN);
    }

    let commands = serde_json::to_value(&recipe.commands).map_err(|err| {
        error!("Failed to serialize recipe commands into JSON {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let result = sqlx::query(
        r#"UPDATE command_recipes
        SET name = $1, description = $2, commands = $3::jsonb, updated_at = now()
        WHERE id = $4"#,
    )
    .bind(&recipe.name)
    .bind(&recipe.description)
    .bind(&commands)
    .bind(recipe_id)
    .execute(&state.pg_pool)
    .await
    .map_err(|err| {
        if let sqlx::Error::Database(db_err) = &err
            && db_err.is_unique_violation()
        {
            return StatusCode::CONFLICT;
        }
        error!("Failed to update recipe {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if result.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    delete,
    path = "/commands/recipes/{recipe_id}",
    params(
        ("recipe_id" = i32, Path),
    ),
    responses(
        (status = 204, description = "Recipe deleted successfully"),
        (status = 404, description = "Recipe not found"),
        (status = 500, description = "Failed to delete recipe"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = COMMANDS_TAG
)]
pub async fn delete_recipe(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Path(recipe_id): Path<i32>,
) -> Result<StatusCode, StatusCode> {
    if !authorization::check(current_user, "recipes", "write") {
        return Err(StatusCode::FORBIDDEN);
    }

    let result = sqlx::query(r#"DELETE FROM command_recipes WHERE id = $1"#)
        .bind(recipe_id)
        .execute(&state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to delete recipe {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if result.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/commands/recipes/{recipe_id}/trigger",
    params(
        ("recipe_id" = i32, Path),
    ),
    request_body = TriggerRecipeInput,
    responses(
        (status = 201, description = "Recipe triggered successfully", body = BundleReceipt),
        (status = 400, description = "No devices supplied"),
        (status = 403, description = "Not allowed to trigger recipes"),
        (status = 404, description = "Recipe not found"),
        (status = 500, description = "Failed to trigger recipe"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = COMMANDS_TAG
)]
pub async fn trigger_recipe(
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Path(recipe_id): Path<i32>,
    Json(input): Json<TriggerRecipeInput>,
) -> Result<(StatusCode, Json<BundleReceipt>), StatusCode> {
    // Triggering only needs `recipes:trigger`; the recipe's commands are NOT
    // re-checked against the caller's command permissions. The recipe is a
    // vetted artifact (authoring needs `recipes:write`), so a user who can only
    // trigger recipes can run one that contains freeform/tunnel steps without
    // being able to issue those commands directly.
    if !authorization::check(current_user, "recipes", "trigger") {
        return Err(StatusCode::FORBIDDEN);
    }

    if input.devices.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let recipe = sqlx::query!(
        r#"SELECT commands FROM command_recipes WHERE id = $1"#,
        recipe_id
    )
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to load recipe {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    let commands: Vec<SafeCommandRequest> =
        serde_json::from_value(recipe.commands).map_err(|err| {
            error!("Failed to deserialize recipe commands {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let receipt = queue_commands_bundle(&state.pg_pool, &input.devices, &commands).await?;

    Ok((StatusCode::CREATED, Json(receipt)))
}
