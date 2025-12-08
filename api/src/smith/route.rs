use crate::asset::Asset;
use crate::db::{DBHandler, DeviceWithToken};
use crate::device::{
    RegistrationError, get_target_release, save_last_ping_with_ip, save_release_id,
};
use crate::ip_address::extract_client_ip;
use crate::{State, storage};
use axum::body::Body;
use axum::extract::{ConnectInfo, Multipart, Path, Query};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use smith::utils::schema::{
    DeviceRegistration, DeviceRegistrationResponse, HomePost, HomePostResponse, Package,
};
use std::net::SocketAddr;
use std::time::SystemTime;
use tracing::{debug, error, info};
use utoipa::{IntoParams, ToSchema};

#[utoipa::path(
  post,
  path = "/smith/register",
  responses(
        (status = 200, description = "Device registration successful"),
        (status = 403, description = "Device not approved"),
        (status = 409, description = "Device already has token"),
        (status = 500, description = "Internal server error")
  )
)]
pub async fn register_device(
    Extension(state): Extension<State>,
    Json(payload): Json<DeviceRegistration>,
) -> (StatusCode, Json<DeviceRegistrationResponse>) {
    let token = crate::device::register_device(payload, &state.pg_pool, state.config).await;

    match token {
        Ok(token) => (StatusCode::OK, Json(token)),
        Err(e) => {
            info!("No token available for device: {:?}", e);
            let status_code = match e {
                RegistrationError::NotNullTokenError => StatusCode::CONFLICT,
                RegistrationError::NotApprovedDevice => StatusCode::FORBIDDEN,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };

            (status_code, Json(DeviceRegistrationResponse::default()))
        }
    }
}

#[utoipa::path(
  post,
  path = "/smith/home",
  responses(
        (status = 200, description = "Device home response")
  ),
  security(
        ("device_token" = [])
  ),
)]
pub async fn home(
    headers: HeaderMap,
    device: DeviceWithToken,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Extension(state): Extension<State>,
    Json(payload): Json<HomePost>,
) -> (StatusCode, Json<HomePostResponse>) {
    let release_id = payload.release_id;
    DBHandler::save_responses(&device, payload, &state.pg_pool)
        .await
        .unwrap_or_else(|err| {
            error!("Error saving responses: {:?}", err);
        });

    let response = HomePostResponse {
        timestamp: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default(),
        commands: DBHandler::get_commands(&device, &state.pg_pool).await,
        target_release_id: get_target_release(&device, &state.pg_pool).await,
    };

    let client_ip = Some(extract_client_ip(&headers, addr));
    tokio::spawn(async move {
        save_release_id(&device, release_id, &state.pg_pool)
            .await
            .unwrap_or_else(|err| {
                error!("Error saving release_id: {:?}", err);
            });
        save_last_ping_with_ip(&device, client_ip, &state.pg_pool, state.config)
            .await
            .unwrap_or_else(|err| {
                error!("Error saving last ping with IP: {:?}", err);
            });
    });

    (StatusCode::OK, Json(response))
}

#[derive(Deserialize, Debug, IntoParams)]
pub struct DownloadParams {
    path: String,
}

