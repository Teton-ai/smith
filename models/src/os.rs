use serde::{Deserialize, Serialize};

/// The base OS image attached to a release. `status` is one of `pending`,
/// `ready` or `failed`; nothing but a `ready` image is downloadable.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Os {
    pub id: i32,
    pub release_id: i32,
    pub file_name: String,
    pub object_key: String,
    pub checksum: String,
    pub size_bytes: i64,
    pub status: String,
    pub upload_id: Option<String>,
    pub part_size: i32,
    pub uploaded_at: Option<chrono::DateTime<chrono::Utc>>,
    pub user_id: Option<i32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Opens a push. `checksum` is the sha256 of the whole image, computed by the
/// client before it starts, and `size_bytes` is checked against the assembled
/// object at completion.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct NewOsUpload {
    pub file_name: String,
    pub checksum: String,
    pub size_bytes: i64,
    /// Bytes per part. Defaults server-side; must be 5 MiB..=5 GiB and yield at
    /// most 10,000 parts.
    pub part_size: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OsPartUrl {
    pub part_number: i32,
    pub url: String,
}

/// Everything the client needs to run (or resume) a push: the parts still
/// outstanding, each with a presigned URL, and the chunking it must use.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OsUploadPlan {
    pub os_id: i32,
    pub part_size: i32,
    pub total_parts: i32,
    /// Parts S3 has already acknowledged; the client skips these.
    pub uploaded_parts: Vec<i32>,
    pub parts: Vec<OsPartUrl>,
}

/// Reported by the client after S3 accepts a part. The ETag is required to
/// complete the upload and cannot be recovered later.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OsPartReport {
    pub etag: String,
    pub size_bytes: i64,
}

/// A time-limited link to the image. Handed over as data rather than as a
/// redirect so the browser's own download manager owns the transfer -- an
/// image is far too large to pull through a fetch in the tab.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct OsDownload {
    pub url: String,
    pub file_name: String,
    pub size_bytes: i64,
}
