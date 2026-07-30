//! Download tickets and the audit trail. Session lifecycle itself is shared
//! with the other device session kinds — see `crate::relay`.
//!
//! All state lives in Postgres rather than process memory, because the four
//! sockets involved in a browse can land on different api replicas.

use sqlx::PgPool;
use tracing::error;
use uuid::Uuid;

/// How long a download object stays in S3 before the sweeper removes it. The
/// signed URL is shorter still, so a link shared onward stops working first.
pub const DOWNLOAD_OBJECT_TTL_SECONDS: i64 = 60 * 60;
/// Lifetime of the CloudFront signed URL handed to the browser.
pub const SIGNED_URL_TTL_SECONDS: u64 = 15 * 60;
/// Prefix for staged downloads, so a bucket lifecycle rule can target them.
pub const OBJECT_PREFIX: &str = "file-browser";

pub struct PendingDownload {
    pub object_key: String,
    pub file_name: String,
    pub size: i64,
    pub session_id: Uuid,
    pub op_id: i64,
}

pub async fn create_download(
    pool: &PgPool,
    session_id: &Uuid,
    op_id: i64,
    upload_token: &str,
    object_key: &str,
    file_name: &str,
    size: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO file_download
            (upload_token, session_id, op_id, object_key, file_name, size)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        upload_token,
        session_id,
        op_id,
        object_key,
        file_name,
        size
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Claim an upload token. Single-use: the `uploaded_at IS NULL` guard and the
/// UPDATE are one statement, so a replayed token cannot overwrite the object,
/// even if two replicas race.
pub async fn claim_upload(
    pool: &PgPool,
    upload_token: &str,
) -> Result<Option<PendingDownload>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        UPDATE file_download
        SET uploaded_at = now()
        WHERE upload_token = $1 AND uploaded_at IS NULL
        RETURNING object_key, file_name, size, session_id, op_id
        "#,
        upload_token
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| PendingDownload {
        object_key: row.object_key,
        file_name: row.file_name,
        size: row.size,
        session_id: row.session_id,
        op_id: row.op_id,
    }))
}

/// How long a claimed-but-undeleted row stays claimed before another sweep pass
/// may retry it. Covers a replica that died between claiming and deleting.
const SWEEP_CLAIM_RETRY_SECONDS: i64 = 15 * 60;

/// Claim expired download rows for deletion. The claim and the guard are one
/// statement, so exactly one replica gets each object key even though every
/// replica sweeps.
///
/// Claiming rather than deleting is what makes a failed S3 delete recoverable:
/// the row survives, and a later pass retries it once the claim goes stale. A
/// `DELETE ... RETURNING` here would drop the row before the object, orphaning
/// it permanently if the delete failed or the process died mid-sweep.
pub async fn claim_expired_objects(pool: &PgPool) -> Result<Vec<(String, String)>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"
        UPDATE file_download
        SET swept_at = now()
        WHERE upload_token IN (
            SELECT upload_token FROM file_download
            WHERE created_at < now() - make_interval(secs => $1::double precision)
              AND (swept_at IS NULL
                   OR swept_at < now() - make_interval(secs => $2::double precision))
            FOR UPDATE SKIP LOCKED
        )
        RETURNING upload_token, object_key, uploaded_at
        "#,
        DOWNLOAD_OBJECT_TTL_SECONDS as f64,
        SWEEP_CLAIM_RETRY_SECONDS as f64
    )
    .fetch_all(pool)
    .await?;

    // Rows whose upload never completed have no object to remove, so they are
    // dropped here rather than handed to the caller for a pointless S3 call.
    let mut orphaned_tokens = Vec::new();
    let mut claimed = Vec::new();
    for row in rows {
        match row.uploaded_at {
            Some(_) => claimed.push((row.upload_token, row.object_key)),
            None => orphaned_tokens.push(row.upload_token),
        }
    }

    if !orphaned_tokens.is_empty() {
        sqlx::query!(
            "DELETE FROM file_download WHERE upload_token = ANY($1)",
            &orphaned_tokens
        )
        .execute(pool)
        .await?;
    }

    Ok(claimed)
}

/// Drop a claimed row once its object is actually gone from S3.
pub async fn finish_sweep(pool: &PgPool, upload_token: &str) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM file_download WHERE upload_token = $1",
        upload_token
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// One row per operation. Downloads are the ones that matter, but listings are
/// recorded too so "what did they look at" is answerable, not just "what did
/// they take".
#[allow(clippy::too_many_arguments)]
pub async fn record_access(
    pool: &PgPool,
    device_id: i32,
    user_id: Option<i32>,
    session_id: &Uuid,
    op: &str,
    path: &str,
    bytes: Option<i64>,
    outcome: &str,
    detail: Option<&str>,
) {
    sqlx::query!(
        r#"
        INSERT INTO device_file_access
            (device_id, user_id, session_id, op, path, bytes, outcome, detail)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        device_id,
        user_id,
        session_id,
        op,
        path,
        bytes,
        outcome,
        detail
    )
    .execute(pool)
    .await
    .inspect_err(|e| error!("Failed to record file access audit row: {e}"))
    .ok();
}
