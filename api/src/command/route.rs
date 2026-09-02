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
use chrono::Utc;
use models::command::{BundleReceipt, BundleWithCommands, CommandRecipe, QueuedCommand};
use models::device::DeviceCommandResponse;
use rand::seq::SliceRandom;
use sentry::types::Uuid;
use serde::Deserialize;
use smith::utils::schema::{SafeCommandRequest, SafeCommandTx};
use sqlx::PgPool;
use std::collections::HashMap;
use std::time::Duration;
use tracing::error;

use crate::command::redact_cmd_data;

/// `wave_size` devices become eligible together, then the next `wave_size`
/// after `wave_duration`, spreading out contention for a shared resource.
struct StaggerPolicy {
    wave_size: u32,
    wave_duration: Duration,
}

const WIFI_SCAN_WAVE_DURATION: Duration = Duration::from_secs(10);
const TEST_NETWORK_WAVE_FRACTION: f64 = 0.10;
const TEST_NETWORK_WAVE_DURATION: Duration = Duration::from_secs(60);

fn bundle_relative_wave_size(device_count: usize, fraction: f64) -> u32 {
    ((device_count as f64 * fraction).round() as u32).max(1)
}

/// `None` for anything that doesn't contend for a shared physical resource.
/// `device_count` sizes policies whose `wave_size` scales with the bundle.
fn stagger_policy(cmd: &SafeCommandTx, device_count: usize) -> Option<StaggerPolicy> {
    match cmd {
        SafeCommandTx::WifiScan => Some(StaggerPolicy {
            wave_size: 2,
            wave_duration: WIFI_SCAN_WAVE_DURATION,
        }),
        // Download+upload each cap at 30s (NETWORK_TEST_TIMEOUT in smithd),
        // so 60s bounds a run. Paces the api server's bandwidth, not an AP.
        SafeCommandTx::TestNetwork => Some(StaggerPolicy {
            wave_size: bundle_relative_wave_size(device_count, TEST_NETWORK_WAVE_FRACTION),
            wave_duration: TEST_NETWORK_WAVE_DURATION,
        }),
        _ => None,
    }
}

/// Strictest policy across `commands`: smallest wave, longest duration.
fn merged_stagger_policy(
    commands: &[SafeCommandRequest],
    device_count: usize,
) -> Option<StaggerPolicy> {
    commands
        .iter()
        .filter_map(|c| stagger_policy(&c.command, device_count))
        .reduce(|a, b| StaggerPolicy {
            wave_size: a.wave_size.min(b.wave_size),
            wave_duration: a.wave_duration.max(b.wave_duration),
        })
}

/// `offset` is relative, not absolute: see the two-phase insert comment below.
struct DeviceWaveOffset {
    device_id: i32,
    offset: Option<Duration>,
}

/// Shuffled once so wave membership doesn't correlate with caller order.
fn assign_wave_offsets(devices: &[i32], policy: Option<&StaggerPolicy>) -> Vec<DeviceWaveOffset> {
    let Some(policy) = policy else {
        return devices
            .iter()
            .map(|&device_id| DeviceWaveOffset {
                device_id,
                offset: None,
            })
            .collect();
    };

    // Avoid a div-by-zero if a future policy sets wave_size to 0.
    let wave_size = policy.wave_size.max(1);

    let mut ordered_devices: Vec<i32> = devices.to_vec();
    ordered_devices.shuffle(&mut rand::rng());

    ordered_devices
        .into_iter()
        .enumerate()
        .map(|(wave_index, device_id)| DeviceWaveOffset {
            device_id,
            offset: Some(policy.wave_duration * (wave_index as u32 / wave_size)),
        })
        .collect()
}

