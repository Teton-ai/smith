pub mod packages;

use crate::State;
use axum::{
    async_trait,
    extract::{Extension, FromRequestParts},
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use tracing::error;

#[derive(Debug)]
pub struct AuthedDevice {
    pub id: i32,
    pub serial_number: String,
}

/// A device authenticated by its long-lived opaque token only, never a JWT.
/// Used to mint JWTs, so a JWT cannot be renewed with itself and outlive the
/// token it came from.
#[derive(Debug)]
pub struct OpaqueAuthedDevice(pub AuthedDevice);

async fn bearer_and_state<S>(parts: &mut Parts, state: &S) -> Result<(String, State), Response>
where
    S: Send + Sync,
{
    let TypedHeader(Authorization(bearer)) =
        TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED.into_response())?;

    use axum::RequestPartsExt;
    let Extension(state) = parts
        .extract::<Extension<State>>()
        .await
        .map_err(|err| err.into_response())?;

    Ok((bearer.token().to_string(), state))
}

/// Only approved, unarchived devices may authenticate with their token.
pub(crate) async fn device_by_opaque_token(
    pool: &sqlx::PgPool,
    token: &str,
) -> Result<AuthedDevice, Response> {
    let device = sqlx::query!(
        r#"
        SELECT
            id,
            serial_number
        FROM device
        WHERE
            token IS NOT NULL AND
            token = $1 AND
            approved AND
            NOT archived
        "#,
        token
    )
    .fetch_optional(pool)
    .await
    .map_err(|err| {
        error!("Database error: {:?}", err);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?
    .ok_or_else(|| StatusCode::UNAUTHORIZED.into_response())?;

    Ok(AuthedDevice {
        id: device.id,
        serial_number: device.serial_number,
    })
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthedDevice
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let (token, state) = bearer_and_state(parts, state).await?;

        // Try the token as a device JWT first; if it isn't a valid one, fall
        // back to the opaque-token lookup below.
        if let Ok(claims) = state.device_jwt_signer.verify(&token) {
            let id = claims
                .sub
                .parse::<i32>()
                .map_err(|_| StatusCode::UNAUTHORIZED.into_response())?;

            return Ok(AuthedDevice {
                id,
                serial_number: claims.serial,
            });
        }

        device_by_opaque_token(&state.pg_pool, &token).await
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for OpaqueAuthedDevice
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let (token, state) = bearer_and_state(parts, state).await?;
        device_by_opaque_token(&state.pg_pool, &token)
            .await
            .map(OpaqueAuthedDevice)
    }
}
