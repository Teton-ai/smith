use crate::handlers::DeviceWithToken;
use crate::{State, storage};
use axum::Extension;
use axum::body::Body;
use axum::extract::Path;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::error;

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadResult {
    pub url: String,
}

#[utoipa::path(
    get,
    path = "/smith/download/{path}",
    params(
        ("path" = String, Path, description = "File path to download")
    ),
    responses(
        (status = 200, description = "File download successful", content_type = "application/octet-stream"),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal server error")
    ),
    security(("Access Token" = []))
)]
#[tracing::instrument]
pub async fn download_file(
    _device: DeviceWithToken,
    path: Option<Path<String>>,
    Extension(state): Extension<State>,
) -> Result<axum::response::Response<Body>, StatusCode> {
    // Get file path from request
    let file_path = match path {
        Some(p) => p.0,
        None => return Err(StatusCode::BAD_REQUEST),
    };

    // Split into directory path and file name
    let (dir_path, file_name) = if let Some(idx) = file_path.rfind('/') {
        (&file_path[..idx], &file_path[idx + 1..])
    } else {
        ("", file_path.as_str())
    };

    // Get a signed link to the s3 file
    let response = storage::Storage::download_from_s3(
        &state.config.assets_bucket_name,
        Some(dir_path),
        file_name,
    )
    .await
    .map_err(|err| {
        error!("Failed to get signed link from S3 {:?}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(response)
}
