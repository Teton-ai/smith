use crate::{
    State,
    handlers::AuthedDevice,
    storage::{Storage, UploadPart},
    upload_token::{TokenError, UploadClaims},
};
use axum::{
    Extension, Json,
    extract::Path,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Instant;
use utoipa::ToSchema;

pub const PART_SIZE: i64 = 5 * 1024 * 1024;
const MAX_SIZE: i64 = 512_000_000;
const TOKEN_TTL_SECONDS: i64 = 24 * 60 * 60;
/// Retry-After value, in seconds, for every upload capacity 503.
pub const RETRY_AFTER_SECONDS: &str = "30";
/// Devices ask for fresh part URLs before this runs out.
const PART_URL_TTL_SECONDS: u32 = 60 * 60;

pub struct UploadError(StatusCode, &'static str);

impl From<StatusCode> for UploadError {
    fn from(status: StatusCode) -> Self {
        Self(status, "UploadRejected")
    }
}

impl IntoResponse for UploadError {
    fn into_response(self) -> Response {
        let mut response = (self.0, Json(serde_json::json!({"code":self.1}))).into_response();
        if self.0 == StatusCode::SERVICE_UNAVAILABLE {
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from_static(RETRY_AFTER_SECONDS),
            );
        }
        response
    }
}

fn storage_error(error: anyhow::Error) -> UploadError {
    if crate::storage::is_missing_upload(&error) {
        return UploadError(StatusCode::GONE, "NoSuchUpload");
    }
    tracing::error!(
        error = crate::storage::redacted(&error),
        "Multipart storage operation failed"
    );
    UploadError(StatusCode::BAD_GATEWAY, "StorageUnavailable")
}

#[derive(Deserialize, ToSchema)]
pub struct OpenUpload {
    key: String,
    size_bytes: i64,
    checksum: String,
    #[serde(default)]
    sensitive: bool,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct OpenedUpload {
    upload_id: String,
    token: String,
    part_size: i64,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct UploadPlan {
    confirmed_parts: Vec<UploadPart>,
    missing_parts: Vec<i32>,
    url: Option<String>,
    /// A CDN URL per missing part. The device PUTs the part bytes there and
    /// uses `PUT parts/{number}` on this API when the CDN URL fails.
    #[serde(default)]
    part_urls: Vec<PartUrl>,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct PartUrl {
    part_number: i32,
    url: String,
}

fn validate(request: &OpenUpload) -> Result<(), StatusCode> {
    if request.size_bytes <= 0
        || request.size_bytes > MAX_SIZE
        || request.key.is_empty()
        || request.key.len() > 1024
        || request.key.chars().any(char::is_control)
        || request.key.contains('\\')
        || request
            .key
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
        || request.checksum.len() != 64
        || !request
            .checksum
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
}

fn part_count(size: i64) -> i32 {
    ((size + PART_SIZE - 1) / PART_SIZE) as i32
}

fn part_length(size: i64, number: i32) -> Result<i64, UploadError> {
    if number < 1 || number > part_count(size) {
        return Err(StatusCode::BAD_REQUEST.into());
    }
    Ok((size - i64::from(number - 1) * PART_SIZE).min(PART_SIZE))
}

fn claims(
    device: &AuthedDevice,
    state: &State,
    headers: &HeaderMap,
) -> Result<UploadClaims, UploadError> {
    let token = headers
        .get("x-upload-token")
        .and_then(|v| v.to_str().ok())
        .ok_or(UploadError(StatusCode::FORBIDDEN, "InvalidUploadToken"))?;
    state
        .config
        .upload_token_key
        .verify(token, device.id, chrono::Utc::now().timestamp())
        .map_err(|e| match e {
            TokenError::Invalid => UploadError(StatusCode::FORBIDDEN, "InvalidUploadToken"),
            TokenError::Expired => UploadError(StatusCode::GONE, "UploadTokenExpired"),
        })
}

fn object_url(claims: &UploadClaims) -> String {
    format!("s3://{}/{}", claims.bucket, claims.key)
}

async fn matches_object(claims: &UploadClaims) -> Result<bool, UploadError> {
    Storage::matches_object(
        &claims.bucket,
        &claims.key,
        claims.size_bytes,
        &claims.checksum,
    )
    .await
    .map_err(storage_error)
}

async fn plan(claims: &UploadClaims) -> Result<UploadPlan, UploadError> {
    if matches_object(claims).await? {
        return Ok(UploadPlan {
            confirmed_parts: vec![],
            missing_parts: vec![],
            url: Some(object_url(claims)),
            part_urls: vec![],
        });
    }
    let listed = Storage::list_upload_parts(&claims.bucket, &claims.key, &claims.upload_id)
        .await
        .map_err(storage_error)?;
    let (confirmed_parts, missing_parts) = sort_parts(claims.size_bytes, listed)?;
    Ok(UploadPlan {
        confirmed_parts,
        missing_parts,
        url: None,
        part_urls: vec![],
    })
}

/// Splits the parts S3 lists into confirmed parts and missing part numbers.
/// A CDN part PUT has no length check, so a part with the wrong size counts
/// as missing and the device sends it again.
fn sort_parts(
    size: i64,
    mut parts: Vec<UploadPart>,
) -> Result<(Vec<UploadPart>, Vec<i32>), UploadError> {
    parts.sort_by_key(|p| p.part_number);
    if parts
        .windows(2)
        .any(|pair| pair[0].part_number == pair[1].part_number)
    {
        return Err(UploadError(StatusCode::CONFLICT, "InvalidParts"));
    }
    let mut confirmed = Vec::with_capacity(parts.len());
    for part in parts {
        if part.etag.is_empty() {
            return Err(UploadError(StatusCode::CONFLICT, "InvalidParts"));
        }
        if part.size_bytes == part_length(size, part.part_number)? {
            confirmed.push(part);
        }
    }
    let missing = (1..=part_count(size))
        .filter(|n| !confirmed.iter().any(|part| part.part_number == *n))
        .collect();
    Ok((confirmed, missing))
}

/// Presigned S3 `UploadPart` URLs moved onto the CDN. The CDN path forwards
/// the query string to the bucket's regional host, so S3 checks the
/// signature, the upload id and the part number. An error gives no URLs and
/// the device uses the API route.
async fn part_urls(state: &State, claims: &UploadClaims, parts: &[i32]) -> Vec<PartUrl> {
    let Some(cdn) = &state.config.device_upload_cdn_url else {
        return vec![];
    };
    if parts.is_empty() {
        return vec![];
    }
    let prefix = bucket_label(state, claims);
    let parts: Result<Vec<(i32, u64)>, _> = parts
        .iter()
        .map(|number| {
            part_length(claims.size_bytes, *number).map(|length| (*number, length as u64))
        })
        .collect();
    let Ok(parts) = parts else {
        return vec![];
    };
    let urls: anyhow::Result<Vec<PartUrl>> = Storage::presign_regional_upload_parts(
        &claims.bucket,
        &claims.key,
        &claims.upload_id,
        &parts,
        PART_URL_TTL_SECONDS,
    )
    .await
    .and_then(|urls| {
        urls.into_iter()
            .map(|(part_number, url)| {
                Ok(PartUrl {
                    part_number,
                    url: cdn_url(cdn, prefix, &url)?,
                })
            })
            .collect()
    });
    match urls {
        Ok(urls) => {
            metrics::counter!("smith_upload_part_urls_total", urls.len() as u64, "bucket" => prefix);
            urls
        }
        Err(error) => {
            tracing::error!("Failed to presign CDN upload parts: {error}");
            vec![]
        }
    }
}

/// `sensitive` for the data engine bucket, `assets` for the other bucket. It is
/// the CDN path prefix and the counter label.
fn bucket_label(state: &State, claims: &UploadClaims) -> &'static str {
    if claims.bucket == state.config.data_engine_bucket_name {
        "sensitive"
    } else {
        "assets"
    }
}

fn cdn_url(cdn: &str, prefix: &str, presigned: &str) -> anyhow::Result<String> {
    let url = reqwest::Url::parse(presigned)?;
    let query = url
        .query()
        .ok_or_else(|| anyhow::anyhow!("Presigned URL has no query"))?;
    Ok(format!("{cdn}/{prefix}{}?{query}", url.path()))
}

#[utoipa::path(post,path="/smith/upload-sessions",request_body=OpenUpload,
    responses((status=200,body=OpenedUpload),(status=400,description="Invalid upload")),security(("device_token"=[])))]
pub async fn open(
    device: AuthedDevice,
    Extension(state): Extension<State>,
    Json(request): Json<OpenUpload>,
) -> Result<Json<OpenedUpload>, UploadError> {
    validate(&request)?;
    let bucket = if request.sensitive {
        &state.config.data_engine_bucket_name
    } else {
        &state.config.assets_bucket_name
    };
    let upload_id = Storage::initiate_multipart(
        bucket,
        &request.key,
        "application/octet-stream",
        "attachment",
    )
    .await
    .map_err(storage_error)?;
    let claims = UploadClaims {
        device_id: device.id,
        bucket: bucket.clone(),
        key: request.key,
        upload_id: upload_id.clone(),
        size_bytes: request.size_bytes,
        checksum: request.checksum,
        expires_at: chrono::Utc::now().timestamp() + TOKEN_TTL_SECONDS,
    };
    let token = state
        .config
        .upload_token_key
        .sign(&claims)
        .map_err(|_| UploadError(StatusCode::INTERNAL_SERVER_ERROR, "TokenSigningFailed"))?;
    Ok(Json(OpenedUpload {
        upload_id,
        token,
        part_size: PART_SIZE,
    }))
}

#[utoipa::path(get,path="/smith/upload-sessions/status",params(("x-upload-token"=String,Header)),
    responses((status=200,body=UploadPlan),(status=403,description="Invalid token or device"),(status=410,description="Upload no longer exists")),security(("device_token"=[])))]
