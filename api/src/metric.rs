use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::IntoResponse;
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

pub fn setup_metrics_recorder() -> PrometheusHandle {
    // Metrics
    const EXPONENTIAL_SECONDS: &[f64] = &[
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
    ];

    // 64 KiB to 1 GiB, x4 steps.
    const UPLOAD_BYTES: &[f64] = &[
        65_536.0,
        262_144.0,
        1_048_576.0,
        4_194_304.0,
        16_777_216.0,
        67_108_864.0,
        268_435_456.0,
        1_073_741_824.0,
    ];

    PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("http_requests_duration_seconds".to_string()),
            EXPONENTIAL_SECONDS,
        )
        .expect("error: failed to build prometheus recorder")
        .set_buckets_for_metric(
            Matcher::Full("smith_upload_bytes".to_string()),
            UPLOAD_BYTES,
        )
        .expect("error: failed to build prometheus recorder")
        .install_recorder()
        .expect("error: failed to install prometheus recorder")
}

pub async fn track_metrics(req: Request, next: Next) -> impl IntoResponse {
    let start = Instant::now();
    let path = if let Some(matched_path) = req.extensions().get::<MatchedPath>() {
        matched_path.as_str().to_owned()
    } else {
        req.uri().path().to_owned()
    };
    let method = req.method().clone();

    let response = next.run(req).await;

    let latency = start.elapsed().as_secs_f64();
    let status = response.status().as_u16().to_string();

    let labels = [
        ("method", method.to_string()),
        ("path", path),
        ("status", status),
    ];

    metrics::increment_counter!("http_requests_total", &labels);
    metrics::histogram!("http_requests_duration_seconds", latency, &labels);

    response
}

/// Reports the per-minute peak of upload permits in use, as a CloudWatch EMF line and a gauge.
pub fn spawn_upload_permits_reporter(permits: Arc<Semaphore>, max: usize) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            let mut peak = 0;
            for _ in 0..60 {
                tick.tick().await;
                peak = peak.max(max.saturating_sub(permits.available_permits()));
            }
            metrics::gauge!("smith_upload_permits_in_use", peak as f64);
            // Raw stdout, not a tracing event: CloudWatch only extracts EMF from a top-level JSON line.
            println!(
                "{}",
                upload_permits_emf(peak, chrono::Utc::now().timestamp_millis())
            );
        }
    });
}

fn upload_permits_emf(in_use: usize, timestamp_ms: i64) -> Value {
    json!({
        "_aws": {
            "Timestamp": timestamp_ms,
            "CloudWatchMetrics": [{
                "Namespace": "SmithApi",
                "Dimensions": [["Service"]],
                "Metrics": [{ "Name": "UploadPermitsInUse", "Unit": "Count" }],
            }],
        },
        "Service": "smith-api",
        "UploadPermitsInUse": in_use,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_permits_emf_matches_cloudwatch_contract() {
        let line = upload_permits_emf(7, 1_700_000_000_000);
        let directive = &line["_aws"]["CloudWatchMetrics"][0];
        assert_eq!(directive["Namespace"], "SmithApi");
        assert_eq!(directive["Dimensions"], json!([["Service"]]));
        assert_eq!(
            directive["Metrics"],
            json!([{ "Name": "UploadPermitsInUse", "Unit": "Count" }])
        );
        assert_eq!(line["Service"], "smith-api");
        assert_eq!(line["UploadPermitsInUse"], 7);
        assert_eq!(line["_aws"]["Timestamp"], 1_700_000_000_000_i64);
    }
}
