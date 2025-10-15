mod audience;
use crate::{State, user::CurrentUser};
use audience::Audience;
use axum::{
    Extension,
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use jwks_client_rs::{JwksClient, source::WebSource};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    aud: Audience,
    exp: u64,
    iat: u64,
    iss: String,
    sub: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Auth0UserInfo {
    pub sub: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub picture: Option<String>,
    pub email_verified: Option<bool>,
}

pub async fn check(
    Extension(state): Extension<State>,
    headers: HeaderMap,
    // you can also add more extractors here but the last
    // extractor must implement `FromRequest` which
    // `Request` does
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = headers
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();

    let url_string =
        std::env::var("AUTH0_ISSUER").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let url = Url::parse(&url_string)
        .unwrap()
        .join(".well-known/jwks.json")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let source: WebSource = WebSource::builder()
        .build(url)
        .expect("Failed to build WebSource");

    let client: JwksClient<WebSource> = JwksClient::builder()
        .time_to_live(Duration::from_secs(60))
        .build(source);

    // Step 3: Verify the token.
    let audience = vec![
        std::env::var("AUTH0_AUDIENCE").expect("error: failed to access AUTH0_AUDIENCE env var"),
    ];

    let claims = client
        .decode::<Claims>(token, &audience)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let pool = state.pg_pool.clone();

    let authorization = state.authorization.clone();

    let current_user = match CurrentUser::id(&pool, &claims.sub).await {
        Ok(user_id) => CurrentUser::build(&pool, &authorization, user_id).await,
        Err(sqlx::Error::RowNotFound) => {
            info!("Creating user for sub={}", claims.sub);
            let issuer = Url::parse(
                &std::env::var("AUTH0_ISSUER").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let userinfo_url = issuer
                .join("userinfo")
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let client_http = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let userinfo: Auth0UserInfo = client_http
                .get(userinfo_url)
                .bearer_auth(token)
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|_| StatusCode::UNAUTHORIZED)?
                .json()
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            info!(
                "Provisioning user; email_present={}",
                userinfo.email.is_some()
            );
            let user_id = CurrentUser::create(&pool, &claims.sub, Some(userinfo))
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            CurrentUser::build(&pool, &authorization, user_id).await
        }
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
    .map_err(|_| StatusCode::UNAUTHORIZED)?;

    request.extensions_mut().insert(current_user);

    let response = next.run(request).await;
    Ok(response)
}