/// Queue `commands` against every device in `devices` as a single bundle.
/// Shared by raw bundle issuing and recipe triggering so both produce identical
/// `command_bundles` / `command_queue` rows and the same receipt shape.
async fn queue_commands_bundle(
    pg_pool: &PgPool,
    devices: &[i32],
    commands: &[SafeCommandRequest],
    user_id: i32,
) -> Result<BundleReceipt, StatusCode> {
    let mut tx = pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let bundle_id = sqlx::query!(
        r#"INSERT INTO command_bundles (user_id) VALUES ($1) RETURNING uuid"#,
        user_id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to insert command bundle {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let policy = merged_stagger_policy(commands, devices.len());
    let wave_offsets = assign_wave_offsets(devices, policy.as_ref());

    // Two-phase: a slow insert could let real time pass an early wave's
    // schedule before anything commits and becomes visible, collapsing
    // waves. So paced rows insert with a far-future placeholder, then get
    // corrected in one UPDATE just before commit using `statement_timestamp()`
    // (unlike `now()`, not frozen at transaction start, and unlike
    // `clock_timestamp()`, shared by every row in this one UPDATE instead of
    // drifting per row).
    let far_future_placeholder = Utc::now() + Duration::from_secs(24 * 60 * 60);

    let mut queued = Vec::with_capacity(devices.len() * commands.len());
    let mut pending_corrections: Vec<(i32, i32)> = Vec::new();
    for DeviceWaveOffset { device_id, offset } in &wave_offsets {
        let placeholder = offset.map(|_| far_future_placeholder);
        for command in commands {
            let cmd = serde_json::to_value(command.command.clone()).map_err(|err| {
                error!("Failed to serialize command into JSON {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            let row = sqlx::query!(
                r#"INSERT INTO command_queue (device_id, cmd, continue_on_error, canceled, bundle, available_at)
                VALUES (
                    $1,
                    $2::jsonb,
                    $3,
                    false,
                    $4,
                    COALESCE($5, now())
                )
                RETURNING id"#,
                device_id,
                cmd,
                command.continue_on_error,
                bundle_id.uuid,
                placeholder
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(|err| {
                error!("Failed to insert command for device {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            if let Some(offset) = offset {
                pending_corrections.push((row.id, offset.as_secs() as i32));
            }

            queued.push(QueuedCommand {
                device: *device_id,
                cmd_id: row.id,
            });
        }
    }

    if !pending_corrections.is_empty() {
        let ids: Vec<i32> = pending_corrections.iter().map(|(id, _)| *id).collect();
        let offset_secs: Vec<i32> = pending_corrections.iter().map(|(_, s)| *s).collect();
        sqlx::query!(
            r#"
            UPDATE command_queue AS cq
            SET available_at = statement_timestamp() + make_interval(secs => u.offset_secs)
            FROM UNNEST($1::int[], $2::int[]) AS u(id, offset_secs)
            WHERE cq.id = u.id
            "#,
            &ids,
            &offset_secs,
        )
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to finalize staggered availability {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
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

    if !authorization::reject_unknown_commands(&bundle_commands.commands) {
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
        current_user.user_id,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(receipt)))
}

#[derive(Deserialize, Debug)]
pub struct PaginationUuid {
    pub starting_after: Option<Uuid>,
    pub ending_before: Option<Uuid>,
    pub limit: Option<i32>,
    /// Who triggered the bundle: absent for no filter, `people` for anything a
    /// person triggered, `system` for api-generated bundles, or a user id.
    pub triggered_by: Option<String>,
}

/// Restricts bundles to one triggerer: `$2` user id, `$3` system-only, `$4`
/// People-only. Static SQL so the has-more queries can carry the same predicate
/// and stay compile-time checked macros; those need literals, so this text is
/// duplicated at both call sites and the parameter positions must match.
const TRIGGERER_FILTER: &str = "($2::int IS NULL OR user_id = $2)
            AND (NOT $3::bool OR user_id IS NULL)
            AND (NOT $4::bool OR user_id IS NOT NULL)";

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

    // A well-formed id for a user that no longer exists is valid input with zero
    // results; only unparseable values are rejected.
    let (filter_user_id, filter_system, filter_people) = match pagination.triggered_by.as_deref() {
        None => (None, false, false),
        Some("people") => (None, false, true),
        Some("system") => (None, true, false),
        Some(other) => match other.parse::<i32>() {
            Ok(id) => (Some(id), false, false),
            Err(_) => return Err(StatusCode::BAD_REQUEST),
        },
    };

    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!("Failed to start transaction {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let limit = pagination.limit.unwrap_or(100).clamp(0, 100);

    // Conditions and ordering are kept apart so filters can be appended.
    // The cursor is the (created_on, uuid) tuple because `created_on` is not
    // unique: ordering on it alone leaves the page boundary undefined and can
    // strand a tied bundle. An exact boundary can also be exclusive, so the
    // cursor bundle is not repeated on the next page.
    let mut conditions: Vec<String> = Vec::new();
    let order_by = if let Some(starting_after) = pagination.starting_after {
        conditions.push(format!(
            "(created_on, uuid) < (SELECT created_on, uuid FROM command_bundles WHERE uuid = '{starting_after}')"
        ));
        "ORDER BY created_on DESC, uuid DESC"
    } else if let Some(ending_before) = pagination.ending_before {
        conditions.push(format!(
            "(created_on, uuid) > (SELECT created_on, uuid FROM command_bundles WHERE uuid = '{ending_before}')"
        ));
        "ORDER BY created_on ASC, uuid ASC"
    } else {
        "ORDER BY created_on DESC, uuid DESC"
    };

    conditions.push(TRIGGERER_FILTER.to_string());

    let where_clause = format!("WHERE {}", conditions.join(" AND "));

    let raw_bundles: Vec<BundleWithRawResponsesExplicit> = sqlx::query_as(&format!(
        r#"WITH latest_bundles AS (
            SELECT *
            FROM command_bundles
            {where_clause}
            {order_by}
            LIMIT $1
        )
        SELECT
            b.uuid,
            b.created_on,
            u.email as user_email,
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
        LEFT JOIN auth.users u ON b.user_id = u.id
        LEFT JOIN command_queue cq ON b.uuid = cq.bundle
        LEFT JOIN command_response cr ON cq.id = cr.command_id
        LEFT JOIN device d ON cq.device_id = d.id
        ORDER BY b.created_on DESC;"#,
    ))
    .bind(limit)
    .bind(filter_user_id)
    .bind(filter_system)
    .bind(filter_people)
    .fetch_all(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to retrieve command bundles {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut map_responses: HashMap<(Uuid, _), (Option<String>, Vec<DeviceCommandResponse>)> =
        HashMap::new();

    raw_bundles.into_iter().for_each(|raw_bundle| {
        let response = DeviceCommandResponse {
            device: raw_bundle.device,
            serial_number: raw_bundle.serial_number,
            cmd_id: raw_bundle.cmd_id,
            issued_at: raw_bundle.issued_at,
            cmd_data: redact_cmd_data(raw_bundle.cmd_data),
            cancelled: raw_bundle.cancelled,
            fetched: raw_bundle.fetched,
            fetched_at: raw_bundle.fetched_at,
            response_id: raw_bundle.response_id,
            response_at: raw_bundle.response_at,
            response: raw_bundle.response,
            status: raw_bundle.status,
            user_email: None,
        };

        map_responses
            .entry((raw_bundle.uuid, raw_bundle.created_on))
            .and_modify(
                |(_, responses): &mut (Option<String>, Vec<DeviceCommandResponse>)| {
                    responses.push(response.clone());
                },
            )
            .or_insert((raw_bundle.user_email, vec![response]));
    });

    let mut bundles: Vec<BundleWithCommands> = map_responses
        .into_iter()
        .map(|((uuid, created_on), (user_email, mut responses))| {
            // Keep commands in the order they were issued (queue id is serial), so
            // the displayed order matches how the bundle/recipe was defined.
            responses.sort_by_key(|r| r.cmd_id);
            BundleWithCommands {
                uuid,
                created_on,
                user_email,
                responses,
            }
        })
        .collect();

    // Must match the SQL ordering: `first_id` / `last_id` become the
    // neighbouring pages' cursors.
    bundles.sort_by(|a, b| {
        b.created_on
            .cmp(&a.created_on)
            .then_with(|| b.uuid.cmp(&a.uuid))
    });

    let first_id = bundles.first().map(|c| c.uuid);
    let last_id = bundles.last().map(|c| c.uuid);

    let has_more_first_id = if let Some(first_id) = first_id {
        let more = sqlx::query_scalar!(
            r#"select exists(
                select 1 from command_bundles
                where (created_on, uuid) > (
                    select created_on, uuid from command_bundles where uuid = $1
                )
                and ($2::int is null or user_id = $2)
                and (not $3::bool or user_id is null)
                and (not $4::bool or user_id is not null)
                order by created_on asc
                limit 1
            )"#,
            first_id,
            filter_user_id,
            filter_system,
            filter_people
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
                where (created_on, uuid) < (
                    select created_on, uuid from command_bundles where uuid = $1
                )
                and ($2::int is null or user_id = $2)
                and (not $3::bool or user_id is null)
                and (not $4::bool or user_id is not null)
                order by created_on desc
                limit 1
            )"#,
            last_id,
            filter_user_id,
            filter_system,
            filter_people
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

    // The filter must travel with the cursor or `next` silently widens back to
    // every triggerer. Rebuilt from the parsed values rather than echoed: `+5`
    // parses as 5 but decodes back as " 5", which would 400 our own link.
    let filter_query = match (filter_user_id, filter_system, filter_people) {
        (Some(user_id), _, _) => format!("&triggered_by={user_id}"),
        (_, true, _) => "&triggered_by=system".to_string(),
        (_, _, true) => "&triggered_by=people".to_string(),
        _ => String::new(),
    };

    let next = last_id.filter(|_| has_more_last_id).map(|last_id| {
        format!(
            "https://{}/commands/bundles?starting_after={last_id}&limit={limit}{filter_query}",
            host.0
        )
    });

    let previous = first_id.filter(|_| has_more_first_id).map(|first_id| {
        format!(
            "https://{}/commands/bundles?ending_before={first_id}&limit={limit}{filter_query}",
            host.0
        )
    });

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
            u.email as user_email,
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
        LEFT JOIN auth.users u ON b.user_id = u.id
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
    let user_email = first.user_email.clone();

    let responses = rows
        .into_iter()
        .map(|raw| DeviceCommandResponse {
            device: raw.device,
            serial_number: raw.serial_number,
            cmd_id: raw.cmd_id,
            issued_at: raw.issued_at,
            cmd_data: redact_cmd_data(raw.cmd_data),
            cancelled: raw.cancelled,
            fetched: raw.fetched,
            fetched_at: raw.fetched_at,
            response_id: raw.response_id,
            response_at: raw.response_at,
            response: raw.response,
            status: raw.status,
            user_email: None,
        })
        .collect();

    Ok(Json(BundleWithCommands {
        uuid,
        created_on,
        user_email,
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
    if !authorization::check(current_user.clone(), "recipes", "trigger") {
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

    let receipt = queue_commands_bundle(
        &state.pg_pool,
        &input.devices,
        &commands,
        current_user.user_id,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(receipt)))
}

#[cfg(test)]
mod stagger_tests {
    use super::*;

    fn request(command: SafeCommandTx) -> SafeCommandRequest {
        SafeCommandRequest {
            id: -1,
            command,
            continue_on_error: false,
        }
    }

    #[test]
    fn wifi_scan_has_a_pacing_policy() {
        let policy = stagger_policy(&SafeCommandTx::WifiScan, 5).expect("WifiScan should be paced");
        assert_eq!(policy.wave_size, 2);
        assert_eq!(policy.wave_duration, Duration::from_secs(10));
    }

    #[test]
    fn unpaced_commands_have_no_policy() {
        assert!(stagger_policy(&SafeCommandTx::Ping, 5).is_none());
    }

    #[test]
    fn merge_picks_up_the_only_policied_command_in_a_mixed_bundle() {
        let commands = [
            request(SafeCommandTx::Ping),
            request(SafeCommandTx::WifiScan),
        ];
        let policy =
            merged_stagger_policy(&commands, 5).expect("bundle contains a policied command");
        assert_eq!(policy.wave_size, 2);
        assert_eq!(policy.wave_duration, Duration::from_secs(10));
    }

    #[test]
    fn merge_returns_none_when_nothing_in_the_bundle_is_policied() {
        let commands = [request(SafeCommandTx::Ping)];
        assert!(merged_stagger_policy(&commands, 5).is_none());
    }

    #[test]
    fn test_network_wave_size_is_bundle_relative() {
        let normal =
            stagger_policy(&SafeCommandTx::TestNetwork, 20).expect("TestNetwork should be paced");
        assert_eq!(normal.wave_size, 2); // round(20 * 0.10) = 2
        assert_eq!(normal.wave_duration, Duration::from_secs(60));

        // round(0.3) floors to 1.
        let floored =
            stagger_policy(&SafeCommandTx::TestNetwork, 3).expect("TestNetwork should be paced");
        assert_eq!(floored.wave_size, 1);
    }

    #[test]
    fn merge_of_test_network_and_wifi_scan_picks_the_strictest_of_each() {
        // At 50 devices the two policies' numbers differ, so this exercises
        // min(wave_size)/max(wave_duration) instead of one winning wholesale.
        let commands = [
            request(SafeCommandTx::TestNetwork),
            request(SafeCommandTx::WifiScan),
        ];
        let test_network_policy =
            stagger_policy(&SafeCommandTx::TestNetwork, 50).expect("TestNetwork should be paced");
        let wifi_scan_policy =
            stagger_policy(&SafeCommandTx::WifiScan, 50).expect("WifiScan should be paced");
        assert_eq!(test_network_policy.wave_size, 5);
        assert!(test_network_policy.wave_size > wifi_scan_policy.wave_size);
        assert!(test_network_policy.wave_duration > wifi_scan_policy.wave_duration);

        let merged =
            merged_stagger_policy(&commands, 50).expect("bundle contains policied commands");
        assert_eq!(merged.wave_size, wifi_scan_policy.wave_size);
        assert_eq!(merged.wave_duration, test_network_policy.wave_duration);
    }

    #[test]
    fn no_policy_leaves_every_device_at_the_column_default() {
        let devices = [1, 2, 3, 4, 5];
        let offsets = assign_wave_offsets(&devices, None);

        assert_eq!(offsets.len(), devices.len());
        assert!(offsets.iter().all(|a| a.offset.is_none()));
    }

    #[test]
    fn waves_group_devices_into_ceil_n_over_k_distinct_slots() {
        let devices: Vec<i32> = (0..5).collect();
        let policy = StaggerPolicy {
            wave_size: 2,
            wave_duration: Duration::from_secs(10),
        };
        let offsets = assign_wave_offsets(&devices, Some(&policy));

        // Shuffling must not drop or duplicate any device.
        let mut assigned_devices: Vec<i32> = offsets.iter().map(|a| a.device_id).collect();
        assigned_devices.sort();
        assert_eq!(assigned_devices, devices);

        // 5 devices at wave_size 2 -> ceil(5/2) = 3 distinct waves, sizes 2, 2, 1.
        let mut counts: HashMap<Duration, usize> = HashMap::new();
        for a in &offsets {
            *counts.entry(a.offset.expect("policy is Some")).or_default() += 1;
        }
        assert_eq!(counts.len(), 3);
        let mut sizes: Vec<usize> = counts.values().copied().collect();
        sizes.sort_unstable();
        assert_eq!(sizes, vec![1, 2, 2]);
    }

    // Needs a live DB at runtime (not just compile time like other `api`
    // tests), which CI doesn't provide, hence `#[ignore]` (`e2e` convention).
    // Lives here, not in `e2e`, since this needs no HTTP/auth.
    #[test]
    #[ignore = "connects to a real, migrated Postgres via DATABASE_URL; not run in \
                CI (test.yml has no live DB). Run against the dev stack, e.g. \
                `docker exec smith-api cargo test -p api -- --ignored`."]
    fn staggered_bundle_available_at_is_correct_after_commit() {
        tokio::runtime::Runtime::new()
            .expect("building a tokio runtime for this test")
            .block_on(staggered_bundle_available_at_is_correct_after_commit_inner());
    }

    async fn staggered_bundle_available_at_is_correct_after_commit_inner() {
        use chrono::DateTime;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must point at a migrated Postgres");
        let pool = sqlx::PgPool::connect(&database_url)
            .await
            .expect("connecting to the test database");

        let user_id: i32 = sqlx::query_scalar("SELECT id FROM auth.users LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("at least one user must exist in auth.users to attribute the bundle to");

        // Throwaway devices, cleaned up at the end (not real fleet devices).
        let unique = Utc::now()
            .timestamp_nanos_opt()
            .expect("current time fits in i64 nanos");
        let mut device_ids = Vec::new();
        for i in 0..5 {
            let id: i32 =
                sqlx::query_scalar("INSERT INTO device (serial_number) VALUES ($1) RETURNING id")
                    .bind(format!("wave-correction-test-{unique}-{i}"))
                    .fetch_one(&pool)
                    .await
                    .expect("inserting a throwaway device");
            device_ids.push(id);
        }

        let commands = vec![SafeCommandRequest {
            id: -1,
            command: SafeCommandTx::WifiScan,
            continue_on_error: false,
        }];

        let result = queue_commands_bundle(&pool, &device_ids, &commands, user_id).await;

        // Read back before cleanup so a failure below doesn't hide what happened.
        let rows: Vec<(i32, DateTime<Utc>)> = match &result {
            Ok(receipt) => {
                sqlx::query_as("SELECT id, available_at FROM command_queue WHERE bundle = $1")
                    .bind(receipt.uuid)
                    .fetch_all(&pool)
                    .await
                    .expect("reading back the inserted rows")
            }
            Err(_) => Vec::new(),
        };

        if let Ok(receipt) = &result {
            sqlx::query("DELETE FROM command_queue WHERE bundle = $1")
                .bind(receipt.uuid)
                .execute(&pool)
                .await
                .expect("cleaning up command_queue rows");
            sqlx::query("DELETE FROM command_bundles WHERE uuid = $1")
                .bind(receipt.uuid)
                .execute(&pool)
                .await
                .expect("cleaning up the command_bundles row");
        }
        sqlx::query("DELETE FROM device WHERE id = ANY($1)")
            .bind(&device_ids)
            .execute(&pool)
            .await
            .expect("cleaning up throwaway devices");

        result.expect("queue_commands_bundle should succeed");
        assert_eq!(
            rows.len(),
            device_ids.len(),
            "every device should have exactly one queued command"
        );

        // Expect 3 waves, sizes 2/2/1. Bucket relative to the earliest row,
        // not a fresh `now()` (cleanup's round trips would skew it), and
        // round rather than truncate to absorb jitter between corrected rows.
        let earliest = rows
            .iter()
            .map(|(_, at)| *at)
            .min()
            .expect("at least one row");
        let mut waves: HashMap<i64, usize> = HashMap::new();
        for (_, available_at) in &rows {
            let delta_ms = (*available_at - earliest).num_milliseconds();
            assert!(
                (0..=25_000).contains(&delta_ms),
                "available_at should land within one bundle's worth of the \
                 earliest row, not a leftover placeholder: {delta_ms}ms"
            );
            let wave = ((delta_ms as f64) / 10_000.0).round() as i64;
            *waves.entry(wave).or_default() += 1;
        }
        assert_eq!(waves.len(), 3, "expected 3 distinct waves, got {waves:?}");
        let mut sizes: Vec<usize> = waves.values().copied().collect();
        sizes.sort_unstable();
        assert_eq!(sizes, vec![1, 2, 2]);
    }
}
