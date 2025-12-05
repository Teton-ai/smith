use crate::State;
use crate::config::Config;
use crate::package::Package;
use axum::body::Body;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use axum_typed_multipart::{FieldData, TryFromMultipart, TypedMultipart};
use futures::TryStreamExt;
use s3::Bucket;
use s3::creds::Credentials;
use s3::error::S3Error;
use serde::Deserialize;
use smith::utils::schema;
use std::error::Error;
use std::io::{Cursor, Read};
use tempfile::NamedTempFile;
use tracing::{debug, error};

const PACKAGES_TAG: &str = "packages";

#[utoipa::path(
    get,
    path = "/packages",
    responses(
        (status = 200, description = "List of registered packages"),
        (status = 500, description = "Failure", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = PACKAGES_TAG
)]
pub async fn get_packages(
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Vec<schema::Package>>, StatusCode> {
    let packages = sqlx::query_as!(
        schema::Package,
        "SELECT * FROM package ORDER BY package.created_at DESC"
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get packages {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(packages))
}

#[derive(Debug, TryFromMultipart, utoipa::ToSchema)]
pub struct ReleasePackageRequest {
    #[schema(format = Binary, value_type = String)]
    #[form_data(limit = "1Gib")]
    file: FieldData<NamedTempFile>,
}

#[utoipa::path(
    put,
    path = "/packages",
    request_body(content = ReleasePackageRequest, content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Sucess releasing package"),
        (status = 500, description = "Failure", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = PACKAGES_TAG
)]
pub async fn release_package(
    Extension(state): Extension<State>,
    TypedMultipart(ReleasePackageRequest { mut file }): TypedMultipart<ReleasePackageRequest>,
) -> axum::response::Result<StatusCode, StatusCode> {
    let file_name = file.metadata.file_name.unwrap_or(String::from("data.bin"));

    let mut buf = Vec::new();
    file.contents.read_to_end(&mut buf).map_err(|err| {
        error!("error: failed to release package {:?}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut cursor = Cursor::new(&buf);
    let mut pkg = debpkg::DebPkg::parse(&mut cursor).unwrap();

    let control_tar = pkg.control().unwrap();
    let control = debpkg::Control::extract(control_tar).unwrap();
    let arch = control.get("Architecture").unwrap();
    debug!("File Name: {}", file_name);
    debug!("Package Name: {}", control.name());
    debug!("Package Version: {}", control.version());
    debug!("Package Architecture: {}", arch);

    Package::new(
        control.name(),
        control.version(),
        arch,
        &file_name,
        &buf,
        state.config,
        &state.pg_pool,
    )
    .await
    .map_err(|err| {
        error!("error: Failed to save package: {:?}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(StatusCode::OK)
}

#[utoipa::path(
    get,
    path = "/packages/{package_name}/latest",
    params(
        ("package_name" = String, Path, description = "Package name")
    ),
    responses(
        (status = 200, description = "Latest version of the package", body = Package),
        (status = 404, description = "Package not found"),
        (status = 500, description = "Failure", body = String),
    ),
    security(
        ("auth_token" = [])
    ),
    tag = PACKAGES_TAG
)]
pub async fn get_package_latest(
    Path(package_name): Path<String>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Package>, StatusCode> {
    let package = sqlx::query_as!(
        Package,
        "SELECT * FROM package WHERE name = $1 ORDER BY created_at DESC LIMIT 1",
        package_name
    )
    .fetch_optional(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get latest package {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    match package {
        Some(pkg) => Ok(Json(pkg)),
        None => Err(StatusCode::NOT_FOUND),
    }
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

#[derive(Debug, Deserialize)]
pub struct DownloadPackageQuery {
    pub name: String,
}

#[utoipa::path(
    get,
    path = "/packages/download",
    params(
        ("name" = String, Query, description = "File name of the package to download")
    ),
    responses(
        (status = 200, description = "Package data", content_type = "application/octet-stream"),
        (status = 404, description = "Package not found"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("auth_token" = [])
    ),
    tag = PACKAGES_TAG
)]
pub async fn download_package(
    Extension(state): Extension<State>,
    Query(params): Query<DownloadPackageQuery>,
) -> Result<Response, Response> {
    stream_package_from_s3(&params.name, state.config).await
}
