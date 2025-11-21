use crate::asset::Asset;
use crate::db::{DBHandler, DeviceWithToken};
use crate::device::{Device, RegistrationError};
use crate::ip_address::IpAddressInfo;
use crate::{State, storage};
use axum::body::Body;
use axum::extract::{ConnectInfo, Multipart, Path, Query};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use futures::TryStreamExt;
use s3::Bucket;
use s3::creds::Credentials;
use s3::error::S3Error;
use serde::{Deserialize, Serialize};
use smith::utils::schema::{
    DeviceRegistration, DeviceRegistrationResponse, HomePost, HomePostResponse, Package,
};
use std::error::Error;
use std::net::SocketAddr;
use std::time::SystemTime;
use tracing::{debug, error, info};
use utoipa::ToSchema;

/// Registers a device and returns an HTTP status with a JSON registration response containing a token when successful.
///
/// On success returns HTTP 200 with a `DeviceRegistrationResponse` containing the issued token. On failure returns one of:
/// - 403 Forbidden when the device is not approved.
/// - 409 Conflict when the device already has a token.
/// - 500 Internal Server Error for other failures.
///
/// # Examples
///
/// ```rust
/// # use axum::response::Json;
/// # use axum::http::StatusCode;
/// # use axum::extract::Extension;
/// # use your_crate::{register_device, DeviceRegistration, DeviceRegistrationResponse, State};
/// # // The following is an illustrative example; constructing `State` depends on application setup.
/// # #[tokio::test]
/// # async fn example_register_device() {
/// let state = /* construct or obtain State for tests */ unimplemented!();
/// let payload = DeviceRegistration {
///     // fill required fields...
///     ..Default::default()
/// };
///
/// let (status, Json(response)): (StatusCode, Json<DeviceRegistrationResponse>) =
///     register_device(Extension(state), Json(payload)).await;
///
/// // Inspect status and response as needed
/// let _ = (status, response);
/// # }
/// ```
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
    let token = Device::register_device(payload, &state.pg_pool, state.config).await;

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

/// Handle a device "home" POST: record the device's responses, return pending commands and target release, and asynchronously update the device's saved release and last-ping info.
///
/// The handler saves the provided responses (errors are logged but do not change the HTTP response), constructs a `HomePostResponse` containing the current UNIX timestamp, any commands for the device, and the device's target release ID, and returns that response with HTTP 200. It also spawns a background task to persist the device's reported release ID and last ping (including client IP).
///
/// # Returns
///
/// A tuple with HTTP 200 status and a JSON payload containing `timestamp`, `commands`, and `target_release_id`.
///
/// # Examples
///
/// ```rust,no_run
/// // Example usage is performed by the HTTP framework; route handlers receive
/// // parameters from request context. This snippet demonstrates the expected outcome:
/// // let (status, json) = home(headers, device, ConnectInfo(addr), Extension(state), Json(payload)).await;
/// // assert_eq!(status, StatusCode::OK);
/// ```
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
        target_release_id: Device::get_target_release(&device, &state.pg_pool).await,
    };

    let client_ip = Some(IpAddressInfo::extract_client_ip(&headers, addr));
    tokio::spawn(async move {
        Device::save_release_id(&device, release_id, &state.pg_pool)
            .await
            .unwrap_or_else(|err| {
                error!("Error saving release_id: {:?}", err);
            });
        Device::save_last_ping_with_ip(&device, client_ip, &state.pg_pool, state.config)
            .await
            .unwrap_or_else(|err| {
                error!("Error saving last ping with IP: {:?}", err);
            });
    });

    (StatusCode::OK, Json(response))
}

/// Provide an HTTP response that streams the requested file from S3.
///
/// Returns an HTTP response whose body streams the file bytes when the `path` parameter
/// identifies an object in the configured assets bucket. If the `path` parameter is missing
/// the handler returns `StatusCode::BAD_REQUEST`. If obtaining a signed link or the S3 fetch
/// fails, the handler returns `StatusCode::INTERNAL_SERVER_ERROR`.
///
/// # Parameters
///
/// - `path`: Optional path parameter containing the file path to download; when absent the
///   request is considered malformed and results in `400 Bad Request`.
///
/// # Returns
///
/// `Ok(Response<Body>)` with a streaming body containing the file data on success; `Err(StatusCode::BAD_REQUEST)`
/// if the path parameter is missing; `Err(StatusCode::INTERNAL_SERVER_ERROR)` if a storage/S3 error occurs.
///
/// # Examples
///
/// ```ignore
/// // Typical invocation within an Axum route test:
/// // let resp = download_file(device_with_token, Some(Path("/dir/file.bin".into())), Extension(state)).await;
/// ```
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
security(
("device_token" = [])
),
)]
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

#[derive(Deserialize, Debug)]
pub struct FetchPackageQuery {
    name: String,
}

/// Fetches a package file from the configured S3 packages bucket and returns it as a streaming response.
///
/// On success this returns an HTTP response whose body streams the package bytes. If the package does not exist the handler returns a 404 response; on other failures it returns a 500 response.
///
/// # Returns
///
/// An HTTP `Response` that streams the requested package bytes, or an error `Response` with status 404 or 500.
///
/// # Examples
///
/// ```
/// // Example (requires a running server exposing the route):
/// // let resp = reqwest::blocking::get("http://localhost:3000/smith/package?name=example.deb").unwrap();
/// // assert!(resp.status().is_success() || resp.status() == reqwest::StatusCode::NOT_FOUND);
/// ```
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

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UploadResult {
    pub url: String,
}

// TODO: Change to streaming, so we are not saving in memory
/// Handle a multipart file upload, persist the file as an Asset in the configured storage,
/// and return a URL pointing to the stored file in the configured assets S3 bucket.
///
/// The returned `UploadResult.url` uses the `s3://{bucket}/{file_name}` form.
///
/// # Examples
///
/// ```no_run
/// // Illustrative example: after a successful upload the handler returns an UploadResult
/// // whose `url` points to the asset in the configured S3 bucket.
/// let result = UploadResult { url: "s3://my-assets-bucket/path/to/file.bin".into() };
/// assert!(result.url.starts_with("s3://my-assets-bucket/"));
/// ```
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
]
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

/// Returns the list of packages associated with a release.
///
/// Queries the database for all packages linked to the provided `release_id` and returns them
/// as JSON. If a database error occurs, the handler responds with an empty JSON array.
///
/// # Parameters
///
/// - `release_id`: Identifier of the release whose packages should be returned.
///
/// # Returns
///
/// `Json<Vec<Package>>` containing all packages for the given release; an empty array is returned
/// in the error response case.
///
/// # Examples
///
/// ```
/// // Example (illustrative): an HTTP GET to `/smith/releases/42/packages` will return
/// // a JSON array of packages associated with release id 42.
/// ```
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