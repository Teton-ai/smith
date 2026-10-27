use crate::{State, handlers::AuthedDevice, storage::Storage};
use axum::{
    Extension, Json,
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use std::time::Duration;
use utoipa::ToSchema;
use uuid::Uuid;

pub const PART_SIZE: i64 = 5 * 1024 * 1024;
const MAX_SIZE: i64 = 512_000_000;

pub struct UploadError(StatusCode);

impl From<StatusCode> for UploadError {
    fn from(status: StatusCode) -> Self {
        Self(status)
    }
}

impl IntoResponse for UploadError {
    fn into_response(self) -> Response {
        let mut response = self.0.into_response();
        if self.0 == StatusCode::SERVICE_UNAVAILABLE {
            response
                .headers_mut()
                .insert("retry-after", axum::http::HeaderValue::from_static("5"));
        }
        response
    }
}

#[derive(Deserialize, ToSchema)]
pub struct OpenUpload {
    key: String,
    size_bytes: i64,
    checksum: String,
    #[serde(default)]
    sensitive: bool,
}

#[derive(Serialize, Deserialize, FromRow, ToSchema)]
pub struct ConfirmedPart {
    part_number: i32,
    etag: String,
    checksum: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct UploadPlan {
    session_id: Uuid,
    part_size: i64,
    confirmed_parts: Vec<ConfirmedPart>,
    missing_parts: Vec<i32>,
    /// Present only after the object bytes have been verified.
    url: Option<String>,
}

#[derive(FromRow)]
struct Session {
    id: Uuid,
    bucket: String,
    object_key: String,
    size_bytes: i64,
    checksum: String,
    upload_id: Option<String>,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: chrono::DateTime<chrono::Utc>,
}

fn internal(error: impl std::fmt::Display) -> StatusCode {
    tracing::error!("Device upload session failed: {error}");
    StatusCode::INTERNAL_SERVER_ERROR
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

fn part_length(size: i64, number: i32) -> Result<usize, StatusCode> {
    if number < 1 || number > part_count(size) {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok((size - i64::from(number - 1) * PART_SIZE).min(PART_SIZE) as usize)
}

async fn owned_session(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    device_id: i32,
) -> Result<Session, StatusCode> {
    sqlx::query_as::<_, Session>(
        "SELECT * FROM device_upload_session WHERE id = $1 AND device_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(device_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)?
    .ok_or(StatusCode::NOT_FOUND)
}

async fn parts(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Vec<ConfirmedPart>, StatusCode> {
    sqlx::query_as::<_, ConfirmedPart>(
        "SELECT part_number, etag, checksum FROM device_upload_part WHERE session_id = $1 ORDER BY part_number",
    ).bind(id).fetch_all(&mut **tx).await.map_err(internal)
}

async fn plan(
    tx: &mut Transaction<'_, Postgres>,
    session: &Session,
) -> Result<UploadPlan, StatusCode> {
    let confirmed_parts = parts(tx, session.id).await?;
    let missing_parts = (1..=part_count(session.size_bytes))
        .filter(|n| !confirmed_parts.iter().any(|p| p.part_number == *n))
        .collect();
    Ok(UploadPlan {
        session_id: session.id,
        part_size: PART_SIZE,
        confirmed_parts,
        missing_parts,
        url: session
            .completed_at
            .map(|_| format!("s3://{}/{}", session.bucket, session.object_key)),
    })
}

#[utoipa::path(post, path = "/smith/upload-sessions", request_body = OpenUpload,
    responses((status = 200, body = UploadPlan), (status = 400, description = "Invalid upload"),
    (status = 409, description = "Key already has an active upload")), security(("device_token" = [])))]
pub async fn open(
    device: AuthedDevice,
    Extension(state): Extension<State>,
    Json(request): Json<OpenUpload>,
) -> Result<Json<UploadPlan>, UploadError> {
    validate(&request)?;
    let bucket = if request.sensitive {
        &state.config.data_engine_bucket_name
    } else {
        &state.config.assets_bucket_name
    };
    let mut tx = state.pg_pool.begin().await.map_err(internal)?;
    // A device has one identity for each immutable file, even across API replicas.
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(i64::from(device.id))
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let existing = sqlx::query_as::<_, Session>(
        "SELECT * FROM device_upload_session WHERE device_id=$1 AND bucket=$2 AND object_key=$3 AND size_bytes=$4 AND checksum=$5 FOR UPDATE",
    ).bind(device.id).bind(bucket).bind(&request.key).bind(request.size_bytes).bind(&request.checksum)
        .fetch_optional(&mut *tx).await.map_err(internal)?;
    let existing = match existing {
        Some(session)
            if session.completed_at.is_none()
                && session.created_at < chrono::Utc::now() - chrono::Duration::hours(12) =>
        {
            discard(&mut tx, &session).await?;
            None
        }
        other => other,
    };
    let session = if let Some(session) = existing {
        session
    } else {
        let id = Uuid::new_v4();
        // Lifecycle rules cover an S3 upload whose creation reply is lost.
        sqlx::query("INSERT INTO device_upload_session(id, device_id, bucket, object_key, size_bytes, checksum) VALUES ($1,$2,$3,$4,$5,$6)")
            .bind(id).bind(device.id).bind(bucket).bind(&request.key).bind(request.size_bytes).bind(&request.checksum)
            .execute(&mut *tx).await.map_err(|e| {
                if e.as_database_error().is_some_and(|e| e.is_unique_violation()) { StatusCode::CONFLICT } else { internal(e) }
            })?;
        Session {
            id,
            bucket: bucket.clone(),
            object_key: request.key,
            size_bytes: request.size_bytes,
            checksum: request.checksum,
            upload_id: None,
            completed_at: None,
            created_at: chrono::Utc::now(),
        }
    };
    let mut session = session;
    if session.upload_id.is_none() && session.completed_at.is_none() {
        let upload_id = Storage::initiate_multipart(
            &session.bucket,
            &session.object_key,
            "application/octet-stream",
            "attachment",
        )
        .await
        .map_err(internal)?;
        sqlx::query("UPDATE device_upload_session SET upload_id=$2 WHERE id=$1")
            .bind(session.id)
            .bind(&upload_id)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        session.upload_id = Some(upload_id);
    }
    sqlx::query("UPDATE device_upload_session SET touched_at=now() WHERE id=$1")
        .bind(session.id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let result = plan(&mut tx, &session).await?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(result))
}

#[utoipa::path(put, path = "/smith/upload-sessions/{id}/parts/{number}",
    params(("id" = Uuid, Path), ("number" = i32, Path), ("x-content-sha256" = String, Header)),
    request_body(content = Vec<u8>, content_type = "application/octet-stream"),
    responses((status = 200, body = ConfirmedPart), (status = 400, description = "Invalid part"),
    (status = 404, description = "Session not owned by device")), security(("device_token" = [])))]
pub async fn put_part(
    device: AuthedDevice,
    Extension(state): Extension<State>,
    Path((id, number)): Path<(Uuid, i32)>,
    request: axum::extract::Request,
) -> Result<Json<ConfirmedPart>, UploadError> {
    let _permit = state
        .upload_permits
        .try_acquire()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let mut tx = state.pg_pool.begin().await.map_err(internal)?;
    let session = owned_session(&mut tx, id, device.id).await?;
    if session.completed_at.is_some() {
        return Err(StatusCode::CONFLICT.into());
    }
    let claimed_checksum = request
        .headers()
        .get("x-content-sha256")
        .and_then(|h| h.to_str().ok())
        .ok_or(StatusCode::BAD_REQUEST)?
        .to_owned();
    let body = axum::body::to_bytes(request.into_body(), PART_SIZE as usize)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    let checksum = format!("{:x}", Sha256::digest(&body));
    if claimed_checksum != checksum {
        return Err(StatusCode::BAD_REQUEST.into());
    }
    if body.len() != part_length(session.size_bytes, number)? {
        return Err(StatusCode::BAD_REQUEST.into());
    }
    if let Some(part) = parts(&mut tx, id)
        .await?
        .into_iter()
        .find(|p| p.part_number == number)
    {
        if part.checksum != checksum {
            return Err(StatusCode::CONFLICT.into());
        }
        tx.commit().await.map_err(internal)?;
        return Ok(Json(part));
    }
    let upload_id = session.upload_id.as_deref().ok_or(StatusCode::CONFLICT)?;
    let result = Storage::upload_session_part(
        &session.bucket,
        &session.object_key,
        upload_id,
        number,
        body.to_vec(),
    )
    .await;
    let etag = match result {
        Ok(etag) => etag,
        Err(error) if crate::storage::is_missing_upload(&error) => {
            restart_multipart(&mut tx, &session).await?;
            tx.commit().await.map_err(internal)?;
            return Err(StatusCode::CONFLICT.into());
        }
        Err(error) => return Err(internal(error).into()),
    };
    sqlx::query("INSERT INTO device_upload_part (session_id, part_number, etag, checksum) VALUES ($1,$2,$3,$4)")
        .bind(id).bind(number).bind(&etag).bind(&checksum).execute(&mut *tx).await.map_err(internal)?;
    sqlx::query("UPDATE device_upload_session SET touched_at=now() WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(ConfirmedPart {
        part_number: number,
        etag,
        checksum,
    }))
}

