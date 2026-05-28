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

/// Heuristic: a JWT has exactly two dots and base64url segments.
/// Opaque device tokens are random hex/base64 without dots.
fn looks_like_jwt(token: &str) -> bool {
    token.bytes().filter(|&b| b == b'.').count() == 2
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthedDevice
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
                .await
                .map_err(|_| StatusCode::UNAUTHORIZED.into_response())?;

        use axum::RequestPartsExt;
        let Extension(state) = parts
            .extract::<Extension<State>>()
            .await
            .map_err(|err| err.into_response())?;

        let token = bearer.token();

        if looks_like_jwt(token)
            && let Ok(claims) = state.device_jwt_signer.verify(token)
            && let Ok(id) = claims.sub.parse::<i32>()
        {
            return Ok(AuthedDevice {
                id,
                serial_number: claims.serial,
            });
        }

        let device = sqlx::query!(
            r#"
            SELECT
                id,
                serial_number
            FROM device
            WHERE
                token IS NOT NULL AND
                token = $1
            "#,
            token
        )
        .fetch_optional(&state.pg_pool)
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
}
