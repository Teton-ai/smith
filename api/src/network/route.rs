use crate::State;
use axum::http::StatusCode;
use axum::response::Result;
use axum::{
    Extension, Json,
    extract::{Path, Query},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use smith::utils::schema::NetworkType;
use smith::utils::schema::{Network, NetworkInfo, NewNetwork, SpeedSample};
use tracing::{error, info};
use utoipa::ToSchema;
use uuid::Uuid;

const NETWORKS_TAG: &str = "networks";
const EXTENDED_TEST_TAG: &str = "extended-network-test";

#[derive(Debug, serde::Deserialize)]
pub struct SerialNumbers {
    serial_numbers: Option<String>,
}

#[utoipa::path(
    get,
    path = "/networks",
    params(
        ("serial_numbers" = Option<String>, Query, description = "Optional list of device serial numbers to filter networks. If not provided, returns all networks")
    ),
    responses(
        (status = 200, description = "List of networks retrieved successfully"),
        (status = 500, description = "Failed to retrieve networks", body = String),
    ),
    security(("auth_token" = [])),
    tag = NETWORKS_TAG
)]
pub async fn get_networks(
    Extension(state): Extension<State>,
    Query(query): Query<SerialNumbers>,
) -> Result<Json<Vec<Network>>, StatusCode> {
    let networks = match query.serial_numbers {
        Some(serial_numbers) => {
            let serials: Vec<String> = serial_numbers.split(',').map(String::from).collect();
            sqlx::query_as!(
                Network,
                r#"
                SELECT
                    n.id,
                    n.network_type::TEXT as "network_type",
                    n.is_network_hidden,
                    n.ssid,
                    n.name,
                    n.description,
                    n.password
                FROM network n
                JOIN device d ON n.id = d.network_id
                WHERE d.serial_number = ANY($1)
                "#,
                &serials[..]
            )
            .fetch_all(&state.pg_pool)
            .await
        }
        None => {
            sqlx::query_as!(
                Network,
                r#"
                SELECT
                    n.id,
                    n.network_type::TEXT as "network_type",
                    n.is_network_hidden,
                    n.ssid,
                    n.name,
                    n.description,
                    n.password
                FROM network n
                "#
            )
            .fetch_all(&state.pg_pool)
            .await
        }
    }
    .map_err(|err| {
        error!("error: failed to get networks: {:?}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(networks))
}

#[utoipa::path(
    get,
    path = "/networks/{network_id}",
    params(
        ("network_id" = i32, Path),
    ),
    responses(
        (status = 200, description = "Return found network"),
        (status = 500, description = "Failed to retrieve network", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = NETWORKS_TAG
)]
pub async fn get_network_by_id(
    Path(network_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<Network>, StatusCode> {
    let network = sqlx::query_as!(
        Network,
        r#"
        SELECT
            network.id,
            network.network_type::TEXT,
            network.is_network_hidden,
            network.ssid,
            network.name,
            network.description,
            network.password
        FROM network
        WHERE network.id = $1
        "#,
        network_id
    )
    .fetch_one(&state.pg_pool)
    .await
    .map_err(|err| {
        error!(
            "error: failed to get network for id {}: {:?}",
            network_id, err
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(network))
}

#[utoipa::path(
    delete,
    path = "/networks/{network_id}",
    params(
        ("network_id" = i32, Path),
    ),
    responses(
        (status = StatusCode::NO_CONTENT, description = "Successfully deleted the network"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to delete network", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = NETWORKS_TAG
)]
pub async fn delete_network_by_id(
    Path(network_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<StatusCode, StatusCode> {
    sqlx::query!(r#"DELETE FROM network WHERE id = $1"#, network_id)
        .execute(&state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to delete network {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/networks",
    responses(
        (status = 201, description = "Network created successfully"),
        (status = 304, description = "Network was not modified"),
        (status = 500, description = "Failed to create network", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = NETWORKS_TAG
)]
pub async fn create_network(
    Extension(state): Extension<State>,
    Json(new_network): Json<NewNetwork>,
) -> Result<(StatusCode, Json<Network>), StatusCode> {
    let created_network = sqlx::query_as!(
        Network,
        r#"
        INSERT INTO network (network_type, is_network_hidden, ssid, name, description, password)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, network_type::TEXT as "network_type", is_network_hidden, ssid, name, description, 'secret' as password
        "#,
        new_network.network_type as NetworkType,
        new_network.is_network_hidden,
        new_network.ssid,
        new_network.name,
        new_network.description,
        new_network.password,
    )
    .fetch_one(&state.pg_pool)
    .await
    .map_err(|err| {
      error!("Failed to insert network {err}");
      StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok((StatusCode::CREATED, Json(created_network)))
}

// Extended network test types and endpoints

#[derive(Debug, Deserialize, ToSchema)]
pub struct StartExtendedTestRequest {
    pub label_filter: String,
    #[serde(default = "default_duration")]
    pub duration_minutes: u32,
}

fn default_duration() -> u32 {
    3
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StartExtendedTestResponse {
    #[schema(value_type = String)]
    pub session_id: Uuid,
    pub device_count: i32,
    pub message: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ExtendedTestStatus {
    #[schema(value_type = String)]
    pub session_id: Uuid,
    pub status: String,
    pub label_filter: String,
    pub duration_minutes: i32,
    pub device_count: i32,
    pub completed_count: i32,
    #[schema(value_type = String)]
    pub created_at: DateTime<Utc>,
    pub results: Vec<DeviceExtendedTestResult>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeviceExtendedTestResult {
    pub device_id: i32,
    pub serial_number: String,
    pub status: String,
    #[schema(value_type = Option<Vec<Object>>)]
    pub minute_stats: Option<Vec<MinuteStats>>,
    #[schema(value_type = Option<Object>)]
    pub network_info: Option<NetworkInfo>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MinuteStats {
    pub minute: u8,
    pub sample_count: u32,
    pub download: SpeedStats,
    pub upload: Option<SpeedStats>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SpeedStats {
    pub average_mbps: f64,
    pub std_dev: f64,
    pub q25: f64,
    pub q50: f64,
    pub q75: f64,
}

#[utoipa::path(
    post,
    path = "/network/extended-test",
    request_body = StartExtendedTestRequest,
    responses(
        (status = 201, description = "Extended test started", body = StartExtendedTestResponse),
        (status = 400, description = "Invalid label filter, duration > 8, or no devices found"),
        (status = 500, description = "Internal server error"),
    ),
    security(("auth_token" = [])),
    tag = EXTENDED_TEST_TAG
)]
pub async fn start_extended_network_test(
    Extension(state): Extension<State>,
    Json(request): Json<StartExtendedTestRequest>,
) -> Result<(StatusCode, Json<StartExtendedTestResponse>), StatusCode> {
    // Validate duration (3-8 minutes)
    if request.duration_minutes < 3 {
        error!(
            duration = request.duration_minutes,
            "Duration must be at least 3 minutes"
        );
        return Err(StatusCode::BAD_REQUEST);
    }
    if request.duration_minutes > 8 {
        error!(
            duration = request.duration_minutes,
            "Duration exceeds maximum of 8 minutes"
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    // Rate limiting: Check if there's already an active extended test
    // A test is considered active if created within 10 minutes and has pending (non-canceled, no response) commands
    let active_test = sqlx::query!(
        r#"
        SELECT nts.id, nts.created_at,
               COUNT(cq.id) FILTER (WHERE NOT cq.canceled) as total_commands,
               COUNT(cr.id) as completed_commands
        FROM network_test_sessions nts
        JOIN command_queue cq ON cq.bundle = nts.bundle_id
        LEFT JOIN command_response cr ON cr.command_id = cq.id
        WHERE nts.created_at > NOW() - INTERVAL '10 minutes'
        GROUP BY nts.id, nts.created_at
        HAVING COUNT(cq.id) FILTER (WHERE NOT cq.canceled) > COUNT(cr.id)
        LIMIT 1
        "#
    )
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|err| {
        error!(error = %err, "Failed to check for active extended tests");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if let Some(active) = active_test {
        error!(
            session_id = %active.id,
            "An extended network test is already running"
        );
        return Err(StatusCode::CONFLICT);
    }

    // Note: label_filter is accepted for API compatibility but currently ignored
    // All online devices are targeted regardless of label
    let _label_filter = request.label_filter.as_str();

    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!(error = %err, "Failed to start transaction");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // TEMP: Get all ONLINE devices for testing (ignores label filter)
    // Online = last_ping within 5 minutes
    let devices = sqlx::query!(
        r#"
        SELECT d.id, d.serial_number
        FROM device d
        WHERE d.archived = false
          AND d.last_ping > NOW() - INTERVAL '5 minutes'
        "#
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|err| {
        error!("Failed to query devices: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if devices.is_empty() {
        error!(
            "No devices found for label filter: {}",
            request.label_filter
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    // Create command bundle
    let bundle_id =
        sqlx::query_scalar!(r#"INSERT INTO command_bundles DEFAULT VALUES RETURNING uuid"#)
            .fetch_one(&mut *tx)
            .await
            .map_err(|err| {
                error!("Failed to create command bundle: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

    // Queue ExtendedNetworkTest command for all devices (bulk insert)
    let command = serde_json::json!({
        "ExtendedNetworkTest": {
            "duration_minutes": request.duration_minutes
        }
    });

    let device_ids: Vec<i32> = devices.iter().map(|d| d.id).collect();
    let mut serial_numbers: Vec<String> = devices.iter().map(|d| d.serial_number.clone()).collect();
    serial_numbers.sort();

    sqlx::query!(
        r#"
        INSERT INTO command_queue (device_id, cmd, continue_on_error, canceled, bundle)
        SELECT unnest($1::int[]), $2::jsonb, false, false, $3
        "#,
        &device_ids,
        command,
        bundle_id
    )
    .execute(&mut *tx)
    .await
    .map_err(|err| {
        error!(error = %err, device_count = device_ids.len(), "Failed to queue commands");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Insert into network_test_sessions for easier querying
    // device_set_hash is MD5 of sorted serial numbers joined by comma
    let serial_numbers_str = serial_numbers.join(",");
    let session_id = sqlx::query_scalar!(
        r#"
        INSERT INTO network_test_sessions (label_filter, duration_minutes, device_count, device_set_hash, bundle_id)
        VALUES ($1, $2, $3, md5($4), $5)
        RETURNING id
        "#,
        &request.label_filter,
        request.duration_minutes as i32,
        devices.len() as i32,
        &serial_numbers_str,
        bundle_id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| {
        error!(error = %err, "Failed to insert network_test_session");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    tx.commit().await.map_err(|err| {
        error!("Failed to commit transaction: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok((
        StatusCode::CREATED,
        Json(StartExtendedTestResponse {
            session_id,
            device_count: devices.len() as i32,
            message: format!(
                "Started extended network test for {} devices with label '{}'",
                devices.len(),
                request.label_filter
            ),
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/network/extended-test/{session_id}",
    params(
        ("session_id" = String, Path, description = "Extended test session ID (bundle UUID)")
    ),
    responses(
        (status = 200, description = "Extended test status", body = ExtendedTestStatus),
        (status = 404, description = "Session not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(("auth_token" = [])),
    tag = EXTENDED_TEST_TAG
)]
pub async fn get_extended_test_status(
    Path(session_id): Path<Uuid>,
    Extension(state): Extension<State>,
) -> Result<Json<ExtendedTestStatus>, StatusCode> {
    // Get session info from network_test_sessions
    struct SessionRow {
        _id: Uuid,
        created_at: DateTime<Utc>,
        label_filter: String,
        duration_minutes: i32,
        _device_count: i32,
        bundle_id: Uuid,
    }

    let session = sqlx::query_as!(
        SessionRow,
        r#"
        SELECT id as "_id", created_at, label_filter, duration_minutes, device_count as "_device_count", bundle_id
        FROM network_test_sessions
        WHERE id = $1
        "#,
        session_id
    )
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to fetch session: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    // Get all commands and responses for this bundle
    #[derive(Debug)]
    struct CommandRow {
        device_id: i32,
        serial_number: String,
        fetched: bool,
        canceled: bool,
        response: Option<Value>,
    }

    let rows = sqlx::query_as!(
        CommandRow,
        r#"
        SELECT
            cq.device_id,
            d.serial_number,
            cq.fetched as "fetched!",
            cq.canceled as "canceled!",
            cr.response as "response?"
        FROM command_queue cq
        JOIN device d ON d.id = cq.device_id
        LEFT JOIN command_response cr ON cr.command_id = cq.id
        WHERE cq.bundle = $1
        "#,
        session.bundle_id
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to fetch commands: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if rows.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    let duration_minutes = session.duration_minutes;

    // Build results
    let mut results = Vec::new();
    let mut completed_count = 0;
    let mut canceled_count = 0;

    for row in &rows {
        let (status, minute_stats, network_info) = if let Some(response) = &row.response {
            // Parse ExtendedNetworkTest response
            if let Some(ext_test) = response.get("ExtendedNetworkTest") {
                completed_count += 1;

                let samples: Vec<SpeedSample> = ext_test
                    .get("samples")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();

                let network_info: Option<NetworkInfo> = ext_test
                    .get("network_info")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());

                let minute_stats = compute_minute_stats(&samples, duration_minutes as u32);

                ("completed".to_string(), Some(minute_stats), network_info)
            } else {
                ("failed".to_string(), None, None)
            }
        } else if row.canceled {
            canceled_count += 1;
            ("canceled".to_string(), None, None)
        } else if row.fetched {
            ("running".to_string(), None, None)
        } else {
            ("pending".to_string(), None, None)
        };

        results.push(DeviceExtendedTestResult {
            device_id: row.device_id,
            serial_number: row.serial_number.clone(),
            status,
            minute_stats,
            network_info,
        });
    }

    let device_count = rows.len() as i32;
    // Test is complete when all commands have either responded or been canceled
    let all_resolved = completed_count + canceled_count == device_count;
    let overall_status = if all_resolved {
        if canceled_count > 0 {
            "canceled" // At least some were canceled
        } else {
            "completed"
        }
    } else if completed_count > 0 {
        "partial"
    } else if rows.iter().any(|r| r.fetched) {
        "running"
    } else {
        "pending"
    };

    let response = ExtendedTestStatus {
        session_id,
        status: overall_status.to_string(),
        label_filter: session.label_filter,
        duration_minutes,
        device_count,
        completed_count,
        created_at: session.created_at,
        results,
    };

    Ok(Json(response))
}

fn compute_minute_stats(samples: &[SpeedSample], duration_minutes: u32) -> Vec<MinuteStats> {
    let mut minute_stats = Vec::new();

    for minute in 0..duration_minutes {
        // Filter samples for this minute
        let minute_samples: Vec<&SpeedSample> = samples
            .iter()
            .filter(|s| {
                // Calculate which minute this sample belongs to based on its position
                // We use the index relative to the first sample's timestamp
                if let Some(first) = samples.first() {
                    let elapsed_secs = (s.started_at - first.started_at).num_seconds();
                    let sample_minute = (elapsed_secs / 60) as u32;
                    sample_minute == minute
                } else {
                    false
                }
            })
            .collect();

        if minute_samples.is_empty() {
            continue;
        }

        let download_values: Vec<f64> = minute_samples.iter().map(|s| s.download_mbps).collect();
        let upload_values: Vec<f64> = minute_samples
            .iter()
            .filter_map(|s| s.upload_mbps)
            .collect();

        let download_stats = compute_speed_stats(&download_values);
        let upload_stats = if upload_values.is_empty() {
            None
        } else {
            Some(compute_speed_stats(&upload_values))
        };

        minute_stats.push(MinuteStats {
            minute: minute as u8,
            sample_count: minute_samples.len() as u32,
            download: download_stats,
            upload: upload_stats,
        });
    }

    minute_stats
}

fn compute_speed_stats(values: &[f64]) -> SpeedStats {
    if values.is_empty() {
        return SpeedStats {
            average_mbps: 0.0,
            std_dev: 0.0,
            q25: 0.0,
            q50: 0.0,
            q75: 0.0,
        };
    }

    let n = values.len() as f64;
    let average_mbps = values.iter().sum::<f64>() / n;

    let variance = values
        .iter()
        .map(|v| (v - average_mbps).powi(2))
        .sum::<f64>()
        / n;
    let std_dev = variance.sqrt();

    // Sort for percentiles
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let q25 = percentile(&sorted, 25.0);
    let q50 = percentile(&sorted, 50.0);
    let q75 = percentile(&sorted, 75.0);

    SpeedStats {
        average_mbps,
        std_dev,
        q25,
        q50,
        q75,
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }

    let idx = (p / 100.0) * (sorted.len() - 1) as f64;
    let lower = idx.floor() as usize;
    let upper = idx.ceil() as usize;

    if lower == upper {
        sorted[lower]
    } else {
        let frac = idx - lower as f64;
        sorted[lower] * (1.0 - frac) + sorted[upper] * frac
    }
}

// Session listing types and endpoint

#[derive(Debug, Serialize, ToSchema)]
pub struct ExtendedTestSessionSummary {
    #[schema(value_type = String)]
    pub session_id: Uuid,
    #[schema(value_type = String)]
    pub created_at: DateTime<Utc>,
    pub device_count: i64,
    pub completed_count: i64,
    pub status: String,
}

#[utoipa::path(
    get,
    path = "/network/extended-test/sessions",
    responses(
        (status = 200, description = "List of extended test sessions", body = Vec<ExtendedTestSessionSummary>),
        (status = 500, description = "Internal server error"),
    ),
    security(("auth_token" = [])),
    tag = EXTENDED_TEST_TAG
)]
pub async fn list_extended_test_sessions(
    Extension(state): Extension<State>,
) -> Result<Json<Vec<ExtendedTestSessionSummary>>, StatusCode> {
    // Query from network_test_sessions, join to command_queue/response for completion count
    let sessions = sqlx::query!(
        r#"
        SELECT
            nts.id,
            nts.created_at,
            nts.device_count,
            COUNT(cr.id) as completed_count,
            COUNT(cq.id) FILTER (WHERE cq.canceled) as canceled_count
        FROM network_test_sessions nts
        JOIN command_queue cq ON cq.bundle = nts.bundle_id
        LEFT JOIN command_response cr ON cr.command_id = cq.id
        GROUP BY nts.id, nts.created_at, nts.device_count
        ORDER BY nts.created_at DESC
        LIMIT 50
        "#
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to fetch extended test sessions: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let summaries: Vec<ExtendedTestSessionSummary> = sessions
        .into_iter()
        .map(|row| {
            let device_count = row.device_count as i64;
            let completed_count = row.completed_count.unwrap_or(0);
            let canceled_count = row.canceled_count.unwrap_or(0);
            let all_resolved = completed_count + canceled_count >= device_count;
            let status = if all_resolved {
                if canceled_count > 0 {
                    "canceled"
                } else {
                    "completed"
                }
            } else if completed_count > 0 {
                "partial"
            } else {
                "running"
            };

            ExtendedTestSessionSummary {
                session_id: row.id,
                created_at: row.created_at,
                device_count,
                completed_count,
                status: status.to_string(),
            }
        })
        .collect();

    Ok(Json(summaries))
}

#[derive(Serialize, ToSchema)]
pub struct CancelExtendedTestResponse {
    pub canceled_count: i64,
    pub message: String,
}

/// Cancel a running extended network test
///
/// Marks all pending commands as canceled, allowing the test to complete with current results.
#[utoipa::path(
    post,
    path = "/network/extended-test/{session_id}/cancel",
    params(
        ("session_id" = String, Path, description = "Extended test session ID (bundle UUID)"),
    ),
    responses(
        (status = 200, description = "Test canceled", body = CancelExtendedTestResponse),
        (status = 404, description = "Session not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(("auth_token" = [])),
    tag = EXTENDED_TEST_TAG
)]
pub async fn cancel_extended_test(
    Path(session_id): Path<Uuid>,
    Extension(state): Extension<State>,
) -> Result<Json<CancelExtendedTestResponse>, StatusCode> {
    // Get the bundle_id from network_test_sessions
    let session = sqlx::query!(
        "SELECT bundle_id FROM network_test_sessions WHERE id = $1",
        session_id
    )
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|err| {
        error!(session_id = %session_id, error = %err, "Failed to fetch session");
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    // Cancel all pending commands for this session (not already responded, not already canceled)
    let result = sqlx::query!(
        r#"
        UPDATE command_queue
        SET canceled = true
        WHERE bundle = $1
          AND canceled = false
          AND id NOT IN (SELECT command_id FROM command_response WHERE command_id IS NOT NULL)
        "#,
        session.bundle_id
    )
    .execute(&state.pg_pool)
    .await
    .map_err(|err| {
        error!(session_id = %session_id, error = %err, "Failed to cancel extended test");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let canceled_count = result.rows_affected() as i64;

    info!(
        session_id = %session_id,
        canceled_count = canceled_count,
        "Extended test canceled"
    );

    Ok(Json(CancelExtendedTestResponse {
        canceled_count,
        message: format!("Canceled {} pending commands", canceled_count),
    }))
}