#[utoipa::path(post, path = "/smith/upload-sessions/{id}/complete", params(("id" = Uuid, Path)),
    responses((status = 200, body = super::route::UploadResult), (status = 409, description = "Missing parts"),
    (status = 404, description = "Session not owned by device")), security(("device_token" = [])))]
pub async fn complete(
    device: AuthedDevice,
    Extension(state): Extension<State>,
    Path(id): Path<Uuid>,
) -> Result<Json<super::route::UploadResult>, UploadError> {
    let _permit = state
        .upload_permits
        .try_acquire()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let mut tx = state.pg_pool.begin().await.map_err(internal)?;
    let session = owned_session(&mut tx, id, device.id).await?;
    if session.completed_at.is_none() {
        let confirmed = parts(&mut tx, id).await?;
        if confirmed.len() != part_count(session.size_bytes) as usize
            || confirmed
                .iter()
                .enumerate()
                .any(|(i, p)| p.part_number != i as i32 + 1)
        {
            return Err(StatusCode::CONFLICT.into());
        }
        // S3 may have completed before the prior API process or database transaction failed.
        if !Storage::matches_object(
            &session.bucket,
            &session.object_key,
            session.size_bytes,
            &session.checksum,
        )
        .await
        .map_err(internal)?
        {
            let result = Storage::complete_multipart(
                &session.bucket,
                &session.object_key,
                session.upload_id.as_deref().ok_or(StatusCode::CONFLICT)?,
                confirmed
                    .into_iter()
                    .map(|p| s3::serde_types::Part {
                        part_number: p.part_number as u32,
                        etag: p.etag,
                    })
                    .collect(),
            )
            .await;
            if let Err(error) = result {
                if crate::storage::is_missing_upload(&error) {
                    restart_multipart(&mut tx, &session).await?;
                    tx.commit().await.map_err(internal)?;
                    return Err(StatusCode::CONFLICT.into());
                }
                return Err(internal(error).into());
            }
            if !Storage::matches_object(
                &session.bucket,
                &session.object_key,
                session.size_bytes,
                &session.checksum,
            )
            .await
            .map_err(internal)?
            {
                return Err(StatusCode::UNPROCESSABLE_ENTITY.into());
            }
        }
        sqlx::query("UPDATE device_upload_session SET completed_at=now(), touched_at=now(), upload_id=NULL WHERE id=$1")
            .bind(id).execute(&mut *tx).await.map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(Json(super::route::UploadResult {
        url: format!("s3://{}/{}", session.bucket, session.object_key),
    }))
}

#[utoipa::path(delete, path = "/smith/upload-sessions/{id}", params(("id" = Uuid, Path)),
    responses((status = 204, description = "Upload aborted"), (status = 404, description = "Session not owned by device")),
    security(("device_token" = [])))]
