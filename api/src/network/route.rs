use crate::State;
use crate::user::CurrentUser;
use axum::http::StatusCode;
use axum::response::Result;
use axum::{
    Extension, Json,
    extract::{Path, Query},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use smith::utils::schema::NetworkType;
use smith::utils::schema::{Network, NetworkInfo, NewNetwork, SpeedSample};
use tracing::{error, info, warn};
use utoipa::ToSchema;
use uuid::Uuid;

use super::evaluation::{Evaluation, evaluate};

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

/// The one construction of the content-addressing `credentials` envelope.
///
/// Both the lock key and the identity match project `->>'psk'`, so a row must be
/// stored with exactly the envelope it was looked up with. Building it anywhere
/// else risks inserting a row that the very key which created it cannot find
/// again, which turns every repeat write into a fresh duplicate.
pub(crate) fn content_credentials(password: Option<&str>) -> Value {
    match password {
        Some(psk) => json!({ "psk": psk }),
        None => json!({}),
    }
}

/// The `network_type` enum label as stored in Postgres, for the text-typed
/// `p_network_type` argument of `network_find_by_content`.
fn network_type_label(network_type: &NetworkType) -> &'static str {
    match network_type {
        NetworkType::Wifi => "wifi",
        NetworkType::Ethernet => "ethernet",
        NetworkType::Dongle => "dongle",
    }
}

/// Every failure in `create_network` is an opaque 500 to the caller; only the log
/// line differs. Keeps the handler readable instead of repeating the same closure.
fn internal_error(context: &'static str) -> impl Fn(sqlx::Error) -> StatusCode {
    move |err| {
        error!("{context}: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

/// Maps the App API `wifi_security_enum` to Smith's `security_type` vocabulary.
/// Tokens verified against `../api/migrations/20250917114752_new_network_tables.sql`
/// and `../api/src/features/networks/schema.ts` (App API repo). Provisional: to be
/// re-verified when the App API actually starts sending `security` (Stage 5).
fn map_app_security(security: &str) -> Option<&'static str> {
    match security {
        "open" => Some("open"),
        "WPA2-Personal" => Some("wpa-psk"),
        "WPA2-Enterprise" => Some("wpa-eap"),
        _ => None,
    }
}

/// `None` (field omitted) is the only case that falls back to the password
/// heuristic, for backward compatibility with callers that predate `security`.
/// An explicit but unrecognized value is rejected rather than silently
/// downgraded to a guess: persisting the wrong `security_type` would produce
/// incorrect content matches (see `network_find_by_content`).
fn resolve_security_type(
    security: Option<&str>,
    password: Option<&str>,
) -> Result<&'static str, ()> {
    match security {
        None => Ok(if password.is_none() {
            "open"
        } else {
            "wpa-psk"
        }),
        Some(security) => map_app_security(security).ok_or_else(|| {
            warn!(
                security,
                "unknown explicit security value in create_network; rejecting"
            );
        }),
    }
}

/// `security_type` is WiFi-specific vocabulary (see `NewNetwork::security`); an
/// Ethernet or Dongle network has no security type, so it stays NULL instead of
/// getting a meaningless "open"/"wpa-psk" guess from `resolve_security_type`.
fn security_type_for(
    network_type: &NetworkType,
    security: Option<&str>,
    password: Option<&str>,
) -> Result<Option<&'static str>, ()> {
    if *network_type != NetworkType::Wifi {
        return Ok(None);
    }
    resolve_security_type(security, password).map(Some)
}

