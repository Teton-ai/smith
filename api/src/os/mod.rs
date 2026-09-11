pub mod route;

use crate::storage::Storage;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::{MissedTickBehavior, sleep};
use tracing::{error, info};

/// Fallback for anything whose extension we do not recognise. Deliberately
/// generic: a browser maps a concrete type back to its own preferred extension
/// and renames the download to match, so claiming less is what keeps the name
/// intact.
const DEFAULT_CONTENT_TYPE: &str = "application/octet-stream";

/// Stamped on the object when the multipart upload is opened. Deliberately not
/// signed into the part URLs: a signed `Content-Type` must be echoed back
/// byte-identically by the client or S3 rejects the part.
///
/// This has to describe the bytes we were actually handed. S3 stores whatever
/// is declared here and CloudFront echoes it back on download, so declaring
/// gzip for an `.iso` leaves the browser holding a header that contradicts the
/// URL -- it trusts the header and saves the image as `.gz`.
pub fn content_type_for(file_name: &str) -> &'static str {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".gz") || lower.ends_with(".tgz") {
        "application/gzip"
    } else {
        DEFAULT_CONTENT_TYPE
    }
}

/// Stamped alongside the content type so the name the image was pushed under is
/// the name it comes back as. Without it the browser has to guess from the URL,
/// and the dashboard cannot help: its `download` attribute is ignored because
/// the link points at the CDN rather than at our own origin.
pub fn content_disposition_for(file_name: &str) -> String {
    format!("attachment; filename=\"{file_name}\"")
}

/// 100 MiB puts a 20 GiB image at ~205 parts -- few enough round trips to stay
/// cheap, small enough that a failed part is a cheap retry over a bad link.
pub const DEFAULT_PART_SIZE: i32 = 100 * 1024 * 1024;
/// S3's floor for every part except the last.
pub const MIN_PART_SIZE: i32 = 5 * 1024 * 1024;
/// S3 allows 5 GiB, but nothing about a resumable push is improved by a chunk
/// that large and it keeps the column comfortably inside `integer`.
pub const MAX_PART_SIZE: i32 = 1024 * 1024 * 1024;
/// Hard S3 limit on parts in one upload.
pub const MAX_PARTS: i64 = 10_000;
/// `rust-s3` refuses anything longer (`validate_expiry`), and S3's own SigV4
/// ceiling is the same 7 days. Expiry is evaluated when a part *starts*, so
/// this bounds how long a push may sit idle, not how long it may take.
pub const UPLOAD_URL_TTL_SECONDS: u32 = 7 * 24 * 60 * 60;

const SWEEP_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// An upload is abandoned once its part URLs have expired, because from that
/// point it can never be completed. Tying the two together means a push is
/// never swept while it could still legitimately finish.
const ABANDONED_UPLOAD_TTL_SECONDS: u64 = UPLOAD_URL_TTL_SECONDS as u64;

/// One image per release, so the release id alone identifies the object.
pub fn object_key(release_id: i32, file_name: &str) -> String {
    format!("os/{release_id}/{file_name}")
}

/// Number of parts an image of `size_bytes` splits into. The last part is
/// whatever remains and may be shorter than `part_size`.
pub fn total_parts(size_bytes: i64, part_size: i32) -> i64 {
    let part_size = part_size as i64;
    (size_bytes + part_size - 1) / part_size
}

/// Abort multipart uploads that can no longer be completed and drop their rows.
///
/// Safe on every replica: the claim marks and returns rows in one statement, so
/// each abandoned upload goes to exactly one process. A row whose abort fails
/// keeps its `upload_id` and is retried on a later pass rather than being
/// dropped with parts still stored in S3.
pub fn spawn_sweeper(pool: PgPool, bucket: &'static str) {
    tokio::spawn(async move {
        // Replicas all boot together during a rolling deploy and `interval`
        // fires immediately, so stagger rather than sweep in lockstep.
        sleep(SWEEP_INTERVAL / 2).await;

        let mut ticker = tokio::time::interval(SWEEP_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;

            if let Err(e) = sweep_abandoned_uploads(&pool, bucket).await {
                error!("Failed to sweep abandoned OS uploads: {e}");
            }
        }
    });
}