pub async fn status(
    device: AuthedDevice,
    Extension(state): Extension<State>,
    headers: HeaderMap,
) -> Result<Json<UploadPlan>, UploadError> {
    let claims = claims(&device, &state, &headers)?;
    let mut plan = plan(&claims).await?;
    plan.part_urls = part_urls(&state, &claims, &plan.missing_parts).await;
    Ok(Json(plan))
}

#[utoipa::path(put,path="/smith/upload-sessions/parts/{number}",params(("number"=i32,Path),("x-upload-token"=String,Header)),
    request_body(content=Vec<u8>,content_type="application/octet-stream"),responses((status=200,body=UploadPart),(status=400,description="Invalid part length or ClientAborted"),(status=410,description="Upload no longer exists"),(status=503,description="Upload capacity reached, retry after the Retry-After delay")),security(("device_token"=[])))]
pub async fn put_part(
    device: AuthedDevice,
    Extension(state): Extension<State>,
    Path(number): Path<i32>,
    request: axum::extract::Request,
) -> Result<Json<UploadPart>, UploadError> {
    let claims = claims(&device, &state, request.headers())?;
    let size = part_length(claims.size_bytes, number)?;
    if request
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        != Some(size)
    {
        return Err(UploadError(StatusCode::BAD_REQUEST, "InvalidPartLength"));
    }
    // Only part transfers hold a permit, because they last as long as the device link needs.
    let Ok(_permit) = state.upload_permits.try_acquire() else {
        return Err(UploadError(
            StatusCode::SERVICE_UNAVAILABLE,
            "UploadCapacity",
        ));
    };
    // Body read error means the device disconnected.
    let client_error = Arc::new(AtomicBool::new(false));
    let bytes = Arc::new(AtomicU64::new(0));
    let body = reqwest::Body::wrap_stream(request.into_body().into_data_stream().inspect({
        let client_error = client_error.clone();
        let bytes = bytes.clone();
        move |chunk| match chunk {
            Ok(chunk) => {
                bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            }
            Err(_) => client_error.store(true, Ordering::Relaxed),
        }
    }));
    let started = Instant::now();
    let etag = Storage::upload_session_part(
        &claims.bucket,
        &claims.key,
        &claims.upload_id,
        number,
        body,
        size as u64,
    )
    .await
    .map_err(|error| {
        let elapsed_ms = started.elapsed().as_millis() as u64;
        let body_error = client_error.load(Ordering::Relaxed);
        if body_error || crate::storage::is_request_timeout(&error) {
            tracing::warn!(
                reason = if body_error { "body_error" } else { "s3_timeout" },
                serial = %device.serial_number,
                part = number,
                bytes = bytes.load(Ordering::Relaxed),
                elapsed_ms,
                key = claims.key,
                error = crate::storage::redacted(&error),
                "Device upload aborted by client"
            );
            return UploadError(StatusCode::BAD_REQUEST, "ClientAborted");
        }
        storage_error(error.context(format!(
            "part {number} of {} after {elapsed_ms} ms",
            device.serial_number
        )))
    })?;
    metrics::increment_counter!("smith_upload_proxied_parts_total", "bucket" => bucket_label(&state, &claims));
    Ok(Json(UploadPart {
        part_number: number,
        etag,
        size_bytes: size,
    }))
}

