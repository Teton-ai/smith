pub mod packages;
pub mod tags;

use crate::db::DeviceWithToken;
use crate::{State, device::get_device_from_token};
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

// TODO: DEPRECATED: This FromRequestParts implementation is deprecated.
// Use middleware::from_fn(device::Device::middleware) instead for device authentication.
// https://docs.rs/axum/latest/axum/extract/index.html#accessing-other-extractors-in-fromrequest-or-fromrequestparts-implementations
#[async_trait]
impl<S> FromRequestParts<S> for DeviceWithToken
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Extract the authorization token.
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
                .await
                .map_err(|_| (StatusCode::UNAUTHORIZED,).into_response())?;

        use axum::RequestPartsExt;
        let Extension(state) = parts
            .extract::<Extension<State>>()
            .await
            .map_err(|err| err.into_response())?;

        let device = get_device_from_token(bearer.token().to_string(), &state.pg_pool)
            .await
            .map_err(|err| {
                error!("Database error: {:?}", err);
                (StatusCode::INTERNAL_SERVER_ERROR,).into_response()
            })?
            .ok_or_else(|| (StatusCode::UNAUTHORIZED,).into_response())?;

        Ok(DeviceWithToken {
            id: device.id,
            serial_number: device.serial_number,
        })
    }
}