async fn sweep_abandoned_uploads(pool: &PgPool, bucket: &str) -> Result<(), sqlx::Error> {
    let claimed = sqlx::query!(
        r#"
        UPDATE os SET status = 'failed'
        WHERE id IN (
            SELECT id FROM os
            WHERE status IN ('pending', 'failed')
              AND upload_id IS NOT NULL
              AND created_at < now() - make_interval(secs => $1::double precision)
            FOR UPDATE SKIP LOCKED
        )
        RETURNING id, object_key, upload_id AS "upload_id!"
        "#,
        ABANDONED_UPLOAD_TTL_SECONDS as f64
    )
    .fetch_all(pool)
    .await?;

    for row in claimed {
        if let Err(e) = Storage::abort_multipart(bucket, &row.object_key, &row.upload_id).await {
            // The row keeps its upload_id, so the next pass tries again rather
            // than leaving parts billing in S3 with nothing pointing at them.
            error!(
                "Failed to abort abandoned OS upload {} ({}): {e}",
                row.id, row.object_key
            );
            continue;
        }

        sqlx::query!("DELETE FROM os WHERE id = $1", row.id)
            .execute(pool)
            .await?;

        info!("Swept abandoned OS upload {} ({})", row.id, row.object_key);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The api completes an upload only when the recorded parts cover the image
    /// exactly, and the client derives its byte ranges from the same numbers.
    /// A disagreement here assembles a corrupt object rather than failing, so
    /// the split is pinned.
    #[test]
    fn splits_an_image_into_parts() {
        let gib = 1024 * 1024 * 1024;

        // 20 GiB at 100 MiB: 204 full parts and a short remainder.
        assert_eq!(total_parts(20 * gib, DEFAULT_PART_SIZE), 205);

        // An exact multiple must not produce a trailing empty part.
        assert_eq!(
            total_parts(DEFAULT_PART_SIZE as i64 * 4, DEFAULT_PART_SIZE),
            4
        );

        // Anything smaller than one part is still one part.
        assert_eq!(total_parts(1, DEFAULT_PART_SIZE), 1);
        assert_eq!(
            total_parts(DEFAULT_PART_SIZE as i64 + 1, DEFAULT_PART_SIZE),
            2
        );
    }

    /// The largest image the part limit allows, to catch a `MAX_PARTS` or
    /// `MAX_PART_SIZE` change that quietly caps how big an image may be.
    #[test]
    fn largest_image_fits_within_the_part_limit() {
        let max_bytes = MAX_PART_SIZE as i64 * MAX_PARTS;
        assert_eq!(total_parts(max_bytes, MAX_PART_SIZE), MAX_PARTS);
        assert!(total_parts(max_bytes + 1, MAX_PART_SIZE) > MAX_PARTS);
    }

    #[test]
    fn object_key_is_scoped_to_the_release() {
        assert_eq!(object_key(43, "base.tar.gz"), "os/43/base.tar.gz");
    }

    /// A content type that contradicts the extension is what makes a browser
    /// rename the download, so every name maps to a type that agrees with it or
    /// to one generic enough to claim nothing.
    #[test]
    fn content_type_follows_the_extension() {
        assert_eq!(content_type_for("base.tar.gz"), "application/gzip");
        assert_eq!(content_type_for("base.gz"), "application/gzip");
        assert_eq!(content_type_for("base.tgz"), "application/gzip");
        assert_eq!(content_type_for("BASE.TAR.GZ"), "application/gzip");

        // The regression: an image that is not gzip must not claim to be, or
        // the browser saves it as `.gz`.
        assert_eq!(
            content_type_for("overview-installer_6574729_amd64.iso"),
            "application/octet-stream"
        );
        assert_eq!(content_type_for("base.img"), "application/octet-stream");
        assert_eq!(content_type_for("base"), "application/octet-stream");

        // `.gz` has to be the real suffix, not merely present in the name.
        assert_eq!(content_type_for("base.gz.iso"), "application/octet-stream");
    }

    #[test]
    fn content_disposition_carries_the_file_name() {
        assert_eq!(
            content_disposition_for("overview-installer_6574729_amd64.iso"),
            "attachment; filename=\"overview-installer_6574729_amd64.iso\""
        );
    }
}
