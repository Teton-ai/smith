use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is healthy", body = &str),
        (status = 500, description = "Service is not healthy", body = &str),
    )
)]
pub async fn check() -> Result<Response, StatusCode> {
    Ok(format!("I'm good: {}", env!("CARGO_PKG_VERSION")).into_response())
}
