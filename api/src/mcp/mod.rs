//! Remote MCP server, so agents can drive Smith with the caller's own identity.
//!
//! The API is only an OAuth resource server here: clients discover Auth0 from
//! the protected-resource metadata, log the user in there, and present the same
//! kind of token the dashboard uses (audience `AUTH0_AUDIENCE`).

use crate::State;
use crate::config::McpConfig;
use crate::middlewares::authentication::{authenticate, bearer_token};
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
};
use serde_json::json;
use std::sync::Arc;
use tracing::error;

mod tools;

/// Routes for `/mcp` and its OAuth metadata; empty when MCP is not configured.
pub fn router(state: State) -> Router {
    let Some(mcp) = state.config.mcp.as_ref() else {
        return Router::new();
    };

    let config = StreamableHttpServerConfig::default()
        .with_allowed_hosts(mcp.allowed_hosts.clone())
        // The API runs as several instances behind a load balancer, so no
        // request may rely on session state held by a single instance.
        .with_legacy_session_mode(false)
        .with_json_response(true);

    let service = StreamableHttpService::new(
        move || Ok(tools::SmithMcp::new(state.clone())),
        Arc::new(NeverSessionManager::default()),
        config,
    );

    Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn(require_token))
        // Metadata has to stay public: it is how a client learns where to log in.
        .route(
            "/.well-known/oauth-protected-resource",
            get(protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(protected_resource_metadata),
        )
}

async fn require_token(
    Extension(state): Extension<State>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(mcp) = state.config.mcp.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let token = bearer_token(request.headers()).to_owned();
    match authenticate(&state, &token).await {
        Ok((current_user, _holder)) => {
            request.extensions_mut().insert(current_user);
            next.run(request).await
        }
        Err(StatusCode::UNAUTHORIZED) => unauthorized(mcp),
        Err(status) => status.into_response(),
    }
}

/// The `WWW-Authenticate` challenge is what starts the client's OAuth flow.
fn unauthorized(mcp: &McpConfig) -> Response {
    let challenge = format!("Bearer resource_metadata=\"{}\"", mcp.resource_metadata_url);
    match HeaderValue::from_str(&challenge) {
        Ok(value) => (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, value)],
        )
            .into_response(),
        Err(err) => {
            error!("Invalid MCP WWW-Authenticate challenge: {err}");
            StatusCode::UNAUTHORIZED.into_response()
        }
    }
}

/// OAuth 2.0 Protected Resource Metadata (RFC 9728).
async fn protected_resource_metadata(Extension(state): Extension<State>) -> Response {
    let Some(mcp) = state.config.mcp.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };

    Json(json!({
        "resource": mcp.resource_url,
        "authorization_servers": [state.config.auth0_issuer],
        "bearer_methods_supported": ["header"],
        "resource_name": "Smith",
    }))
    .into_response()
}
