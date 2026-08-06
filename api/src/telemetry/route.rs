use crate::State;
use crate::handlers::AuthedDevice;
use crate::modem::{clear_modem, save_modem};
use axum::body::{Body, to_bytes};
use axum::extract::{Path, Query, Request};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use tracing::{error, warn};
use utoipa::{IntoParams, ToSchema};

const TELEMETRY_TAG: &str = "telemetry";

#[utoipa::path(
    post,
    path = "/smith/telemetry/victoria",
    responses(
        (status = 200, description = "Victoria metrics data forwarded successfully"),
        (status = 501, description = "Victoria metrics not implemented")
    ),
    security(
        ("device_token" = [])
    ),
)]
pub async fn victoria(
    device: AuthedDevice,
    Extension(state): Extension<State>,
    req: Request<Body>,
) -> Result<StatusCode, StatusCode> {
    let client_config = state
        .config
        .victoria_metrics_client
        .as_ref()
        .ok_or(StatusCode::NOT_IMPLEMENTED)?;

    let (parts, body) = req.into_parts();
    let method = parts.method;
    let mut headers = parts.headers;

    headers.remove("authorization");
    // A device hanging up mid-upload is not a server fault, and answering 500 only makes
    // it retry the same payload.
    let Ok(body_bytes) = to_bytes(body, usize::MAX).await.inspect_err(|err| {
        warn!(
            error = %err,
            serial_number = %device.serial_number,
            "Client disconnected before telemetry body was fully read"
        );
    }) else {
        return Ok(StatusCode::OK);
    };

    let response = client_config
        .client
        .request(method, &client_config.url)
        .headers(headers)
        .body(body_bytes)
        .send()
        .await;

    // Best-effort: keep the single-instance target off the device's critical path.
    match response {
        Ok(res) if !res.status().is_success() => {
            error!(
                status = %res.status(),
                serial_number = device.serial_number,
                "VictoriaMetrics rejected telemetry"
            );
        }
        Ok(_) => {}
        Err(err) => {
            error!(error = %err, "Failed to forward request to VictoriaMetrics");
        }
    }

    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct NewModem {
    pub imei: String,
    pub network_provider: String,
}

#[utoipa::path(
    post,
    path = "/smith/telemetry/modem",
    responses(
        (status = 200, description = "Modem telemetry data processed successfully"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("device_token" = [])
    ),
)]
pub async fn modem(
    device: AuthedDevice,
    Extension(state): Extension<State>,
    Json(modem): Json<Option<NewModem>>,
) -> Result<StatusCode, StatusCode> {
    tokio::spawn(async move {
        match modem {
            Some(modem) => {
                let _ = save_modem(
                    device.serial_number,
                    modem.imei,
                    modem.network_provider,
                    &state.pg_pool,
                )
                .await;
            }
            None => {
                let _ = clear_modem(device.serial_number, &state.pg_pool).await;
            }
        }
    });
    Ok(StatusCode::OK)
}

/// Longest window a telemetry query will serve. A hand-crafted `from` shouldn't
/// be able to ask VictoriaMetrics to scan a device's entire retention.
const TELEMETRY_MAX_WINDOW_DAYS: i64 = 31;
const TELEMETRY_DEFAULT_WINDOW_HOURS: i64 = 1;
const TELEMETRY_DEFAULT_STEP_SECONDS: u32 = 60;
const TELEMETRY_MIN_STEP_SECONDS: u32 = 5;
/// Enough for the largest department several times over, but bounded so one
/// request can't build an unbounded regex alternation.
const TELEMETRY_MAX_SERIALS: usize = 500;

