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

        // Try the token as a device JWT first; if it isn't a valid one, fall
        // back to the opaque-token lookup below.
        if let Ok(claims) = state.device_jwt_signer.verify(token) {
            let id = claims
                .sub
                .parse::<i32>()
                .map_err(|_| StatusCode::UNAUTHORIZED.into_response())?;

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
