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

    // Use the shared JwksClient from state
    let audience = vec![state.config.auth0_audience.clone()];

    let claims = state
        .jwks_client
        .decode::<Claims>(token, &audience)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let pool = state.pg_pool.clone();

    let authorization = state.authorization.clone();

    // Check if user exists and has email populated
    let existing_user = match CurrentUser::lookup(&pool, &claims.sub).await {
        Ok((user_id, has_email)) => Some((user_id, has_email)),
        Err(sqlx::Error::RowNotFound) => None,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let needs_userinfo = existing_user
        .map(|(_, has_email)| !has_email)
        .unwrap_or(true);

    // Fetch userinfo from Auth0 if user is new or missing email
    let userinfo = if needs_userinfo {
        let issuer = Url::parse(&state.config.auth0_issuer)
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
        Some(userinfo)
    } else {
        None
    };

    // Create or update user as needed
    let user_id = match existing_user {
        Some((user_id, has_email)) => {
            // User exists, update email if missing and we have it
            if !has_email {
                if let Some(ref info) = userinfo {
                    if let Some(ref email) = info.email {
                        info!("Updating email for user_id={}", user_id);
                        CurrentUser::update_email(&pool, user_id, email)
                            .await
                            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                    }
                }
            }
            user_id
        }
        None => {
            info!("Creating user for sub={}", claims.sub);
            CurrentUser::create(&pool, &claims.sub, userinfo)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        }
    };

    let current_user = CurrentUser::build(&pool, &authorization, user_id)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    request.extensions_mut().insert(current_user);

    let response = next.run(request).await;
    Ok(response)
}