pub async fn abort(
    device: AuthedDevice,
    Extension(state): Extension<State>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, StatusCode> {
    let mut tx = state.pg_pool.begin().await.map_err(internal)?;
    let session = owned_session(&mut tx, id, device.id).await?;
    discard(&mut tx, &session).await?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn restart_multipart(
    tx: &mut Transaction<'_, Postgres>,
    session: &Session,
) -> Result<(), StatusCode> {
    let upload_id = Storage::initiate_multipart(
        &session.bucket,
        &session.object_key,
        "application/octet-stream",
        "attachment",
    )
    .await
    .map_err(internal)?;
    sqlx::query("DELETE FROM device_upload_part WHERE session_id=$1")
        .bind(session.id)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    sqlx::query("UPDATE device_upload_session SET upload_id=$2, touched_at=now() WHERE id=$1")
        .bind(session.id)
        .bind(upload_id)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    Ok(())
}

async fn discard(tx: &mut Transaction<'_, Postgres>, session: &Session) -> Result<(), StatusCode> {
    if let Some(upload_id) = &session.upload_id {
        match Storage::abort_multipart(&session.bucket, &session.object_key, upload_id).await {
            Ok(()) => {}
            Err(e)
                if e.downcast_ref::<s3::error::S3Error>()
                    .is_some_and(|e| matches!(e, s3::error::S3Error::HttpFailWithBody(404, _))) => {
            }
            Err(e) => return Err(internal(e)),
        }
    }
    sqlx::query("DELETE FROM device_upload_session WHERE id=$1")
        .bind(session.id)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    Ok(())
}

async fn sweep(pool: &PgPool) -> Result<(), StatusCode> {
    // Keep completed receipts beyond Brain's seven-day queue lifetime.
    loop {
        let mut tx = pool.begin().await.map_err(internal)?;
        let session = sqlx::query_as::<_, Session>(
            "SELECT * FROM device_upload_session WHERE (completed_at IS NOT NULL AND touched_at < now() - interval '14 days') OR (completed_at IS NULL AND created_at < now() - interval '12 hours') ORDER BY touched_at LIMIT 1 FOR UPDATE SKIP LOCKED",
        ).fetch_optional(&mut *tx).await.map_err(internal)?;
        let Some(session) = session else {
            return Ok(());
        };
        discard(&mut tx, &session).await?;
        tx.commit().await.map_err(internal)?;
    }
}

pub fn spawn_sweeper(pool: PgPool) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(3600));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if let Err(status) = sweep(&pool).await {
                tracing::error!(%status, "Failed to sweep device uploads");
            }
        }
    });
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
        assert_eq!(part_length(request.size_bytes, 1), Ok(PART_SIZE as usize));
        assert_eq!(part_length(request.size_bytes, 2), Ok(1));
        assert_eq!(
            part_length(request.size_bytes, 3),
            Err(StatusCode::BAD_REQUEST)
        );
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
