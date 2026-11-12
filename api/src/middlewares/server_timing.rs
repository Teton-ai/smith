use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use std::time::{Duration, Instant};

const SERVER_TIMING: HeaderName = HeaderName::from_static("server-timing");

/// Lets a client subtract the server's time from its own round trip.
pub async fn record(request: Request, next: Next) -> Response {
    let started = Instant::now();
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&app_timing(started.elapsed())) {
        response.headers_mut().insert(SERVER_TIMING, value);
    }
    response
}

fn app_timing(elapsed: Duration) -> String {
    format!("app;dur={:.1}", elapsed.as_secs_f64() * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, middleware, routing::get};
    use tower::ServiceExt;

    #[test]
    fn timing_has_one_decimal_in_milliseconds() {
        assert_eq!(app_timing(Duration::from_micros(12_449)), "app;dur=12.4");
        assert_eq!(app_timing(Duration::ZERO), "app;dur=0.0");
    }

    #[tokio::test]
    async fn responses_carry_the_header() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn(record));
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let header = response.headers()["server-timing"].to_str().unwrap();
        assert!(header.starts_with("app;dur="), "{header}");
    }
}