#[utoipa::path(post,path="/smith/upload-sessions/complete",params(("x-upload-token"=String,Header)),
    responses((status=200,body=super::route::UploadResult),(status=409,description="Missing or invalid parts"),(status=410,description="Upload no longer exists")),security(("device_token"=[])))]
pub async fn complete(
    device: AuthedDevice,
    Extension(state): Extension<State>,
    headers: HeaderMap,
) -> Result<Json<super::route::UploadResult>, UploadError> {
    let claims = claims(&device, &state, &headers)?;
    let plan = plan(&claims).await?;
    if plan.url.is_none() {
        if !plan.missing_parts.is_empty() {
            return Err(UploadError(StatusCode::CONFLICT, "MissingParts"));
        }
        let parts = plan.confirmed_parts.len() as u64;
        let result = Storage::complete_multipart(
            &claims.bucket,
            &claims.key,
            &claims.upload_id,
            plan.confirmed_parts
                .into_iter()
                .map(|part| s3::serde_types::Part {
                    part_number: part.part_number as u32,
                    etag: part.etag,
                })
                .collect(),
        )
        .await;
        let verified = matches_object(&claims).await?;
        if let Err(error) = result
            && !verified
        {
            return Err(storage_error(error));
        }
        if !verified {
            return Err(UploadError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "ChecksumMismatch",
            ));
        }
        metrics::counter!("smith_upload_completed_parts_total", parts, "bucket" => bucket_label(&state, &claims));
    }
    Ok(Json(super::route::UploadResult {
        url: object_url(&claims),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_immutable_identity_and_ranges() {
        let mut request = OpenUpload {
            key: "clips/one.mp4".into(),
            size_bytes: PART_SIZE + 1,
            checksum: "a".repeat(64),
            sensitive: true,
        };
        assert_eq!(validate(&request), Ok(()));
        assert_eq!(part_length(request.size_bytes, 1).ok(), Some(PART_SIZE));
        assert_eq!(part_length(request.size_bytes, 2).ok(), Some(1));
        assert!(part_length(request.size_bytes, 3).is_err());
        for key in ["", "/clip", "clips/../clip", "clips//clip", "clips/\\clip"] {
            request.key = key.into();
            assert_eq!(validate(&request), Err(StatusCode::BAD_REQUEST));
        }
        request.key = "clips/one.mp4".into();
        request.checksum = "G".repeat(64);
        assert_eq!(validate(&request), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn wrong_size_part_is_missing_again() -> anyhow::Result<()> {
        let part = |part_number, size_bytes| UploadPart {
            part_number,
            etag: "\"e\"".into(),
            size_bytes,
        };
        let sorted = sort_parts(PART_SIZE + 1, vec![part(2, 1), part(1, 7)]);
        let Ok((confirmed, missing)) = sorted else {
            anyhow::bail!("valid parts were rejected");
        };
        assert_eq!(
            confirmed.iter().map(|p| p.part_number).collect::<Vec<_>>(),
            [2]
        );
        assert_eq!(missing, [1]);
        assert!(sort_parts(PART_SIZE + 1, vec![part(1, PART_SIZE), part(1, PART_SIZE)]).is_err());
        Ok(())
    }

    #[test]
    fn cdn_url_keeps_signed_path_and_query() {
        let presigned = "https://b.s3.eu-north-1.amazonaws.com/clips/a%20b.mp4?partNumber=2&uploadId=u%2B1&X-Amz-Signature=s";
        assert_eq!(
            cdn_url("https://cdn.teton.ai/device-uploads", "sensitive", presigned).ok(),
            Some("https://cdn.teton.ai/device-uploads/sensitive/clips/a%20b.mp4?partNumber=2&uploadId=u%2B1&X-Amz-Signature=s".into())
        );
    }
}

#[cfg(test)]
#[path = "uploads_tests.rs"]
mod integration_tests;