#[utoipa::path(
  get,
  path = "/smith/download",
  params(
        DownloadParams
  ),
  responses(
        (status = 200, description = "File download successful", content_type = "application/octet-stream"),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal server error")
  ),
  security(
        ("device_token" = [])
  ),
)]
pub async fn download_file(
    _device: DeviceWithToken,
    Query(params): Query<DownloadParams>,
    Extension(state): Extension<State>,
) -> Result<axum::response::Response<Body>, StatusCode> {
    let file_path = &params.path;

    // Strip leading slash if present
    let path = file_path.strip_prefix('/').unwrap_or(file_path.as_str());
    // Split into bucket, directory path, and file name
    let (bucket, dir_path, file_name) = if let Some(first_idx) = path.find('/') {
        let bucket = &path[..first_idx];
        let remaining_path = &path[first_idx + 1..];

        if let Some(last_idx) = remaining_path.rfind('/') {
            let dir_path = &remaining_path[..last_idx];
            let file_name = &remaining_path[last_idx + 1..];
            (bucket, dir_path, file_name)
        } else {
            (bucket, "", remaining_path)
        }
    } else {
        (path, "", "")
    };

    if file_name.is_empty() || bucket.is_empty() {
        error!("File name is empty in the requested path: {}", path);
        return Err(StatusCode::BAD_REQUEST);
    }

    // Add more buckets here if needed
    let response = match bucket.to_lowercase().as_str() {
        // "packages" => &state.config.packages_bucket_name,
        // "assets" => &state.config.assets_bucket_name,
        "packages" => storage::Storage::download_package_from_cdn(
            &state.config.packages_bucket_name,
            Some(dir_path),
            file_name,
            &state.config.cloudfront.package_domain_name,
            &state.config.cloudfront.package_key_pair_id,
            &state.config.cloudfront.package_private_key,
        )
        .await
        .map_err(|err| {
            error!("Failed to get signed link from CDN {:?}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        })?,
        // TODO: need to implement asset downloading or put
        // ota stuff in packages bucket
        //
        // "assets" => storage::Storage::download_package_from_cdn(
        //     &state.config.packages_bucket_name,
        //     Some(dir_path),
        //     file_name,
        //     &state.config.cloudfront.package_domain_name,
        //     &state.config.cloudfront.package_key_pair_id,
        //     &state.config.cloudfront.package_private_key,
        // )
        // .await
        // .map_err(|err| {
        //     error!("Failed to get signed link from CDN {:?}", err);
        //     StatusCode::INTERNAL_SERVER_ERROR
        // })?,
        _ => {
            error!("Invalid bucket name requested: {}", bucket);
            return Err(StatusCode::BAD_REQUEST);
        }
    };

    Ok(response)
}

#[derive(Deserialize, Debug)]
pub struct FetchPackageQuery {
    name: String,
}

#[utoipa::path(
  get,
  path = "/smith/package",
  params(
        ("name" = String, Query, description = "Package name to fetch")
  ),
  responses(
        (status = 200, description = "Package data", content_type = "application/octet-stream"),
        (status = 404, description = "Package not found"),
        (status = 500, description = "Internal server error")
  ),
  security(
        ("device_token" = [])
  ),
)]
pub async fn fetch_package(
    _device: DeviceWithToken,
    Extension(state): Extension<State>,
    params: Query<FetchPackageQuery>,
) -> Result<Response, Response> {
    debug!("Fetching package {}", &params.name);
    crate::package::route::stream_package_from_s3(&params.name, state.config).await
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UploadResult {
    pub url: String,
}

// TODO: Change to streaming, so we are not saving in memory
#[utoipa::path(
  post,
  path = "/smith/upload",
  responses(
        (status = 200, description = "File uploaded successfully", body = UploadResult),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal server error")
  ),
  security(
        ("device_token" = [])
  ),
)]
pub async fn upload_file(
    _device: DeviceWithToken,
    path: Option<Path<String>>,
    Extension(state): Extension<State>,
    mut multipart: Multipart,
) -> Result<Json<UploadResult>, StatusCode> {
    let mut file_name = String::new();
    if let Some(prefix) = path {
        file_name.push_str(&prefix.0);
        file_name.push('/');
    }

    let mut file_data = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .expect("error: failed to get next multipart field")
    {
        if let Some(local_file_name) = field.file_name().map(|s| s.to_string()) {
            file_name.push_str(&local_file_name);
        }
        match field.bytes().await {
            Ok(bytes) => file_data.extend(bytes.clone()),
            _ => return Err(StatusCode::BAD_REQUEST),
        };
    }

    if file_name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    Asset::new(&file_name, &file_data, state.config)
        .await
        .map_err(|err| {
            error!("{:?}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(UploadResult {
        url: format!("s3://{}/{}", &state.config.assets_bucket_name, &file_name),
    }))
}

#[utoipa::path(
  get,
  path = "/smith/releases/{release_id}/packages",
  params(
        ("release_id" = i32, Path, description = "Release ID")
  ),
  responses(
        (status = 200, description = "List of packages for the release"),
        (status = 500, description = "Internal server error")
  ),
  security(
        ("device_token" = [])
  ),
)]
pub async fn list_release_packages(
    _device: DeviceWithToken,
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

#[utoipa::path(
    get,
    path = "/smith/network/test-file",
    responses(
        (status = 200, description = "Returns a 20MB test file for network speed testing"),
    )
)]
pub async fn test_file() -> Response<Body> {
    const FILE_SIZE: usize = 20 * 1024 * 1024; // 20MB
    let data = vec![0u8; FILE_SIZE];

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/octet-stream")
        .header("Content-Length", FILE_SIZE.to_string())
        .body(Body::from(data))
        .unwrap()
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UploadTestResult {
    pub bytes_received: usize,
}

#[utoipa::path(
    post,
    path = "/smith/network/test-upload",
    responses(
        (status = 200, description = "Receives upload data for network speed testing", body = UploadTestResult),
    )
)]
pub async fn test_upload(body: axum::body::Bytes) -> Json<UploadTestResult> {
    Json(UploadTestResult {
        bytes_received: body.len(),
    })
}
