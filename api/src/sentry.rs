use crate::config::Config;
use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use sentry::ClientInitGuard;
use tracing::{info, warn};

pub struct Sentry;

impl Sentry {
    pub fn init(config: &'static Config) -> Option<ClientInitGuard> {
        if let Some(sentry_url) = &config.sentry_url {
            return Some(sentry::init((
                sentry_url.as_str(),
                sentry::ClientOptions {
                    release: sentry::release_name!(),
                    send_default_pii: true,
                    traces_sample_rate: 1.0,
                    ..Default::default()
                },
            )));
        }
        warn!("Sentry integration disabled: no sentry_url configured");
        None
    }

    pub async fn capture_errors_middleware(request: Request, next: Next) -> Response {
        let method = request.method().clone();
        let uri = request.uri().clone();

        let response = next.run(request).await;

        if response.status() == StatusCode::INTERNAL_SERVER_ERROR {
            sentry::capture_message(
                &format!("Internal Server Error: {} {}", method, uri),
                sentry::Level::Error,
            );
        }

        response
    }
}
