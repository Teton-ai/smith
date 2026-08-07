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
    let body_bytes = to_bytes(body, usize::MAX).await.map_err(|err| {
        error!("Failed to read body bytes: {}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

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

/// Metric names callers may ask for. PromQL itself never crosses the wire — an
/// unknown name fails deserialization and the request is rejected, so a caller
/// can't reach series the registry doesn't expose.
#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryMetric {
    Cpu,
    Memory,
    Temperature,
    DiskTemperature,
    DiskFree,
    NetworkRx,
    GpuLoad,
    FanSpeed,
}

impl TelemetryMetric {
    /// One device: `$SEL` becomes an equality matcher on the serial.
    fn single_template(self) -> &'static str {
        match self {
            Self::Cpu => "avg by (serial_number) (plex_system_cpu_usage_percent{$SEL})",
            Self::Memory => "plex_system_memory_usage_bytes{$SEL} / 1024 / 1024",
            Self::Temperature => "plex_system_temperature_celsius{$SEL}",
            // Some NVMe report SMART temp in deci-Celsius (371 = 37.1C) and some
            // in plain Celsius. Real disks top out near 80C, so >= 150 can only
            // be deci-Celsius — dividing unconditionally would turn a genuinely
            // dangerous 102C disk into a healthy-looking 10.2C. `* 1` on the
            // second branch is not a no-op: arithmetic drops `__name__`, and
            // without it a disk crossing the threshold mid-window comes back as
            // two series with mismatched labels instead of one.
            Self::DiskTemperature => {
                "(plex_system_disk_temperature_celsius{$SEL} >= 150) / 10 \
                 or (plex_system_disk_temperature_celsius{$SEL} < 150) * 1"
            }
            Self::DiskFree => "plex_system_disk_available_space_bytes{$SEL} / 1024 / 1024 / 1024",
            Self::NetworkRx => "rate(plex_system_network_rx_bytes{$SEL}[5m])",
            Self::GpuLoad => "avg by (serial_number) (plex_system_gpu_load_percent{$SEL})",
            Self::FanSpeed => "avg by (serial_number) (plex_system_fan_speed_percent{$SEL})",
        }
    }

    /// Many devices: `$SEL` becomes a regex matcher, and every form reduces to
    /// one series per `serial_number`. `last_over_time` carries the most recent
    /// sample forward so a device reporting on a slow interval still lands in
    /// the result instead of dropping out between scrapes.
    fn batch_template(self) -> &'static str {
        match self {
            Self::Cpu => {
                "max by (serial_number) (last_over_time(plex_system_cpu_usage_percent{$SEL}[5m]))"
            }
            Self::Memory => {
                "max by (serial_number) (last_over_time(plex_system_memory_usage_bytes{$SEL}[5m])) \
                 / 1024 / 1024"
            }
            Self::Temperature => {
                "max by (serial_number) (last_over_time(plex_system_temperature_celsius{$SEL}[5m]))"
            }
            Self::DiskTemperature => {
                "(max by (serial_number) (last_over_time(plex_system_disk_temperature_celsius{$SEL}[5m])) >= 150) / 10 \
                 or (max by (serial_number) (last_over_time(plex_system_disk_temperature_celsius{$SEL}[5m])) < 150) * 1"
            }
            Self::DiskFree => {
                "max by (serial_number) (last_over_time(plex_system_disk_available_space_bytes{$SEL}[5m])) \
                 / 1024 / 1024 / 1024"
            }
            Self::NetworkRx => {
                "max by (serial_number) (rate(plex_system_network_rx_bytes{$SEL}[5m]))"
            }
            Self::GpuLoad => {
                "max by (serial_number) (last_over_time(plex_system_gpu_load_percent{$SEL}[5m]))"
            }
            Self::FanSpeed => {
                "max by (serial_number) (last_over_time(plex_system_fan_speed_percent{$SEL}[5m]))"
            }
        }
    }
}

/// Serials reach PromQL as label-matcher literals, so anything outside this set
/// is rejected rather than escaped — a `"` or `}` would otherwise let a caller
/// rewrite the query the registry built.
fn is_safe_serial(serial: &str) -> bool {
    !serial.is_empty() && serial.chars().all(|c| c.is_ascii_alphanumeric())
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TelemetryQuery {
    /// Metric to read, e.g. `cpu`, `temperature`, `disk_temperature`.
    pub metric: TelemetryMetric,
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
    /// Metric to read, e.g. `cpu`, `temperature`, `disk_temperature`.
    pub metric: TelemetryMetric,
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

    let promql = query
        .metric
        .single_template()
        .replace("$SEL", &format!(r#"serial_number="{serial_number}""#));

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

    let promql = query.metric.batch_template().replace(
        "$SEL",
        &format!(r#"serial_number=~"{}""#, serials.join("|")),
    );

    query_range(&state, promql, window).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_METRICS: [TelemetryMetric; 8] = [
        TelemetryMetric::Cpu,
        TelemetryMetric::Memory,
        TelemetryMetric::Temperature,
        TelemetryMetric::DiskTemperature,
        TelemetryMetric::DiskFree,
        TelemetryMetric::NetworkRx,
        TelemetryMetric::GpuLoad,
        TelemetryMetric::FanSpeed,
    ];

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
    fn every_metric_template_carries_the_selector_placeholder() {
        for metric in ALL_METRICS {
            assert!(metric.single_template().contains("$SEL"), "{metric:?}");
            assert!(metric.batch_template().contains("$SEL"), "{metric:?}");
        }
    }

    #[test]
    fn batch_templates_reduce_to_one_series_per_device() {
        for metric in ALL_METRICS {
            assert!(
                metric.batch_template().contains("by (serial_number)"),
                "{metric:?}"
            );
        }
    }

    #[test]
    fn disk_temperature_only_divides_deci_celsius_readings() {
        // A 102C disk is a real emergency; it must not be reported as 10.2C.
        for promql in [
            TelemetryMetric::DiskTemperature.single_template(),
            TelemetryMetric::DiskTemperature.batch_template(),
        ] {
            assert!(promql.contains(">= 150"));
            assert!(promql.contains("< 150"));
            assert!(!promql.contains("celsius{$SEL} / 10"));
        }
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
