use crate::State;
use crate::config::Config;
use crate::package::Package;
use axum::body::Body;
use axum::{
    Extension, Json,
    extract::Path,
    response::{IntoResponse, Response},
};
use axum::{http::StatusCode, response::Result};
use futures::TryStreamExt;
use s3::Bucket;
use s3::creds::Credentials;
use s3::error::S3Error;
use std::error::Error;
use tracing::error;

// TODO: I believe this whole stuff is legacy and not documented, check and delete

pub async fn get_package_by_id(
    Path(package_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Response, Response> {
    get_package_info_by_id(Path(package_id), Extension(state))
        .await
        .map(|json| json.into_response())
}

async fn get_package_info_by_id(
    Path(package_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<Json<Package>, Response> {
    let package = sqlx::query_as!(
        Package,
        "SELECT * FROM package WHERE package.id = $1",
        package_id
    )
    .fetch_one(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get package {err}");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    Ok(Json(package))
}

pub async fn delete_package_by_id(
    Path(package_id): Path<i32>,
    Extension(state): Extension<State>,
) -> Result<StatusCode, StatusCode> {
    Package::delete(&package_id, state.config, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to delete the package {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// Streams a package from S3 by its file name.
/// Returns a streaming response with the package data.
pub async fn stream_package_from_s3(
    file_name: &str,
    config: &Config,
) -> Result<Response, Response> {
    let bucket = Bucket::new(
        &config.packages_bucket_name,
        config.aws_region.parse().map_err(|e| {
            error!("Failed to parse AWS region: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        })?,
        Credentials::default().map_err(|e| {
            error!("Failed to get AWS credentials: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        })?,
    )
    .map_err(|e| {
        error!("Failed to create S3 bucket: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;

    let stream = bucket.get_object_stream(file_name).await.map_err(|e| {
        error!("Failed to get package from S3: {:?}", e);
        match e {
            S3Error::HttpFailWithBody(404, _) => (
                StatusCode::NOT_FOUND,
                format!("{} package not found", file_name),
            )
                .into_response(),
            _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    })?;

    let adapted_stream = stream
        .bytes
        .map_ok(|data| data)
        .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync + 'static>);

    let body = Body::from_stream(adapted_stream);

    Ok(Response::new(body).into_response())
}
