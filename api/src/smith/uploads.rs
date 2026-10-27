use crate::{
    State,
    handlers::AuthedDevice,
    storage::{Storage, UploadPart},
    upload_token::{TokenError, UploadClaims},
};
use axum::{
    Extension, Json,
    extract::Path,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub const PART_SIZE: i64 = 5 * 1024 * 1024;
const MAX_SIZE: i64 = 512_000_000;
const TOKEN_TTL_SECONDS: i64 = 24 * 60 * 60;

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
            response
                .headers_mut()
                .insert("retry-after", axum::http::HeaderValue::from_static("5"));
        }
        response
    }
}

fn storage_error(error: anyhow::Error) -> UploadError {
    if crate::storage::is_missing_upload(&error) {
        return UploadError(StatusCode::GONE, "NoSuchUpload");
    }
    tracing::error!("Multipart storage operation failed");
    UploadError(StatusCode::BAD_GATEWAY, "StorageUnavailable")
}

fn capacity(state: &State) -> Result<tokio::sync::OwnedSemaphorePermit, UploadError> {
    state
        .upload_permits
        .clone()
        .try_acquire_owned()
        .map_err(|_| UploadError(StatusCode::SERVICE_UNAVAILABLE, "UploadCapacity"))
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
        });
    }
    let mut confirmed_parts =
        Storage::list_upload_parts(&claims.bucket, &claims.key, &claims.upload_id)
            .await
            .map_err(storage_error)?;
    confirmed_parts.sort_by_key(|p| p.part_number);
    if confirmed_parts
        .windows(2)
        .any(|pair| pair[0].part_number == pair[1].part_number)
    {
        return Err(UploadError(StatusCode::CONFLICT, "InvalidParts"));
    }
    for part in &confirmed_parts {
        if part.etag.is_empty()
            || part.size_bytes != part_length(claims.size_bytes, part.part_number)?
        {
            return Err(UploadError(StatusCode::CONFLICT, "InvalidParts"));
        }
    }
    let missing_parts = (1..=part_count(claims.size_bytes))
        .filter(|n| !confirmed_parts.iter().any(|part| part.part_number == *n))
        .collect();
    Ok(UploadPlan {
        confirmed_parts,
        missing_parts,
        url: None,
    })
}

#[utoipa::path(post,path="/smith/upload-sessions",request_body=OpenUpload,
    responses((status=200,body=OpenedUpload),(status=400,description="Invalid upload"),(status=503,description="Upload capacity reached")),security(("device_token"=[])))]
pub async fn open(
    device: AuthedDevice,
    Extension(state): Extension<State>,
    Json(request): Json<OpenUpload>,
) -> Result<Json<OpenedUpload>, UploadError> {
    validate(&request)?;
    let _permit = capacity(&state)?;
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
    let _permit = capacity(&state)?;
    Ok(Json(plan(&claims).await?))
}

#[utoipa::path(put,path="/smith/upload-sessions/parts/{number}",params(("number"=i32,Path),("x-upload-token"=String,Header)),
    request_body(content=Vec<u8>,content_type="application/octet-stream"),responses((status=200,body=UploadPart),(status=400,description="Invalid part length"),(status=410,description="Upload no longer exists")),security(("device_token"=[])))]
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
    let _permit = capacity(&state)?;
    let body = reqwest::Body::wrap_stream(request.into_body().into_data_stream());
    let etag = Storage::upload_session_part(
        &claims.bucket,
        &claims.key,
        &claims.upload_id,
        number,
        body,
        size as u64,
    )
    .await
    .map_err(storage_error)?;
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
    let _permit = capacity(&state)?;
    let plan = plan(&claims).await?;
    if plan.url.is_none() {
        if !plan.missing_parts.is_empty() {
            return Err(UploadError(StatusCode::CONFLICT, "MissingParts"));
        }
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
        if let Err(error) = result
            && !matches_object(&claims).await?
        {
            return Err(storage_error(error));
        }
        if !matches_object(&claims).await? {
            return Err(UploadError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "ChecksumMismatch",
            ));
        }
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
}

#[cfg(test)]
#[path = "uploads_tests.rs"]
mod integration_tests;
