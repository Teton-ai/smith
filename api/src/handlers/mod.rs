pub mod packages;
pub mod tags;

use crate::State;
use crate::db::{AuthorizationError, DBHandler, DeviceWithToken};
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

        let device = DBHandler::validate_token(bearer.token(), &state.pg_pool)
            .await
            .map_err(|auth_err| match auth_err {
                AuthorizationError::UnauthorizedDevice => {
                    (StatusCode::UNAUTHORIZED,).into_response()
                }
                AuthorizationError::DatabaseError(err) => {
                    error!("Database error: {:?}", err);
                    (StatusCode::INTERNAL_SERVER_ERROR,).into_response()
                }
            })?;

        Ok(device) // Assuming `Self` can be created from a token
    }
}