fn is_safe_metric_name(metric: &str) -> bool {
    let mut chars = metric.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == ':' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

fn is_safe_serial(serial: &str) -> bool {
    !serial.is_empty() && serial.chars().all(|c| c.is_ascii_alphanumeric())
}

/// One device, reduced to a single line. A device that exports the same metric
/// once per core, disk, or fan would otherwise come back as several series that
/// the caller has no way to tell apart.
fn single_query(metric: &str, selector: &str, rate: bool) -> String {
    let inner = if rate {
        format!("rate({metric}{{{selector}}}[5m])")
    } else {
        format!("{metric}{{{selector}}}")
    };
    format!("avg by (serial_number) ({inner})")
}

/// Many devices, reduced to one series per `serial_number`. `last_over_time`
/// carries the most recent sample forward so a device reporting on a slow
/// interval still lands in the result between scrapes.
fn batch_query(metric: &str, selector: &str, rate: bool) -> String {
    let inner = if rate {
        format!("rate({metric}{{{selector}}}[5m])")
    } else {
        format!("last_over_time({metric}{{{selector}}}[5m])")
    };
    format!("max by (serial_number) ({inner})")
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TelemetryQuery {
    /// Name of the series to read, e.g. `node_cpu_usage_percent`.
    pub metric: String,
    /// Set when the series is a counter, so it is read as a per-second rate.
    #[serde(default)]
    pub rate: bool,
    /// Start of the window (RFC 3339). Defaults to one hour before `to`.
    pub from: Option<DateTime<Utc>>,
    /// End of the window (RFC 3339). Defaults to now.
    pub to: Option<DateTime<Utc>>,
    /// Resolution in seconds. Defaults to 60.
    pub step: Option<u32>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TelemetryBatchQuery {
    /// Comma-separated serial numbers.
    pub serials: String,
    /// Name of the series to read, e.g. `node_cpu_usage_percent`.
    pub metric: String,
    /// Set when the series is a counter, so it is read as a per-second rate.
    #[serde(default)]
    pub rate: bool,
    /// Start of the window (RFC 3339). Defaults to one hour before `to`.
    pub from: Option<DateTime<Utc>>,
    /// End of the window (RFC 3339). Defaults to now.
    pub to: Option<DateTime<Utc>>,
    /// Resolution in seconds. Defaults to 60.
    pub step: Option<u32>,
}

/// Normalized, clamped window shared by both handlers.
struct ResolvedWindow {
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    step: u32,
}

fn resolve_window(
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    step: Option<u32>,
) -> Result<ResolvedWindow, StatusCode> {
    let to = to.unwrap_or_else(Utc::now);
    let from = from.unwrap_or_else(|| to - Duration::hours(TELEMETRY_DEFAULT_WINDOW_HOURS));

    if from >= to {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Clamp rather than reject: an over-long range is still a sensible request,
    // it just gets served at the longest span we're willing to scan.
    let from = from.max(to - Duration::days(TELEMETRY_MAX_WINDOW_DAYS));
    let step = step
        .unwrap_or(TELEMETRY_DEFAULT_STEP_SECONDS)
        .max(TELEMETRY_MIN_STEP_SECONDS);

    Ok(ResolvedWindow { from, to, step })
}

/// Runs a range query against VictoriaMetrics and hands the Prometheus response
/// back untouched, so callers keep working against the standard matrix shape.
async fn query_range(
    state: &State,
    promql: String,
    window: ResolvedWindow,
) -> Result<Response, StatusCode> {
    let client_config = state
        .config
        .victoria_metrics_read_client
        .as_ref()
        .ok_or(StatusCode::NOT_IMPLEMENTED)?;

    let url = format!(
        "{}/api/v1/query_range",
        client_config.url.trim_end_matches('/')
    );

    let response = client_config
        .client
        .get(&url)
        .query(&[
            ("query", promql.as_str()),
            ("start", &window.from.timestamp().to_string()),
            ("end", &window.to.timestamp().to_string()),
            ("step", &window.step.to_string()),
        ])
        .send()
        .await
        .map_err(|err| {
            error!(error = %err, "Failed to query VictoriaMetrics");
            StatusCode::BAD_GATEWAY
        })?;

    let status = response.status();
    let body = response.bytes().await.map_err(|err| {
        error!(error = %err, "Failed to read VictoriaMetrics response");
        StatusCode::BAD_GATEWAY
    })?;

    if !status.is_success() {
        warn!(status = %status, "VictoriaMetrics rejected a telemetry query");
    }

    Ok((
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
        [(header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response())
}

#[utoipa::path(
    get,
    path = "/devices/{device_id}/telemetry",
    params(
        ("device_id" = String, Path),
        TelemetryQuery
    ),
    responses(
        (status = StatusCode::OK, description = "Prometheus matrix response for the metric"),
        (status = StatusCode::BAD_REQUEST, description = "Unknown metric or invalid window"),
        (status = StatusCode::NOT_FOUND, description = "Device not found"),
        (status = StatusCode::NOT_IMPLEMENTED, description = "VictoriaMetrics reads not configured"),
        (status = StatusCode::BAD_GATEWAY, description = "VictoriaMetrics unreachable"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TELEMETRY_TAG
)]
pub async fn get_telemetry_for_device(
    Path(device_id): Path<String>,
    Query(query): Query<TelemetryQuery>,
    Extension(state): Extension<State>,
) -> Result<Response, StatusCode> {
    let window = resolve_window(query.from, query.to, query.step)?;

    if !is_safe_metric_name(&query.metric) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let serial_number = sqlx::query_scalar!(
        r#"
        SELECT serial_number FROM device
        WHERE
            CASE
                WHEN $1 ~ '^[0-9]+$' AND length($1) <= 10 THEN
                    id = $1::int4
                ELSE
                    serial_number = $1
            END
        "#,
        device_id
    )
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to resolve device {device_id}: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    // A serial already in our own device table shouldn't be able to fail this,
    // but the matcher is built by string substitution so it is checked anyway.
    if !is_safe_serial(&serial_number) {
        error!(serial_number, "Refusing to build PromQL for unsafe serial");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let promql = single_query(
        &query.metric,
        &format!(r#"serial_number="{serial_number}""#),
        query.rate,
    );

    query_range(&state, promql, window).await
}

#[utoipa::path(
    get,
    path = "/telemetry/devices",
    params(TelemetryBatchQuery),
    responses(
        (status = StatusCode::OK, description = "Prometheus matrix response, one series per serial"),
        (status = StatusCode::BAD_REQUEST, description = "Unknown metric, invalid window, or invalid serial"),
        (status = StatusCode::NOT_IMPLEMENTED, description = "VictoriaMetrics reads not configured"),
        (status = StatusCode::BAD_GATEWAY, description = "VictoriaMetrics unreachable"),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = TELEMETRY_TAG
)]
pub async fn get_telemetry_for_devices(
    Query(query): Query<TelemetryBatchQuery>,
    Extension(state): Extension<State>,
) -> Result<Response, StatusCode> {
    let window = resolve_window(query.from, query.to, query.step)?;

    let serials: Vec<&str> = query
        .serials
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if serials.is_empty() || serials.len() > TELEMETRY_MAX_SERIALS {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !serials.iter().all(|s| is_safe_serial(s)) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !is_safe_metric_name(&query.metric) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let promql = batch_query(
        &query.metric,
        &format!(r#"serial_number=~"{}""#, serials.join("|")),
        query.rate,
    );

    query_range(&state, promql, window).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_serials_that_could_escape_a_label_matcher() {
        assert!(is_safe_serial("SN123abc"));
        assert!(!is_safe_serial(""));
        assert!(!is_safe_serial(r#"a" or up{"#));
        assert!(!is_safe_serial("a|b"));
        assert!(!is_safe_serial("a.*"));
        // Serials are alphanumeric; separators are rejected rather than escaped.
        assert!(!is_safe_serial("SN-123"));
        assert!(!is_safe_serial("SN_123"));
    }

    #[test]
    fn rejects_metric_names_that_could_append_an_expression() {
        assert!(is_safe_metric_name("node_cpu_usage_percent"));
        assert!(is_safe_metric_name("_leading_underscore"));
        assert!(is_safe_metric_name("namespace:recorded:rule"));
        assert!(!is_safe_metric_name(""));
        // A metric name is not an expression: no selectors, operators, or calls.
        assert!(!is_safe_metric_name("up} or something{"));
        assert!(!is_safe_metric_name("up + up"));
        assert!(!is_safe_metric_name("rate(up[5m])"));
        assert!(!is_safe_metric_name("1_starts_with_digit"));
    }

    #[test]
    fn one_device_reduces_to_one_line() {
        // A device exporting the metric per core or per disk would otherwise
        // come back as several series the caller can't tell apart.
        assert_eq!(
            single_query("node_temp_celsius", r#"serial_number="SN1""#, false),
            r#"avg by (serial_number) (node_temp_celsius{serial_number="SN1"})"#
        );
    }

    #[test]
    fn reads_counters_as_a_rate() {
        // Rating has to happen in PromQL: it is what handles counter resets.
        assert_eq!(
            single_query("node_rx_bytes", r#"serial_number="SN1""#, true),
            r#"avg by (serial_number) (rate(node_rx_bytes{serial_number="SN1"}[5m]))"#
        );
    }

    #[test]
    fn batch_reduces_to_one_series_per_device() {
        assert_eq!(
            batch_query("node_temp", r#"serial_number=~"A|B""#, false),
            r#"max by (serial_number) (last_over_time(node_temp{serial_number=~"A|B"}[5m]))"#
        );
        assert_eq!(
            batch_query("node_rx_bytes", r#"serial_number=~"A|B""#, true),
            r#"max by (serial_number) (rate(node_rx_bytes{serial_number=~"A|B"}[5m]))"#
        );
    }

    #[test]
    fn window_defaults_to_the_last_hour_and_clamps_long_ranges() {
        let to = Utc::now();
        let resolved = resolve_window(None, Some(to), None).expect("defaults are valid");
        assert_eq!(resolved.to, to);
        assert_eq!(resolved.from, to - Duration::hours(1));
        assert_eq!(resolved.step, TELEMETRY_DEFAULT_STEP_SECONDS);

        let clamped = resolve_window(Some(to - Duration::days(365)), Some(to), Some(1))
            .expect("an over-long range is clamped, not rejected");
        assert_eq!(clamped.from, to - Duration::days(TELEMETRY_MAX_WINDOW_DAYS));
        assert_eq!(clamped.step, TELEMETRY_MIN_STEP_SECONDS);
    }

    #[test]
    fn rejects_an_inverted_window() {
        let to = Utc::now();
        assert!(resolve_window(Some(to), Some(to - Duration::hours(1)), None).is_err());
    }
}