#[utoipa::path(
    post,
    path = "/networks",
    responses(
        (status = 200, description = "Matching network already existed"),
        (status = 201, description = "Network created successfully"),
        (status = 400, description = "Unrecognized `security` value"),
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
    let security_type: Option<&str> = security_type_for(
        &new_network.network_type,
        new_network.security.as_deref(),
        new_network.password.as_deref(),
    )
    .map_err(|()| StatusCode::BAD_REQUEST)?;

    if security_type == Some("open") && new_network.password.is_some() {
        warn!("create_network got an open network with a password; keeping the credential");
    }

    // Built unconditionally from the password, never from security_type: the
    // stored envelope has to be byte-identical to the one the lock and the match
    // are computed from (see content_credentials).
    let credentials = content_credentials(new_network.password.as_deref());
    let network_type_label = network_type_label(&new_network.network_type);

    let mut tx = state
        .pg_pool
        .begin()
        .await
        .map_err(internal_error("Failed to begin create_network transaction"))?;

    // Race-tested in e2e/tests/daemon_api.rs
    // (concurrent_identical_network_posts_converge_to_one_row).
    sqlx::query!(
        "SELECT pg_advisory_xact_lock(network_content_lock_key($1, $2, $3))",
        new_network.ssid,
        new_network.is_network_hidden,
        credentials,
    )
    .execute(&mut *tx)
    .await
    .map_err(internal_error("Failed to take network content lock"))?;

    let existing_id: Option<i32> = sqlx::query_scalar!(
        r#"SELECT network_find_by_content($1, $2, $3, $4, $5)"#,
        new_network.ssid,
        new_network.is_network_hidden,
        credentials,
        security_type as Option<&str>,
        network_type_label,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(internal_error(
        "Failed to match existing network by content",
    ))?;

    let (status, network) = match existing_id {
        Some(id) => {
            // Heal in place exactly as ReportNMProfiles does (api/src/home.rs):
            // the relaxed match can only route a typed caller to a NULL row or an
            // already-equal typed row, so COALESCE fills unknown -> known and
            // never clobbers a real value. Folded into the fetch to keep the
            // matched path at one round trip.
            let existing = sqlx::query_as!(
                Network,
                r#"
                UPDATE network SET security_type = COALESCE(security_type, $2)
                WHERE id = $1
                RETURNING id, network_type::TEXT as "network_type", is_network_hidden, ssid, name, description, 'secret' as password
                "#,
                id,
                security_type as Option<&str>,
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(internal_error("Failed to fetch matched network"))?;
            (StatusCode::OK, existing)
        }
        None => {
            let created = sqlx::query_as!(
                Network,
                r#"
                INSERT INTO network (network_type, is_network_hidden, ssid, name, description, password, security_type, credentials)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                RETURNING id, network_type::TEXT as "network_type", is_network_hidden, ssid, name, description, 'secret' as password
                "#,
                new_network.network_type as NetworkType,
                new_network.is_network_hidden,
                new_network.ssid,
                new_network.name,
                new_network.description,
                new_network.password,
                security_type as Option<&str>,
                credentials,
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(internal_error("Failed to insert network"))?;
            (StatusCode::CREATED, created)
        }
    };

    tx.commit().await.map_err(internal_error(
        "Failed to commit create_network transaction",
    ))?;

    Ok((status, Json(network)))
}

#[cfg(test)]
mod tests {
    use super::{
        content_credentials, map_app_security, network_type_label, resolve_security_type,
        security_type_for,
    };
    use serde_json::json;
    use smith::utils::schema::NetworkType;

    /// The whole point of `content_credentials`: what gets stored on the row and
    /// what the lock/match project must be the same value. A row inserted with a
    /// different envelope than it was searched with is invisible to the next
    /// lookup, so every repeat write forks a duplicate.
    #[test]
    fn content_credentials_round_trips_the_psk() {
        assert_eq!(
            content_credentials(Some("hunter2")),
            json!({"psk": "hunter2"})
        );
        assert_eq!(content_credentials(None), json!({}));
    }

    /// `->>'psk'` (what the SQL functions actually compare) must agree for the
    /// no-password case regardless of which writer built the envelope.
    #[test]
    fn content_credentials_has_no_psk_key_when_absent() {
        assert!(content_credentials(None).get("psk").is_none());
    }

    #[test]
    fn network_type_label_matches_the_pg_enum_labels() {
        assert_eq!(network_type_label(&NetworkType::Wifi), "wifi");
        assert_eq!(network_type_label(&NetworkType::Ethernet), "ethernet");
        assert_eq!(network_type_label(&NetworkType::Dongle), "dongle");
    }

    #[test]
    fn map_app_security_known_values() {
        assert_eq!(map_app_security("open"), Some("open"));
        assert_eq!(map_app_security("WPA2-Personal"), Some("wpa-psk"));
        assert_eq!(map_app_security("WPA2-Enterprise"), Some("wpa-eap"));
    }

    #[test]
    fn map_app_security_unknown_returns_none() {
        assert_eq!(map_app_security("wep"), None);
        assert_eq!(map_app_security(""), None);
        assert_eq!(map_app_security("WPA3-Personal"), None);
    }

    /// Omitting `security` is the only case allowed to fall back to the
    /// password heuristic (backward compatibility with pre-`security` callers).
    #[test]
    fn resolve_security_type_omitted_uses_password_heuristic() {
        assert_eq!(resolve_security_type(None, None), Ok("open"));
        assert_eq!(resolve_security_type(None, Some("hunter2")), Ok("wpa-psk"));
    }

    #[test]
    fn resolve_security_type_known_explicit_value_is_used_verbatim() {
        assert_eq!(
            resolve_security_type(Some("open"), Some("hunter2")),
            Ok("open")
        );
        assert_eq!(
            resolve_security_type(Some("WPA2-Enterprise"), None),
            Ok("wpa-eap")
        );
    }

    /// An explicit but unrecognized value must be rejected, not silently
    /// downgraded to a guess (a wrong security_type produces wrong content
    /// matches; see network_find_by_content).
    #[test]
    fn resolve_security_type_rejects_unknown_explicit_value() {
        assert_eq!(resolve_security_type(Some("WPA3-Personal"), None), Err(()));
    }

    #[test]
    fn security_type_for_non_wifi_is_always_null() {
        assert_eq!(
            security_type_for(&NetworkType::Ethernet, None, Some("hunter2")),
            Ok(None)
        );
        assert_eq!(
            security_type_for(&NetworkType::Dongle, Some("open"), None),
            Ok(None)
        );
        // Even an unrecognized explicit value is ignored for non-WiFi types,
        // since the vocabulary doesn't apply to them in the first place.
        assert_eq!(
            security_type_for(&NetworkType::Ethernet, Some("garbage"), None),
            Ok(None)
        );
    }

    #[test]
    fn security_type_for_wifi_delegates_to_resolve_security_type() {
        assert_eq!(
            security_type_for(&NetworkType::Wifi, None, Some("hunter2")),
            Ok(Some("wpa-psk"))
        );
        assert_eq!(
            security_type_for(&NetworkType::Wifi, Some("garbage"), None),
            Err(())
        );
    }
}

// Extended network test types and endpoints

#[derive(Debug, Deserialize, ToSchema)]
pub struct StartExtendedTestRequest {
    pub label_filter: String,
    #[serde(default)]
    pub serial_numbers: Vec<String>,
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
    pub evaluation: Evaluation,
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

#[derive(Debug, Serialize, Clone, ToSchema)]
pub struct MinuteStats {
    pub minute: u8,
    pub sample_count: u32,
    pub download: SpeedStats,
    pub upload: Option<SpeedStats>,
}

#[derive(Debug, Serialize, Clone, ToSchema)]
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
    Extension(current_user): Extension<CurrentUser>,
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

    let label_filters: Vec<String> = request
        .label_filter
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().to_string())
        .collect();

    let mut tx = state.pg_pool.begin().await.map_err(|err| {
        error!(error = %err, "Failed to start transaction");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let devices: Vec<(i32, String)> = if !request.serial_numbers.is_empty() {
        sqlx::query_as(
            r#"
            SELECT d.id, d.serial_number
            FROM device d
            WHERE d.archived = false
              AND d.last_ping > NOW() - INTERVAL '5 minutes'
              AND d.serial_number = ANY($1)
            "#,
        )
        .bind(&request.serial_numbers)
        .fetch_all(&mut *tx)
        .await
    } else if label_filters.is_empty() {
        sqlx::query_as(
            r#"
            SELECT d.id, d.serial_number
            FROM device d
            WHERE d.archived = false
              AND d.last_ping > NOW() - INTERVAL '5 minutes'
            "#,
        )
        .fetch_all(&mut *tx)
        .await
    } else {
        sqlx::query_as(
            r#"
            SELECT DISTINCT d.id, d.serial_number
            FROM device d
            JOIN device_label dl ON dl.device_id = d.id
            JOIN label l ON l.id = dl.label_id
            WHERE d.archived = false
              AND d.last_ping > NOW() - INTERVAL '5 minutes'
              AND l.name || '=' || dl.value = ANY($1)
            "#,
        )
        .bind(&label_filters)
        .fetch_all(&mut *tx)
        .await
    }
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
    let bundle_id = sqlx::query_scalar!(
        r#"INSERT INTO command_bundles (user_id) VALUES ($1) RETURNING uuid"#,
        current_user.user_id
    )
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

    let device_ids: Vec<i32> = devices.iter().map(|d| d.0).collect();
    let mut serial_numbers: Vec<String> = devices.iter().map(|d| d.1.clone()).collect();
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
    let mut failed_count = 0;
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
                failed_count += 1;
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
    // Test is complete when all commands have either responded (completed or failed) or been canceled
    let all_resolved = completed_count + failed_count + canceled_count == device_count;
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

    let evaluation = evaluate(&results);

    let response = ExtendedTestStatus {
        session_id,
        status: overall_status.to_string(),
        label_filter: session.label_filter,
        duration_minutes,
        device_count,
        completed_count,
        created_at: session.created_at,
        results,
        evaluation,
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
    pub label_filter: String,
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
            nts.label_filter,
            nts.device_count,
            COUNT(cr.id) as completed_count,
            COUNT(cq.id) FILTER (WHERE cq.canceled) as canceled_count
        FROM network_test_sessions nts
        JOIN command_queue cq ON cq.bundle = nts.bundle_id
        LEFT JOIN command_response cr ON cr.command_id = cq.id
        GROUP BY nts.id, nts.created_at, nts.label_filter, nts.device_count
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
                label_filter: row.label_filter,
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

#[derive(Debug, Deserialize)]
pub struct FindSessionsByDevicesQuery {
    /// Comma-separated list of device serial numbers
    pub serial_numbers: String,
}

/// Find extended test sessions that were run for a specific set of devices
///
/// The serial numbers are hashed and compared against stored device_set_hash to find exact matches.
#[utoipa::path(
    get,
    path = "/network/extended-test/sessions/by-devices",
    params(
        ("serial_numbers" = String, Query, description = "Comma-separated list of device serial numbers")
    ),
    responses(
        (status = 200, description = "Sessions matching the device set", body = Vec<ExtendedTestSessionSummary>),
        (status = 400, description = "Invalid serial numbers"),
        (status = 500, description = "Internal server error"),
    ),
    security(("auth_token" = [])),
    tag = EXTENDED_TEST_TAG
)]
pub async fn find_sessions_by_devices(
    Query(query): Query<FindSessionsByDevicesQuery>,
    Extension(state): Extension<State>,
) -> Result<Json<Vec<ExtendedTestSessionSummary>>, StatusCode> {
    // Parse and sort serial numbers, then compute hash
    let mut serial_numbers: Vec<&str> = query
        .serial_numbers
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if serial_numbers.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    serial_numbers.sort_unstable();
    serial_numbers.dedup();
    let serial_numbers_str = serial_numbers.join(",");

    // Query sessions with matching device_set_hash (using PostgreSQL md5)
    let sessions = sqlx::query!(
        r#"
        SELECT
            nts.id,
            nts.created_at,
            nts.label_filter,
            nts.device_count,
            COUNT(cr.id) as completed_count,
            COUNT(cq.id) FILTER (WHERE cq.canceled) as canceled_count
        FROM network_test_sessions nts
        JOIN command_queue cq ON cq.bundle = nts.bundle_id
        LEFT JOIN command_response cr ON cr.command_id = cq.id
        WHERE nts.device_set_hash = md5($1)
        GROUP BY nts.id, nts.created_at, nts.label_filter, nts.device_count
        ORDER BY nts.created_at DESC
        LIMIT 50
        "#,
        &serial_numbers_str
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to fetch sessions by device hash: {err}");
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
                label_filter: row.label_filter,
                device_count,
                completed_count,
                status: status.to_string(),
            }
        })
        .collect();

    Ok(Json(summaries))
}
