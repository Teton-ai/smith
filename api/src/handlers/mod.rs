pub mod auth;
pub mod commands;
pub mod devices;
pub mod distributions;
pub mod download;
pub mod events;
pub mod health;
pub mod home;
pub mod ip_address;
pub mod network;
pub mod packages;
pub mod releases;
pub mod tags;
pub mod upload;

use crate::State;
use crate::db::{AuthorizationError, DBHandler, DeviceWithToken};
use axum::body::Body;
use axum::{
    Json, async_trait,
    extract::{Extension, FromRequestParts, Path, Query},
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use futures::TryStreamExt;
use s3::error::S3Error;
use s3::{Bucket, creds::Credentials};
use serde::Deserialize;
use smith::utils::schema::Package;
use std::error::Error;
use tracing::{debug, error};

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

#[derive(Deserialize, Debug)]
pub struct FetchPackageQuery {
    name: String,
}

#[tracing::instrument]
pub async fn fetch_package(
    //device: DeviceWithToken,
    Extension(state): Extension<State>,
    params: Query<FetchPackageQuery>,
) -> Result<Response, Response> {
    let deb_package_name = &params.name;
    debug!("Fetching package {}", &deb_package_name);
    let bucket = Bucket::new(
        &state.config.packages_bucket_name,
        state
            .config
            .aws_region
            .parse()
            .expect("error: failed to parse AWS region"),
        Credentials::default().unwrap(),
    )
    .map_err(|e| {
        error!("{:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    let stream = bucket
        .get_object_stream(&deb_package_name)
        .await
        .map_err(|e| {
            error!("{:?}", e);
            match e {
                S3Error::HttpFailWithBody(404, _) => (
                    StatusCode::NOT_FOUND,
                    format!("{} package not found", &deb_package_name),
                )
                    .into_response(),

                _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            }
        })?;

    let adapted_stream = stream
        .bytes
        .map_ok(|data| data)
        .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync + 'static>);

    let stream = Body::from_stream(adapted_stream);

    Ok(Response::new(stream).into_response())
}

#[tracing::instrument]
pub async fn list_release_packages(
    //device: DeviceWithToken,
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<Vec<Package>>, Json<Vec<Package>>> {
    let packages = sqlx::query_as!(
        Package,
        "
        SELECT package.*
        FROM release_packages
        JOIN package ON package.id = release_packages.package_id
        WHERE release_packages.release_id = $1
        ",
        release_id
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get packages from distribution name {err}");
        Json(vec![])
    })?;

    Ok(Json(packages))
}
