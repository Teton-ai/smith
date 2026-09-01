use super::{
    CONTENT_TYPE, DEFAULT_PART_SIZE, MAX_PART_SIZE, MAX_PARTS, MIN_PART_SIZE,
    UPLOAD_URL_TTL_SECONDS, object_key, total_parts,
};
use crate::State;
use crate::release::get_release_by_id;
use crate::storage::{self, Storage};
use crate::user::CurrentUser;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::{Extension, Json};
use models::os::{NewOsUpload, Os, OsDownload, OsPartReport, OsPartUrl, OsUploadPlan};
use s3::serde_types::Part;
use tracing::{error, info, warn};

const OS_TAG: &str = "os";

/// An image may only be attached to, replaced on, or removed from an
/// unpublished draft -- the same rule `add_package_to_release` and the release
/// service routes apply. Devices converge on a release *id*, so a published
/// release whose image can still change would mean one version describing two
/// different systems depending on when a device pulled it.
async fn require_draft_release(release_id: i32, state: &State) -> Result<(), StatusCode> {
    let release = get_release_by_id(release_id, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get release: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    if release.yanked || !release.draft {
        return Err(StatusCode::CONFLICT);
    }

    Ok(())
}

async fn load_os(release_id: i32, pool: &sqlx::PgPool) -> Result<Option<Os>, sqlx::Error> {
    sqlx::query_as!(
        Os,
        "SELECT id, release_id, file_name, object_key, checksum, size_bytes, status,
                upload_id, part_size, uploaded_at, user_id, created_at
         FROM os WHERE release_id = $1",
        release_id
    )
    .fetch_optional(pool)
    .await
}

/// Rejects anything that would escape the release's prefix or produce a key the
/// device download path cannot address.
fn validate_file_name(file_name: &str) -> Result<(), StatusCode> {
    if file_name.is_empty()
        || file_name.contains('/')
        || file_name.contains('\\')
        || file_name.starts_with('.')
    {
        warn!("Rejected OS image file name: {file_name}");
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
}

/// The checksum is the image's identity and is compared on the device, so it is
/// stored in one canonical form rather than however the client happened to
/// print it.
fn normalize_checksum(checksum: &str) -> Result<String, StatusCode> {
    let normalized = checksum.trim().to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        warn!("Rejected OS image checksum: expected a hex sha256");
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(normalized)
}

fn resolve_part_size(requested: Option<i32>, size_bytes: i64) -> Result<i32, StatusCode> {
    let part_size = requested.unwrap_or(DEFAULT_PART_SIZE);

    if !(MIN_PART_SIZE..=MAX_PART_SIZE).contains(&part_size) {
        warn!("Rejected OS part size {part_size}");
        return Err(StatusCode::BAD_REQUEST);
    }
    if total_parts(size_bytes, part_size) > MAX_PARTS {
        warn!("Rejected OS part size {part_size}: more than {MAX_PARTS} parts");
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(part_size)
}

/// Builds the plan for whatever is still outstanding on an upload. Parts
/// already acknowledged by S3 are reported so a resumed push skips them; every
/// other part gets a fresh presigned URL, which is also how a push that outran
/// its original URLs recovers.
async fn build_plan(os: &Os, state: &State) -> Result<OsUploadPlan, StatusCode> {
    let upload_id = os.upload_id.as_deref().ok_or_else(|| {
        error!("OS {} has no upload in progress", os.id);
        StatusCode::CONFLICT
    })?;

    let uploaded_parts: Vec<i32> = sqlx::query_scalar!(
        "SELECT part_number FROM os_part WHERE os_id = $1 ORDER BY part_number",
        os.id
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get uploaded parts: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let total = total_parts(os.size_bytes, os.part_size);
    let outstanding: Vec<i32> = (1..=total as i32)
        .filter(|n| !uploaded_parts.contains(n))
        .collect();

    let urls = Storage::presign_upload_parts(
        &state.config.packages_bucket_name,
        &os.object_key,
        upload_id,
        &outstanding,
        UPLOAD_URL_TTL_SECONDS,
    )
    .await
    .map_err(|err| {
        error!("Failed to presign OS upload parts: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(OsUploadPlan {
        os_id: os.id,
        part_size: os.part_size,
        total_parts: total as i32,
        uploaded_parts,
        parts: urls
            .into_iter()
            .map(|(part_number, url)| OsPartUrl { part_number, url })
            .collect(),
    })
}

#[utoipa::path(
    post,
    path = "/releases/{release_id}/os",
    params(("release_id" = i32, Path)),
    request_body = NewOsUpload,
    responses(
        (status = StatusCode::CREATED, description = "Upload opened", body = OsUploadPlan),
        (status = StatusCode::BAD_REQUEST, description = "Invalid file name, checksum or part size"),
        (status = StatusCode::NOT_FOUND, description = "Release not found"),
        (status = StatusCode::CONFLICT, description = "Release is yanked or not in draft"),
        (status = StatusCode::INTERNAL_SERVER_ERROR, description = "Failed to open the upload"),
    ),
    security(("auth_token" = [])),
    tag = OS_TAG
)]
pub async fn create_os_upload(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
    Extension(current_user): Extension<CurrentUser>,
    Json(request): Json<NewOsUpload>,
) -> axum::response::Result<(StatusCode, Json<OsUploadPlan>), StatusCode> {
    require_draft_release(release_id, &state).await?;

    validate_file_name(&request.file_name)?;
    let checksum = normalize_checksum(&request.checksum)?;
    if request.size_bytes <= 0 {
        warn!("Rejected OS image size {}", request.size_bytes);
        return Err(StatusCode::BAD_REQUEST);
    }
    let part_size = resolve_part_size(request.part_size, request.size_bytes)?;

    // A re-push of the same image resumes instead of starting over: same file
    // and same bytes means the parts already in S3 are still the right parts.
    if let Some(existing) = load_os(release_id, &state.pg_pool).await.map_err(|err| {
        error!("Failed to get existing os: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })? {
        if existing.status == "pending"
            && existing.checksum == checksum
            && existing.file_name == request.file_name
            && existing.part_size == part_size
            && existing.size_bytes == request.size_bytes
        {
            let plan = build_plan(&existing, &state).await?;
            return Ok((StatusCode::CREATED, Json(plan)));
        }

        delete_os_image(&existing, &state).await?;
    }

    let key = object_key(release_id, &request.file_name);
    let upload_id =
        Storage::initiate_multipart(&state.config.packages_bucket_name, &key, CONTENT_TYPE)
            .await
            .map_err(|err| {
                error!("Failed to initiate OS multipart upload: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

    let os = match sqlx::query_as!(
        Os,
        "INSERT INTO os (release_id, file_name, object_key, checksum, size_bytes, upload_id,
                         part_size, user_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING id, release_id, file_name, object_key, checksum, size_bytes, status,
                   upload_id, part_size, uploaded_at, user_id, created_at",
        release_id,
        request.file_name,
        key,
        checksum,
        request.size_bytes,
        upload_id,
        part_size,
        current_user.user_id
    )
    .fetch_one(&state.pg_pool)
    .await
    {
        Ok(os) => os,
        Err(err) => {
            error!("Failed to create os row: {err}");
            // Nothing references the upload now, so leaving it open would just
            // bill for parts no one can complete.
            if let Err(e) =
                Storage::abort_multipart(&state.config.packages_bucket_name, &key, &upload_id).await
            {
                error!("Failed to abort orphaned OS upload {key}: {e}");
            }
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let plan = build_plan(&os, &state).await?;
    Ok((StatusCode::CREATED, Json(plan)))
}

#[utoipa::path(
    put,
    path = "/releases/{release_id}/os/parts/{part_number}",
    params(
        ("release_id" = i32, Path),
        ("part_number" = i32, Path),
    ),
    request_body = OsPartReport,
    responses(
        (status = StatusCode::NO_CONTENT, description = "Part recorded"),
        (status = StatusCode::BAD_REQUEST, description = "Part number outside the upload"),
        (status = StatusCode::NOT_FOUND, description = "No image on this release"),
        (status = StatusCode::CONFLICT, description = "No upload in progress"),
    ),
    security(("auth_token" = [])),
    tag = OS_TAG
)]
pub async fn report_os_part(
    Path((release_id, part_number)): Path<(i32, i32)>,
    Extension(state): Extension<State>,
    Json(report): Json<OsPartReport>,
) -> axum::response::Result<StatusCode, StatusCode> {
    require_draft_release(release_id, &state).await?;

    let os = load_os(release_id, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get os: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    if os.status != "pending" || os.upload_id.is_none() {
        return Err(StatusCode::CONFLICT);
    }

    let total = total_parts(os.size_bytes, os.part_size);
    if part_number < 1 || part_number as i64 > total {
        warn!(
            "Rejected part {part_number} for os {}: {total} parts",
            os.id
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    // S3 returns the ETag quoted and CompleteMultipartUpload wants it quoted,
    // but clients vary in what they pass through, so it is stored bare and
    // re-quoted at completion.
    let etag = report.etag.trim().trim_matches('"').to_string();
    if etag.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    sqlx::query!(
        "INSERT INTO os_part (os_id, part_number, etag, size_bytes)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (os_id, part_number)
         DO UPDATE SET etag = EXCLUDED.etag,
                       size_bytes = EXCLUDED.size_bytes,
                       uploaded_at = now()",
        os.id,
        part_number,
        etag,
        report.size_bytes
    )
    .execute(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to record os part: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/releases/{release_id}/os/complete",
    params(("release_id" = i32, Path)),
    responses(
        (status = StatusCode::OK, description = "Image assembled and ready", body = Os),
        (status = StatusCode::NOT_FOUND, description = "No image on this release"),
        (status = StatusCode::CONFLICT, description = "Upload incomplete, or assembled size does not match"),
    ),
    security(("auth_token" = [])),
    tag = OS_TAG
)]
pub async fn complete_os_upload(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Os>, StatusCode> {
    require_draft_release(release_id, &state).await?;

    let os = load_os(release_id, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get os: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let upload_id = os.upload_id.clone().ok_or(StatusCode::CONFLICT)?;

    let recorded = sqlx::query!(
        "SELECT part_number, etag, size_bytes FROM os_part WHERE os_id = $1 ORDER BY part_number",
        os.id
    )
    .fetch_all(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to get os parts: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Completing a gapped upload produces a silently corrupt object, so the
    // parts are checked for coverage before S3 is asked to assemble anything.
    let total = total_parts(os.size_bytes, os.part_size);
    if recorded.len() as i64 != total {
        warn!(
            "Refusing to complete os {}: {} of {total} parts recorded",
            os.id,
            recorded.len()
        );
        return Err(StatusCode::CONFLICT);
    }
    let contiguous = recorded
        .iter()
        .enumerate()
        .all(|(i, part)| part.part_number as usize == i + 1);
    let recorded_bytes: i64 = recorded.iter().map(|part| part.size_bytes).sum();
    if !contiguous || recorded_bytes != os.size_bytes {
        warn!(
            "Refusing to complete os {}: contiguous={contiguous}, {recorded_bytes} of {} bytes",
            os.id, os.size_bytes
        );
        return Err(StatusCode::CONFLICT);
    }

    let parts = recorded
        .iter()
        .map(|part| Part {
            part_number: part.part_number as u32,
            etag: format!("\"{}\"", part.etag),
        })
        .collect();

    Storage::complete_multipart(
        &state.config.packages_bucket_name,
        &os.object_key,
        &upload_id,
        parts,
    )
    .await
    .map_err(|err| {
        error!("Failed to complete OS upload {}: {err}", os.object_key);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // The parts agreed on the total, but only S3 knows what it actually
    // assembled. A mismatch means the image is not what the client described,
    // and an image nobody can trust is worse than no image.
    let stored = Storage::object_size(&state.config.packages_bucket_name, &os.object_key)
        .await
        .map_err(|err| {
            error!("Failed to stat completed OS image: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if stored != os.size_bytes {
        error!(
            "Completed OS image {} is {stored} bytes, expected {}",
            os.object_key, os.size_bytes
        );
        if let Err(e) =
            Storage::delete_from_s3(&state.config.packages_bucket_name, &os.object_key).await
        {
            error!(
                "Failed to delete mismatched OS image {}: {e}",
                os.object_key
            );
        }
        sqlx::query!("DELETE FROM os WHERE id = $1", os.id)
            .execute(&state.pg_pool)
            .await
            .map_err(|err| {
                error!("Failed to drop mismatched os row: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        return Err(StatusCode::CONFLICT);
    }

    // Part rows exist only to reach this point; the ETags are meaningless once
    // the object is assembled.
    let os = sqlx::query_as!(
        Os,
        "UPDATE os SET status = 'ready', uploaded_at = now(), upload_id = NULL
         WHERE id = $1
         RETURNING id, release_id, file_name, object_key, checksum, size_bytes, status,
                   upload_id, part_size, uploaded_at, user_id, created_at",
        os.id
    )
    .fetch_one(&state.pg_pool)
    .await
    .map_err(|err| {
        error!("Failed to mark os ready: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    sqlx::query!("DELETE FROM os_part WHERE os_id = $1", os.id)
        .execute(&state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to clear os parts: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    info!(
        "OS image ready for release {release_id}: {} ({} bytes)",
        os.object_key, os.size_bytes
    );

    Ok(Json(os))
}

#[utoipa::path(
    get,
    path = "/releases/{release_id}/os",
    params(("release_id" = i32, Path)),
    responses(
        (status = StatusCode::OK, description = "The image attached to the release", body = Os),
        (status = StatusCode::NOT_FOUND, description = "No image on this release"),
    ),
    security(("auth_token" = [])),
    tag = OS_TAG
)]
pub async fn get_os(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<Os>, StatusCode> {
    let os = load_os(release_id, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get os: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(os))
}

/// Removes an image and whatever it is holding in S3: an open multipart upload
/// if the push never finished, the assembled object if it did.
async fn delete_os_image(os: &Os, state: &State) -> Result<(), StatusCode> {
    match os.upload_id.as_deref() {
        Some(upload_id) => {
            if let Err(e) = Storage::abort_multipart(
                &state.config.packages_bucket_name,
                &os.object_key,
                upload_id,
            )
            .await
            {
                // Worth a line, not worth blocking the caller: the sweeper
                // reclaims uploads whose URLs have expired.
                error!("Failed to abort OS upload {}: {e}", os.object_key);
            }
        }
        None => {
            if let Err(e) =
                Storage::delete_from_s3(&state.config.packages_bucket_name, &os.object_key).await
            {
                error!("Failed to delete OS image {}: {e}", os.object_key);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    sqlx::query!("DELETE FROM os WHERE id = $1", os.id)
        .execute(&state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to delete os row: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(())
}

#[utoipa::path(
    delete,
    path = "/releases/{release_id}/os",
    params(("release_id" = i32, Path)),
    responses(
        (status = StatusCode::NO_CONTENT, description = "Image removed"),
        (status = StatusCode::NOT_FOUND, description = "No image on this release"),
        (status = StatusCode::CONFLICT, description = "Release is yanked or not in draft"),
    ),
    security(("auth_token" = [])),
    tag = OS_TAG
)]
pub async fn delete_os(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<StatusCode, StatusCode> {
    require_draft_release(release_id, &state).await?;

    let os = load_os(release_id, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get os: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    delete_os_image(&os, &state).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/releases/{release_id}/os/download",
    params(("release_id" = i32, Path)),
    responses(
        (status = StatusCode::OK, description = "A time-limited link to the image", body = OsDownload),
        (status = StatusCode::NOT_FOUND, description = "No image on this release"),
        (status = StatusCode::CONFLICT, description = "Image upload has not completed"),
    ),
    security(("auth_token" = [])),
    tag = OS_TAG
)]
pub async fn download_os(
    Path(release_id): Path<i32>,
    Extension(state): Extension<State>,
) -> axum::response::Result<Json<OsDownload>, StatusCode> {
    let os = load_os(release_id, &state.pg_pool)
        .await
        .map_err(|err| {
            error!("Failed to get os: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    if os.status != "ready" {
        return Err(StatusCode::CONFLICT);
    }

    // The object lives in the packages bucket, so the existing CloudFront
    // distribution, key pair and `package-download` behaviour serve it. Only
    // the lifetime differs: an image is orders of magnitude larger than a
    // package and is pulled over whatever link a device happens to have.
    let url = Storage::signed_url(
        &state.config.cloudfront.package_domain_name,
        &state.config.cloudfront.package_key_pair_id,
        &state.config.cloudfront.package_private_key,
        &format!("package-download/{}", os.object_key),
        storage::OS_SIGNED_URL_TTL_SECONDS,
    )
    .map_err(|err| {
        error!("Failed to sign OS image link: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(OsDownload {
        url,
        file_name: os.file_name,
        size_bytes: os.size_bytes,
    }))
}
