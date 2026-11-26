use axum::response::Response;
use cloudfront_sign::{SignedOptions, get_signed_url};
use s3::bucket::CHUNK_SIZE;
use s3::creds::Credentials;
use s3::serde_types::Part;
use s3::utils::read_chunk_async;
use s3::{Bucket, Region};
use std::borrow::Cow;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, OnceCell, RwLock};
use tracing::{error, warn};

const CREDENTIAL_REFRESH_MARGIN_SECS: i64 = 5 * 60;
const CREDENTIAL_REFRESH_COOLDOWN: Duration = Duration::from_secs(60);

const CHECKSUM_ALGORITHM_HEADER: &str = "x-amz-checksum-algorithm";
const CHECKSUM_TYPE_HEADER: &str = "x-amz-checksum-type";
const CRC64NVME_CHECKSUM_HEADER: &str = "x-amz-checksum-crc64nvme";
const CHECKSUM_MODE_HEADER: &str = "x-amz-checksum-mode";
const CHECKSUM_MODE_ENABLED: &str = "ENABLED";
const CRC64NVME: &str = "CRC64NVME";
const FULL_OBJECT: &str = "FULL_OBJECT";
const BAD_DIGEST: &str = "BadDigest";
const NO_SUCH_UPLOAD: &str = "NoSuchUpload";
const REQUEST_TIMEOUT: &str = "RequestTimeout";

/// Lifetime of a package's CloudFront URL. Packages are small enough that an
/// hour comfortably covers a download plus retries.
pub const PACKAGE_SIGNED_URL_TTL_SECONDS: u64 = 60 * 60;

/// Lifetime of an OS image's CloudFront URL. These run to tens of gigabytes and
/// devices pull them over whatever link they have, so the URL has to outlive a
/// slow transfer -- smithd reuses the same one when it resumes.
pub const OS_SIGNED_URL_TTL_SECONDS: u64 = 24 * 60 * 60;

#[derive(serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct UploadPart {
    pub part_number: i32,
    pub etag: String,
    pub size_bytes: i64,
}

#[derive(serde::Deserialize)]
struct ListedUploadParts {
    #[serde(rename = "Part", default)]
    parts: Vec<ListedUploadPart>,
    #[serde(rename = "IsTruncated", default)]
    is_truncated: bool,
}

#[derive(serde::Deserialize)]
struct ListedUploadPart {
    #[serde(rename = "PartNumber")]
    part_number: i32,
    #[serde(rename = "ETag")]
    etag: String,
    #[serde(rename = "Size")]
    size_bytes: i64,
}

pub struct Storage;

/// Records in-flight, outcome and size for one upload; Drop covers every exit path.
/// `result` is "client" while waiting on the request body, "error" while waiting on S3.
pub struct UploadMetrics {
    bytes: u64,
    result: &'static str,
}

impl UploadMetrics {
    pub fn start() -> Self {
        metrics::increment_gauge!("smith_uploads_in_flight", 1.0);
        Self {
            bytes: 0,
            result: "client",
        }
    }
}

impl Drop for UploadMetrics {
    fn drop(&mut self) {
        metrics::decrement_gauge!("smith_uploads_in_flight", 1.0);
        metrics::increment_counter!("smith_uploads_total", "result" => self.result);
        if self.result == "ok" {
            metrics::histogram!("smith_upload_bytes", self.bytes as f64);
        }
    }
}

impl Storage {
    pub async fn save_to_s3(
        bucket_name: &str,
        path: Option<&str>,
        file_name: &str,
        data: &[u8],
    ) -> anyhow::Result<()> {
        let bucket = Self::bucket(bucket_name).await?;

        let object_key = match path {
            Some(p) => format!("{}/{}", p, file_name),
            None => file_name.to_string(),
        };

        bucket.put_object(&object_key, data).await?;
        Ok(())
    }

    /// Streams a reader into S3 with one 8 MiB part in flight at a time. rust-s3's
    /// own `put_object_stream` queues every chunk first, so it buffers the whole object.
    pub async fn stream_to_s3<R>(
        bucket_name: &str,
        object_key: &str,
        reader: &mut R,
        metrics: &mut UploadMetrics,
    ) -> anyhow::Result<u16>
    where
        R: tokio::io::AsyncRead + Unpin + ?Sized,
    {
        const CONTENT_TYPE: &str = "application/octet-stream";

        let bucket = Self::bucket(bucket_name).await?;

        // Anything that fits in one chunk skips multipart entirely.
        let first_chunk = read_chunk_async(reader).await?;
        metrics.bytes = first_chunk.len() as u64;
        metrics.result = "error";
        if first_chunk.len() < CHUNK_SIZE {
            let response = bucket
                .put_object_with_content_type(object_key, &first_chunk, CONTENT_TYPE)
                .await?;
            let status = response.status_code();
            if (200..300).contains(&status) {
                metrics.result = "ok";
            }
            return Ok(status);
        }

        let upload_id = bucket
            .initiate_multipart_upload(object_key, CONTENT_TYPE)
            .await?
            .upload_id;

        let result = async {
            let mut chunk = first_chunk;
            let mut part_number: u32 = 1;
            let mut parts = Vec::new();

            loop {
                let last = chunk.len() < CHUNK_SIZE;
                parts.push(
                    bucket
                        .put_multipart_chunk(
                            chunk,
                            object_key,
                            part_number,
                            &upload_id,
                            CONTENT_TYPE,
                        )
                        .await?,
                );
                if last {
                    break;
                }
                part_number += 1;
                metrics.result = "client";
                chunk = read_chunk_async(reader).await?;
                metrics.result = "error";
                metrics.bytes += chunk.len() as u64;
                // An exact multiple of CHUNK_SIZE has no trailing part to send.
                if chunk.is_empty() {
                    break;
                }
            }

            let response = bucket
                .complete_multipart_upload(object_key, &upload_id, parts)
                .await?;
            let status = response.status_code();
            validate_completion(status, response.as_str()?)?;
            Ok::<u16, anyhow::Error>(status)
        }
        .await;

        if result.is_ok() {
            metrics.result = "ok";
        }
        if result.is_err() {
            // Parts of an abandoned upload are billed until the upload is aborted.
            if let Err(abort_error) = bucket.abort_upload(object_key, &upload_id).await {
                error!("Abort after failed upload of {object_key} did not confirm: {abort_error}");
            }
        }

        result
    }

    /// A time-limited CloudFront URL for an object staged by the file browser.
    /// The browser fetches straight from the CDN, so the api never sits in the
    /// byte path on the way out.
    pub fn signed_url(
        cdn_domain: &str,
        cdn_key_pair_id: &str,
        cdn_private_key: &str,
        object_key: &str,
        ttl_seconds: u64,
    ) -> anyhow::Result<String> {
        let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH)?;

        let options = SignedOptions {
            key_pair_id: Cow::from(cdn_key_pair_id.to_string()),
            private_key: Cow::from(cdn_private_key.to_string()),
            date_less_than: since_epoch.as_secs() + ttl_seconds,
            ..Default::default()
        };

        let url = format!("{cdn_domain}/{object_key}");
        get_signed_url(&url, &options).map_err(anyhow::Error::from)
    }

    pub async fn delete_from_s3(bucket_name: &str, path: &str) -> anyhow::Result<()> {
        let bucket = Self::bucket(bucket_name).await?;
        bucket.delete_object(path).await?;
        Ok(())
    }

    pub async fn download_from_s3(bucket_name: &str, file_name: &str) -> anyhow::Result<Vec<u8>> {
        let bucket = Self::bucket(bucket_name).await?;
        let response = bucket.get_object(file_name).await?;
        Ok(response.to_vec())
    }

    /// `ttl_seconds` bounds the signed URL. CloudFront checks it when a request
    /// starts, and smithd reuses one URL across every resume of a download
    /// (`smithd/src/downloader/download.rs`), so it has to outlast the whole
    /// transfer -- an hour is fine for a package and far too short for a
    /// multi-gigabyte OS image.
    pub async fn download_package_from_cdn(
        bucket_name: &str,
        path: Option<&str>,
        file_name: &str,
        cdn_domain: &str,
        cdn_key_pair_id: &str,
        cdn_private_key: &str,
        ttl_seconds: u64,
    ) -> anyhow::Result<Response> {
        let bucket = Self::bucket(bucket_name).await?;

        let object_key = match path {
            Some(p) => {
                if !p.is_empty() {
                    format!("{}/{}", p, file_name)
                } else {
                    file_name.to_string()
                }
            }
            None => file_name.to_string(),
        };

        let (head_object, _code) = bucket.head_object(&object_key.clone()).await?;

        // Get the values, handling Options
        let content_length = head_object
            .content_length
            .ok_or_else(|| anyhow::anyhow!("Content-Length missing"))?;

        let etag = head_object
            .e_tag
            .ok_or_else(|| anyhow::anyhow!("ETag missing"))?;

        let cloudfront_url = format!("{}/package-download/{}", cdn_domain, object_key);

        // Generate CDN signed URL
        let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH)?;

        let options = SignedOptions {
            key_pair_id: Cow::from(cdn_key_pair_id.to_string()),
            private_key: Cow::from(cdn_private_key.to_string()),
            date_less_than: since_epoch.as_secs() + ttl_seconds,
            ..Default::default()
        };

        let signed_url = get_signed_url(&cloudfront_url, &options)?;

        let response = axum::response::Response::builder()
            .header(axum::http::header::LOCATION, signed_url)
            .header("X-File-Size", content_length)
            .header(axum::http::header::ETAG, etag)
            .body(axum::body::Body::empty())
            .map_err(anyhow::Error::from)?;

        Ok(response)
    }

    /// Reuses one shared HTTPS client pool and cached AWS credentials so S3 calls
    /// do not rebuild TLS contexts or block Tokio on ECS metadata lookups.
    pub(crate) async fn bucket(bucket_name: &str) -> anyhow::Result<Box<Bucket>> {
        let region = Region::from_default_env()?;
        Self::bucket_in_region(bucket_name, region).await
    }

    pub(crate) async fn bucket_in_region(
        bucket_name: &str,
        region: Region,
    ) -> anyhow::Result<Box<Bucket>> {
        Self::bucket_with_min_remaining(bucket_name, region, CREDENTIAL_REFRESH_MARGIN_SECS).await
    }

    async fn bucket_with_min_remaining(
        bucket_name: &str,
        region: Region,
        min_remaining_secs: i64,
    ) -> anyhow::Result<Box<Bucket>> {
        let credentials = cached_credentials(min_remaining_secs).await?;
        Self::bucket_with_credentials(bucket_name, region, credentials).await
    }

    async fn bucket_with_credentials(
        bucket_name: &str,
        region: Region,
        credentials: Credentials,
    ) -> anyhow::Result<Box<Bucket>> {
        let mut bucket = template_bucket().await?.clone();
        bucket.name = bucket_name.to_owned();
        bucket.region = region;
        bucket.set_credentials(credentials);
        Ok(bucket)
    }

    /// Opens a multipart upload and returns its id. Content type and
    /// disposition are fixed here rather than signed into the part URLs: a
    /// signed header has to be echoed back byte-identically by the client or S3
    /// rejects the part with `SignatureDoesNotMatch`, which is a miserable way
    /// to lose an upload that is already 18 GB in. The extra header rides on a
    /// handle used for this one call, so it never reaches `presign_upload_parts`.
    ///
    /// Both stick to the finished object for its lifetime -- S3 has no way to
    /// edit them in place, only to copy the object over itself -- so they are
    /// the one chance to describe the image correctly.
    pub async fn initiate_multipart(
        bucket_name: &str,
        object_key: &str,
        content_type: &str,
        content_disposition: &str,
    ) -> anyhow::Result<String> {
        let bucket = Self::bucket(bucket_name).await?;
        Self::initiate_bucket_multipart(bucket, object_key, content_type, content_disposition, None)
            .await
    }

    pub async fn initiate_crc_multipart(
        bucket_name: &str,
        object_key: &str,
        content_type: &str,
        content_disposition: &str,
        crc64nvme: &str,
    ) -> anyhow::Result<String> {
        let bucket = Self::bucket(bucket_name).await?;
        Self::initiate_bucket_multipart(
            bucket,
            object_key,
            content_type,
            content_disposition,
            Some(crc64nvme),
        )
        .await
    }

    async fn initiate_bucket_multipart(
        mut bucket: Box<Bucket>,
        object_key: &str,
        content_type: &str,
        content_disposition: &str,
        crc64nvme: Option<&str>,
    ) -> anyhow::Result<String> {
        // `add_header` unwraps whatever it cannot parse, so the value is checked
        // here instead of trusted: a header value is visible ASCII plus space.
        // `validate_file_name` already rejects anything else, and this keeps a
        // future caller from turning a bad name into a panic in the api.
        if !content_disposition
            .bytes()
            .all(|b| (0x20..=0x7e).contains(&b))
        {
            anyhow::bail!("Content-Disposition is not encodable as a header value");
        }

        if let Some(crc64nvme) = crc64nvme {
            ensure_base64(crc64nvme)?;
        }

        bucket.add_header("content-disposition", content_disposition);
        if crc64nvme.is_some() {
            bucket.add_header(CHECKSUM_ALGORITHM_HEADER, CRC64NVME);
            bucket.add_header(CHECKSUM_TYPE_HEADER, FULL_OBJECT);
        }

        let response = bucket
            .initiate_multipart_upload(object_key, content_type)
            .await?;
        Ok(response.upload_id)
    }

    /// Presigns an `UploadPart` request per part number. The client PUTs each
    /// chunk straight at its URL, so the bytes never transit the api.
    ///
    /// The bucket is built once for the whole batch on purpose: a 20 GiB image
    /// is a couple of hundred parts, and `Credentials::default()` reaches for
    /// IMDS when the process has no credentials in its environment. Signing
    /// itself is local, so one handle turns a few hundred possible round trips
    /// into zero.
    pub async fn presign_upload_parts(
        bucket_name: &str,
        object_key: &str,
        upload_id: &str,
        part_numbers: &[i32],
        expiry_secs: u32,
    ) -> anyhow::Result<Vec<(i32, String)>> {
        let min_remaining = i64::from(expiry_secs)
            .saturating_add(60)
            .max(CREDENTIAL_REFRESH_MARGIN_SECS);
        let region = Region::from_default_env()?;
        let bucket = Self::bucket_with_min_remaining(bucket_name, region, min_remaining).await?;
        let parts: Vec<(i32, Option<u64>)> = part_numbers.iter().map(|n| (*n, None)).collect();
        Self::presign_parts(&bucket, object_key, upload_id, &parts, expiry_secs).await
    }

    /// Presigns `UploadPart` requests for the host a CloudFront S3 origin
    /// sends, `<bucket>.s3.<region>.amazonaws.com`. SigV4 signs the host, and
    /// rust-s3 uses the legacy `s3-<region>` host for some regions. Each URL
    /// also signs the part length, so S3 refuses a body of another length.
    /// A URL stops working when the signing credentials expire, so the
    /// lifetime is at most the time the credentials have left.
    pub async fn presign_regional_upload_parts(
        bucket_name: &str,
        object_key: &str,
        upload_id: &str,
        parts: &[(i32, u64)],
        expiry_secs: u32,
    ) -> anyhow::Result<Vec<(i32, String)>> {
        let region = match std::env::var("AWS_ENDPOINT") {
            Ok(_) => Region::from_default_env()?,
            Err(_) => regional(std::env::var("AWS_REGION")?),
        };
        let min_remaining = i64::from(expiry_secs)
            .saturating_add(60)
            .max(CREDENTIAL_REFRESH_MARGIN_SECS);
        let credentials = cached_credentials(min_remaining).await?;
        let now = unix_now_secs()?;
        let expiry_secs = url_lifetime(
            expiry_secs,
            credentials
                .expiration
                .as_ref()
                .map(|at| at.unix_timestamp() - now),
        )?;
        let bucket = Self::bucket_with_credentials(bucket_name, region, credentials).await?;
        let parts: Vec<(i32, Option<u64>)> = parts
            .iter()
            .map(|(number, length)| (*number, Some(*length)))
            .collect();
        Self::presign_parts(&bucket, object_key, upload_id, &parts, expiry_secs).await
    }

    async fn presign_parts(
        bucket: &Bucket,
        object_key: &str,
        upload_id: &str,
        parts: &[(i32, Option<u64>)],
        expiry_secs: u32,
    ) -> anyhow::Result<Vec<(i32, String)>> {
        let mut urls = Vec::with_capacity(parts.len());
        for (part_number, length) in parts {
            let mut queries = HashMap::new();
            queries.insert("partNumber".to_string(), part_number.to_string());
            queries.insert("uploadId".to_string(), upload_id.to_string());
            let headers = match length {
                Some(length) => {
                    let mut headers = http02::HeaderMap::new();
                    headers.insert(http02::header::CONTENT_LENGTH, (*length).into());
                    Some(headers)
                }
                None => None,
            };

            let url = bucket
                .presign_put(object_key, expiry_secs, headers, Some(queries))
                .await?;
            urls.push((*part_number, url));
        }

        Ok(urls)
    }

    /// Assembles the parts into the final object. `parts` must be ordered by
    /// part number and cover the upload with no gaps.
    pub async fn complete_multipart(
        bucket_name: &str,
        object_key: &str,
        upload_id: &str,
        parts: Vec<Part>,
    ) -> anyhow::Result<()> {
        let bucket = Self::bucket(bucket_name).await?;
        Self::complete_bucket_multipart(bucket, object_key, upload_id, parts, None).await
    }

    pub async fn complete_crc_multipart(
        bucket_name: &str,
        object_key: &str,
        upload_id: &str,
        parts: Vec<Part>,
        crc64nvme: &str,
    ) -> anyhow::Result<()> {
        let bucket = Self::bucket(bucket_name).await?;
        Self::complete_bucket_multipart(bucket, object_key, upload_id, parts, Some(crc64nvme)).await
    }

    async fn complete_bucket_multipart(
        mut bucket: Box<Bucket>,
        object_key: &str,
        upload_id: &str,
        parts: Vec<Part>,
        crc64nvme: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Some(crc64nvme) = crc64nvme {
            ensure_base64(crc64nvme)?;
            bucket.add_header(CRC64NVME_CHECKSUM_HEADER, crc64nvme);
            bucket.add_header(CHECKSUM_TYPE_HEADER, FULL_OBJECT);
        }
        let completed: anyhow::Result<()> = async {
            let response = bucket
                .complete_multipart_upload(object_key, upload_id, parts)
                .await?;
            validate_completion(response.status_code(), response.as_str()?)
        }
        .await;

        if let Err(refused) = &completed
            && is_bad_digest(refused)
            && let Err(abort) = bucket.abort_upload(object_key, upload_id).await
        {
            error!(
                error = redacted(&anyhow::Error::from(abort)),
                "Failed to abort an upload S3 refused as BadDigest"
            );
        }

        completed
    }

    /// Keeps other parts when S3 rejects this part. rust-s3's chunk helper aborts them.
    pub async fn upload_session_part(
        bucket_name: &str,
        object_key: &str,
        upload_id: &str,
        part_number: i32,
        body: reqwest::Body,
        size: u64,
    ) -> anyhow::Result<String> {
        let urls =
            Self::presign_upload_parts(bucket_name, object_key, upload_id, &[part_number], 900)
                .await?;
        let (_, url) = urls
            .first()
            .ok_or_else(|| anyhow::anyhow!("Missing part URL"))?;
        put_presigned_part(url, body, size).await
    }

    pub async fn list_upload_parts(
        bucket_name: &str,
        object_key: &str,
        upload_id: &str,
    ) -> anyhow::Result<Vec<UploadPart>> {
        let bucket = Self::bucket(bucket_name).await?;
        let queries = HashMap::from([
            ("uploadId".to_owned(), upload_id.to_owned()),
            ("max-parts".to_owned(), "1000".to_owned()),
        ]);
        let url = bucket.presign_get(object_key, 900, Some(queries)).await?;
        let response = list_parts_client()
            .await?
            .get(url)
            .send()
            .await
            .map_err(|e| e.without_url())?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|e| e.without_url())?;
        if missing_upload_body(&body) {
            return Err(s3::error::S3Error::HttpFailWithBody(
                status,
                "<Error><Code>NoSuchUpload</Code></Error>".into(),
            )
            .into());
        }
        anyhow::ensure!(status == 200, "S3 rejected part listing ({status})");
        let listed: ListedUploadParts = quick_xml::de::from_str(&body)?;
        anyhow::ensure!(
            !listed.is_truncated,
            "S3 part list exceeds the permitted upload size"
        );
        Ok(listed
            .parts
            .into_iter()
            .map(|p| UploadPart {
                part_number: p.part_number,
                etag: p.etag,
                size_bytes: p.size_bytes,
            })
            .collect())
    }

    /// Checks the bytes, including recovery after a lost completion reply.
    pub async fn matches_object(
        bucket_name: &str,
        object_key: &str,
        size: i64,
        checksum: &str,
    ) -> anyhow::Result<bool> {
        let bucket = Self::bucket(bucket_name).await?;
        Self::matches_bucket_object(&bucket, object_key, size, checksum).await
    }

    pub async fn matches_crc_object(
        bucket_name: &str,
        object_key: &str,
        size: i64,
        crc64nvme: &str,
    ) -> anyhow::Result<bool> {
        let bucket = Self::bucket(bucket_name).await?;
        Self::matches_crc_bucket_object(&bucket, object_key, size, crc64nvme).await
    }

    async fn matches_crc_bucket_object(
        bucket: &Bucket,
        object_key: &str,
        size: i64,
        crc64nvme: &str,
    ) -> anyhow::Result<bool> {
        use s3::request::Request;
        let mut bucket = bucket.clone();
        bucket.add_header(CHECKSUM_MODE_HEADER, CHECKSUM_MODE_ENABLED);
        let request = s3::request::tokio_backend::HyperRequest::new(
            &bucket,
            object_key,
            s3::command::Command::HeadObject,
        )
        .await?;
        let (headers, status) = match request.response_header().await {
            Ok(response) => response,
            Err(s3::error::S3Error::HttpFailWithBody(404, _)) => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        if status == 404 {
            return Ok(false);
        }
        anyhow::ensure!(status == 200, "S3 object metadata read failed");

        let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
        let length: Option<i64> = header("content-length").and_then(|value| value.parse().ok());
        Ok(length == Some(size) && header(CRC64NVME_CHECKSUM_HEADER) == Some(crc64nvme))
    }

    async fn matches_bucket_object(
        bucket: &Bucket,
        object_key: &str,
        size: i64,
        checksum: &str,
    ) -> anyhow::Result<bool> {
        use futures::StreamExt;
        use sha2::{Digest, Sha256};
        let (head, status) = match bucket.head_object(object_key).await {
            Ok(response) => response,
            Err(s3::error::S3Error::HttpFailWithBody(404, _)) => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        if status == 404 {
            return Ok(false);
        }
        anyhow::ensure!(status == 200, "S3 object metadata read failed");
        if head.content_length != Some(size) {
            return Ok(false);
        }
        // Each check reads and hashes the whole object, up to 512 MB, so few run at once.
        static OBJECT_CHECKS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(16);
        let _permit = OBJECT_CHECKS.acquire().await?;
        let mut response = match bucket.get_object_stream(object_key).await {
            Ok(response) => response,
            Err(s3::error::S3Error::HttpFailWithBody(404, _)) => return Ok(false),
            Err(e) => return Err(e.into()),
        };
        if response.status_code == 404 {
            return Ok(false);
        }
        anyhow::ensure!(response.status_code == 200, "S3 object read failed");
        let mut digest = Sha256::new();
        let mut received = 0i64;
        while let Some(chunk) = response.bytes().next().await {
            let chunk = chunk?;
            received += chunk.len() as i64;
            if received > size {
                return Ok(false);
            }
            digest.update(&chunk);
        }
        Ok(received == size && format!("{:x}", digest.finalize()) == checksum)
    }

    /// Discards an upload and the parts already stored for it. Uncompleted
    /// parts are billed until this runs.
    pub async fn abort_multipart(
        bucket_name: &str,
        object_key: &str,
        upload_id: &str,
    ) -> anyhow::Result<()> {
        let bucket = Self::bucket(bucket_name).await?;
        bucket.abort_upload(object_key, upload_id).await?;
        Ok(())
    }

    /// Size of a stored object, used to check an assembled upload against the
    /// size its client declared.
    pub async fn object_size(bucket_name: &str, object_key: &str) -> anyhow::Result<i64> {
        let bucket = Self::bucket(bucket_name).await?;
        let (head, _code) = bucket.head_object(object_key).await?;
        head.content_length
            .ok_or_else(|| anyhow::anyhow!("Content-Length missing for {object_key}"))
    }
}

struct CachedCredentials {
    credentials: Credentials,
    fetched_at: Instant,
}

struct CredentialCache {
    cached: RwLock<Option<CachedCredentials>>,
    refresh_lock: Mutex<()>,
}

impl CredentialCache {
    const fn new() -> Self {
        Self {
            cached: RwLock::const_new(None),
            refresh_lock: Mutex::const_new(()),
        }
    }

    async fn get_or_refresh<F, Fut>(
        &self,
        min_remaining_secs: i64,
        now_unix: i64,
        now_instant: Instant,
        load: F,
    ) -> anyhow::Result<Credentials>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<Credentials>>,
    {
        if let Some(entry) = self.cached.read().await.as_ref()
            && !needs_refresh(entry, min_remaining_secs, now_unix, now_instant)
        {
            return Ok(entry.credentials.clone());
        }

        let _guard = self.refresh_lock.lock().await;
        if let Some(entry) = self.cached.read().await.as_ref()
            && !needs_refresh(entry, min_remaining_secs, now_unix, now_instant)
        {
            return Ok(entry.credentials.clone());
        }

        match load().await {
            Ok(credentials) => {
                let mut guard = self.cached.write().await;
                *guard = Some(CachedCredentials {
                    credentials: credentials.clone(),
                    fetched_at: now_instant,
                });
                Ok(credentials)
            }
            Err(err) => {
                let mut guard = self.cached.write().await;
                if let Some(entry) = guard.as_mut()
                    && credentials_still_valid(&entry.credentials, now_unix)
                {
                    entry.fetched_at = now_instant;
                    warn!(
                        error = %err,
                        "AWS credential refresh failed; continuing with valid cached credentials"
                    );
                    return Ok(entry.credentials.clone());
                }
                Err(err)
            }
        }
    }
}

fn needs_refresh(
    entry: &CachedCredentials,
    min_remaining_secs: i64,
    now_unix: i64,
    now_instant: Instant,
) -> bool {
    let Some(expiration) = entry.credentials.expiration.as_ref() else {
        return false;
    };
    let remaining = expiration.unix_timestamp() - now_unix;
    if remaining <= 0 {
        return true;
    }
    remaining < min_remaining_secs
        && now_instant.saturating_duration_since(entry.fetched_at) >= CREDENTIAL_REFRESH_COOLDOWN
}

fn credentials_still_valid(credentials: &Credentials, now_unix: i64) -> bool {
    match credentials.expiration.as_ref() {
        Some(expiration) => expiration.unix_timestamp() > now_unix,
        None => true,
    }
}

fn unix_now_secs() -> anyhow::Result<i64> {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    Ok(i64::try_from(secs).unwrap_or(i64::MAX))
}

async fn template_bucket() -> anyhow::Result<&'static Box<Bucket>> {
    static TEMPLATE: OnceCell<Box<Bucket>> = OnceCell::const_new();
    TEMPLATE
        .get_or_try_init(|| async {
            tokio::task::spawn_blocking(|| {
                Bucket::new("", Region::UsEast1, Credentials::anonymous()?)
            })
            .await
            .map_err(|err| anyhow::anyhow!("S3 client init task failed: {err}"))?
            .map_err(anyhow::Error::from)
        })
        .await
}

async fn load_default_credentials() -> anyhow::Result<Credentials> {
    tokio::task::spawn_blocking(Credentials::default)
        .await
        .map_err(|err| anyhow::anyhow!("AWS credential lookup task failed: {err}"))?
        .map_err(anyhow::Error::from)
}

async fn cached_credentials(min_remaining_secs: i64) -> anyhow::Result<Credentials> {
    static CREDENTIALS: CredentialCache = CredentialCache::new();
    let now_unix = unix_now_secs()?;
    CREDENTIALS
        .get_or_refresh(
            min_remaining_secs,
            now_unix,
            Instant::now(),
            load_default_credentials,
        )
        .await
}

async fn list_parts_client() -> anyhow::Result<&'static reqwest::Client> {
    static CLIENT: OnceCell<reqwest::Client> = OnceCell::const_new();
    CLIENT
        .get_or_try_init(|| async {
            tokio::task::spawn_blocking(|| {
                reqwest::Client::builder()
                    .timeout(Duration::from_secs(30))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
            })
            .await
            .map_err(|err| anyhow::anyhow!("S3 HTTP client init task failed: {err}"))?
            .map_err(anyhow::Error::from)
        })
        .await
}

async fn upload_part_client() -> anyhow::Result<&'static reqwest::Client> {
    static CLIENT: OnceCell<reqwest::Client> = OnceCell::const_new();
    CLIENT
        .get_or_try_init(|| async {
            tokio::task::spawn_blocking(|| {
                reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(10))
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
            })
            .await
            .map_err(|err| anyhow::anyhow!("S3 upload HTTP client init task failed: {err}"))?
            .map_err(anyhow::Error::from)
        })
        .await
}

pub(crate) async fn put_presigned_part(
    url: &str,
    body: reqwest::Body,
    size: u64,
) -> anyhow::Result<String> {
    let response = upload_part_client()
        .await?
        .put(url)
        .header("content-length", size)
        .body(body)
        .send()
        .await
        .map_err(|e| e.without_url())?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|e| e.without_url())?;
        let code = s3_error_code(&body);
        if let Some(code) = code.as_deref()
            && matches!(code, NO_SUCH_UPLOAD | REQUEST_TIMEOUT)
        {
            return Err(s3::error::S3Error::HttpFailWithBody(
                status,
                format!("<Error><Code>{code}</Code></Error>"),
            )
            .into());
        }
        match code.as_deref().filter(|code| !code.is_empty()) {
            Some(code) => anyhow::bail!("S3 rejected upload part ({status} {code})"),
            None => anyhow::bail!("S3 rejected upload part ({status})"),
        }
    }
    Ok(response
        .headers()
        .get("etag")
        .ok_or_else(|| anyhow::anyhow!("S3 part has no ETag"))?
        .to_str()?
        .to_owned())
}

/// Seconds a presigned URL can live: `wanted`, but at most the time the
/// signing credentials have left, less one minute.
fn url_lifetime(wanted: u32, credentials_left: Option<i64>) -> anyhow::Result<u32> {
    let Some(left) = credentials_left else {
        return Ok(wanted);
    };
    let left = left - 60;
    anyhow::ensure!(
        left >= 60,
        "Signing credentials expire in less than two minutes"
    );
    Ok(wanted.min(u32::try_from(left).unwrap_or(u32::MAX)))
}

fn regional(region: String) -> Region {
    Region::Custom {
        endpoint: format!("https://s3.{region}.amazonaws.com"),
        region,
    }
}

pub fn is_missing_upload(error: &anyhow::Error) -> bool {
    has_s3_error_code(error, NO_SUCH_UPLOAD)
}

pub fn is_bad_digest(error: &anyhow::Error) -> bool {
    has_s3_error_code(error, BAD_DIGEST)
}

pub fn is_request_timeout(error: &anyhow::Error) -> bool {
    has_s3_error_code(error, REQUEST_TIMEOUT)
}

fn has_s3_error_code(error: &anyhow::Error, expected: &str) -> bool {
    error
        .downcast_ref::<s3::error::S3Error>()
        .is_some_and(|error| {
            matches!(
                error,
                s3::error::S3Error::HttpFailWithBody(_, body)
                    if s3_error_code(body).as_deref() == Some(expected)
            )
        })
}

fn missing_upload_body(body: &str) -> bool {
    s3_error_code(body).as_deref() == Some(NO_SUCH_UPLOAD)
}

fn s3_error_code(body: &str) -> Option<String> {
    let mut reader = quick_xml::Reader::from_str(body);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(e)) if e.local_name().as_ref() == b"Code" => {
                // Keeps the code safe to log.
                return reader
                    .read_text(e.name())
                    .ok()
                    .map(|code| code.chars().filter(char::is_ascii_alphanumeric).collect());
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

// S3 error bodies can echo signed request details.
pub fn redacted(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(|cause| match cause.downcast_ref::<s3::error::S3Error>() {
            Some(s3::error::S3Error::HttpFailWithBody(status, body)) => format!(
                "S3 HTTP {status} {}",
                s3_error_code(body).unwrap_or_default()
            ),
            _ => cause.to_string(),
        })
        .collect::<Vec<_>>()
        .join(": ")
}

fn ensure_base64(value: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !value.is_empty()
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b)),
        "checksum is not base64"
    );
    Ok(())
}

fn validate_completion(status: u16, body: &str) -> anyhow::Result<()> {
    if s3_error_code(body).is_some() {
        return Err(s3::error::S3Error::HttpFailWithBody(status, body.to_owned()).into());
    }
    anyhow::ensure!(
        (200..300).contains(&status),
        "S3 completion status {status}"
    );
    let mut reader = quick_xml::Reader::from_str(body);
    loop {
        match reader.read_event()? {
            quick_xml::events::Event::Start(e) => {
                anyhow::ensure!(
                    e.local_name().as_ref() == b"CompleteMultipartUploadResult",
                    "S3 completion error body"
                );
                return Ok(());
            }
            quick_xml::events::Event::Eof => anyhow::bail!("S3 completion body is empty"),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Bucket, Credentials, HashMap, Part, Storage, UploadMetrics, redacted, regional,
        url_lifetime, validate_completion,
    };

    #[test]
    fn redacted_drops_s3_error_body() {
        let body =
            "<Error><Code>SignatureDoesNotMatch</Code><StringToSign>secret</StringToSign></Error>";
        let error = anyhow::Error::from(s3::error::S3Error::HttpFailWithBody(403, body.into()))
            .context("part 1");
        assert_eq!(
            redacted(&error),
            "part 1: S3 HTTP 403 SignatureDoesNotMatch"
        );
    }

    #[tokio::test]
    async fn put_presigned_part_classifies_request_timeout_and_missing_upload() -> anyhow::Result<()>
    {
        for (status, body, is_timeout, is_missing, expected_msg) in [
            (
                400,
                "<Error><Code>RequestTimeout</Code><Message>Your socket connection to the server was not read from or written to within the timeout period.</Message></Error>",
                true,
                false,
                None,
            ),
            (
                404,
                "<Error><Code>NoSuchUpload</Code></Error>",
                false,
                true,
                None,
            ),
            (
                500,
                "<Error><Code>InternalError</Code></Error>",
                false,
                false,
                Some("S3 rejected upload part (500 InternalError)"),
            ),
            (
                500,
                "gateway failure",
                false,
                false,
                Some("S3 rejected upload part (500)"),
            ),
        ] {
            let (bucket, _seen, server) = fake_s3(status, vec![], body).await?;
            let url = format!("{}/object", bucket.url());
            let result = super::put_presigned_part(&url, reqwest::Body::from("abc"), 3).await;
            server.abort();
            let Err(error) = result else {
                anyhow::bail!("non-2xx S3 response must fail");
            };
            assert_eq!(super::is_request_timeout(&error), is_timeout, "{body}");
            assert_eq!(super::is_missing_upload(&error), is_missing, "{body}");
            if let Some(expected_msg) = expected_msg {
                assert_eq!(error.to_string(), expected_msg, "{body}");
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn object_verification_checks_metadata_before_reading_bytes() -> anyhow::Result<()> {
        use axum::{
            Router,
            body::Body,
            http::{Method, Request, StatusCode},
            response::Response,
            routing::any,
        };
        use sha2::{Digest, Sha256};
        use std::sync::{Arc, Mutex};
        for (head_status, declared_size, checksum_matches, expected) in [
            (200, 4, true, Some(false)),
            (404, 5, true, Some(false)),
            (503, 5, true, None),
            (200, 5, true, Some(true)),
            (200, 5, false, Some(false)),
        ] {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let seen = calls.clone();
            let app = Router::new().fallback(any(move |request: Request<Body>| {
                let seen = seen.clone();
                async move {
                    let method = request.method().clone();
                    seen.lock().unwrap().push(method.clone());
                    let status = if method == Method::HEAD {
                        head_status
                    } else {
                        200
                    };
                    Response::builder()
                        .status(status)
                        .header("content-length", "5")
                        .body(Body::from("bytes"))
                        .unwrap()
                }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let endpoint = format!("http://{}", listener.local_addr()?);
            let server = tokio::spawn(async move { axum::serve(listener, app).await });
            let bucket = s3::Bucket::new(
                "bucket",
                s3::Region::Custom {
                    region: "test".into(),
                    endpoint,
                },
                s3::creds::Credentials::new(Some("test"), Some("test"), None, None, None)?,
            )?
            .with_path_style();
            let checksum = if checksum_matches {
                format!("{:x}", Sha256::digest(b"bytes"))
            } else {
                "a".repeat(64)
            };
            let result =
                super::Storage::matches_bucket_object(&bucket, "object", declared_size, &checksum)
                    .await;
            server.abort();
            match expected {
                Some(value) => assert_eq!(result?, value),
                None => assert!(result.is_err()),
            }
            let expected_calls = if head_status == StatusCode::OK.as_u16() && declared_size == 5 {
                vec![Method::HEAD, Method::GET]
            } else {
                vec![Method::HEAD]
            };
            assert_eq!(*calls.lock().unwrap(), expected_calls);
        }
        Ok(())
    }

    const CRC: &str = "zM77/A2gEEE=";

    type Seen = std::sync::Arc<std::sync::Mutex<Vec<(axum::http::Method, axum::http::HeaderMap)>>>;

    async fn fake_s3(
        status: u16,
        headers: Vec<(&'static str, &'static str)>,
        body: &'static str,
    ) -> anyhow::Result<(
        Box<Bucket>,
        Seen,
        tokio::task::JoinHandle<std::io::Result<()>>,
    )> {
        use axum::{Router, body::Body, http::Request, response::Response, routing::any};
        let seen = Seen::default();
        let record = seen.clone();
        let app = Router::new().fallback(any(move |request: Request<Body>| {
            let record = record.clone();
            let headers = headers.clone();
            async move {
                record
                    .lock()
                    .unwrap()
                    .push((request.method().clone(), request.headers().clone()));
                let mut response = Response::builder().status(status);
                for (name, value) in headers {
                    response = response.header(name, value);
                }
                response.body(Body::from(body)).unwrap()
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        let bucket = Bucket::new(
            "bucket",
            s3::Region::Custom {
                region: "test".into(),
                endpoint,
            },
            Credentials::new(Some("test"), Some("test"), None, None, None)?,
        )?
        .with_path_style();
        Ok((bucket, seen, server))
    }

    fn header(seen: &Seen, index: usize, name: &str) -> Option<String> {
        let seen = seen.lock().unwrap();
        seen.get(index)?
            .1
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    }

    #[tokio::test]
    async fn crc_upload_asks_s3_for_a_full_object_checksum() -> anyhow::Result<()> {
        const INITIATED: &str = "<InitiateMultipartUploadResult><Bucket>bucket</Bucket><Key>object</Key><UploadId>up</UploadId></InitiateMultipartUploadResult>";
        for crc in [Some(CRC), None] {
            let (bucket, seen, server) = fake_s3(200, vec![], INITIATED).await?;
            let upload_id = Storage::initiate_bucket_multipart(
                bucket,
                "object",
                "application/octet-stream",
                "attachment",
                crc,
            )
            .await;
            server.abort();
            assert_eq!(upload_id?, "up");
            let expected = |value: &str| crc.map(|_| value.to_string());
            assert_eq!(
                header(&seen, 0, "x-amz-checksum-algorithm"),
                expected("CRC64NVME")
            );
            assert_eq!(
                header(&seen, 0, "x-amz-checksum-type"),
                expected("FULL_OBJECT")
            );
            assert_eq!(
                header(&seen, 0, "x-amz-meta-crc64nvme"),
                None,
                "the device's claim is not stored on the object; only S3's checksum is trusted"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn crc_completion_sends_the_object_checksum() -> anyhow::Result<()> {
        const COMPLETED: &str =
            "<CompleteMultipartUploadResult><ETag>\"e\"</ETag></CompleteMultipartUploadResult>";
        let (bucket, seen, server) = fake_s3(200, vec![], COMPLETED).await?;
        let parts = vec![Part {
            part_number: 1,
            etag: "\"p\"".into(),
        }];
        let result =
            Storage::complete_bucket_multipart(bucket, "object", "up", parts, Some(CRC)).await;
        server.abort();
        result?;
        assert_eq!(
            header(&seen, 0, "x-amz-checksum-crc64nvme").as_deref(),
            Some(CRC)
        );
        assert_eq!(
            header(&seen, 0, "x-amz-checksum-type").as_deref(),
            Some("FULL_OBJECT")
        );
        Ok(())
    }

    #[tokio::test]
    async fn crc_mismatch_is_a_bad_digest_and_ends_the_upload() -> anyhow::Result<()> {
        use axum::http::Method;
        const BAD_DIGEST: &str = "<Error><Code>BadDigest</Code><Message>The crc64nvme you specified did not match the calculated checksum.</Message></Error>";
        let (bucket, seen, server) = fake_s3(400, vec![], BAD_DIGEST).await?;
        let result =
            Storage::complete_bucket_multipart(bucket, "object", "up", vec![], Some(CRC)).await;
        server.abort();
        let Err(error) = result else {
            anyhow::bail!("S3 refused the completion, so it must fail");
        };
        assert!(super::is_bad_digest(&error));
        assert!(!super::is_missing_upload(&error));
        let methods: Vec<Method> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(m, _)| m.clone())
            .collect();
        assert_eq!(
            methods,
            [Method::POST, Method::DELETE],
            "a refused upload is aborted, so the device's retry gets NoSuchUpload and starts over"
        );
        Ok(())
    }

    #[tokio::test]
    async fn crc_object_check_compares_the_checksum_s3_computed() -> anyhow::Result<()> {
        use axum::http::Method;
        const OTHER: &str = "AAAAAAAAAAA=";
        for (status, computed, claimed, size, expected) in [
            (200, Some(CRC), None, 5, Some(true)),
            (200, Some(OTHER), Some(CRC), 5, Some(false)),
            (200, None, Some(CRC), 5, Some(false)),
            (200, Some(CRC), None, 4, Some(false)),
            (404, None, None, 5, Some(false)),
            (503, None, None, 5, None),
        ] {
            let mut headers = vec![("content-length", "5")];
            if let Some(computed) = computed {
                headers.push(("x-amz-checksum-crc64nvme", computed));
            }
            if let Some(claimed) = claimed {
                headers.push(("x-amz-meta-crc64nvme", claimed));
            }
            let (bucket, seen, server) = fake_s3(status, headers, "").await?;
            let result = Storage::matches_crc_bucket_object(&bucket, "object", size, CRC).await;
            server.abort();
            match expected {
                Some(value) => {
                    assert_eq!(result?, value, "{status} {computed:?} {claimed:?} {size}")
                }
                None => assert!(result.is_err()),
            }
            let methods: Vec<Method> = seen
                .lock()
                .unwrap()
                .iter()
                .map(|(m, _)| m.clone())
                .collect();
            assert_eq!(methods, [Method::HEAD]);
            assert_eq!(
                header(&seen, 0, "x-amz-checksum-mode").as_deref(),
                Some("ENABLED")
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn bad_digest_inside_a_successful_completion_is_a_bad_digest() -> anyhow::Result<()> {
        use axum::http::Method;
        const EMBEDDED: &str = "<?xml version=\"1.0\"?>\n<Error><Code>BadDigest</Code></Error>";
        let (bucket, seen, server) = fake_s3(200, vec![], EMBEDDED).await?;
        let result =
            Storage::complete_bucket_multipart(bucket, "object", "up", vec![], Some(CRC)).await;
        server.abort();
        let Err(error) = result else {
            anyhow::bail!("an error body means S3 did not complete the upload");
        };
        assert!(super::is_bad_digest(&error));
        let methods: Vec<Method> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(m, _)| m.clone())
            .collect();
        assert_eq!(methods, [Method::POST, Method::DELETE]);
        Ok(())
    }

    #[tokio::test]
    async fn crc_that_is_not_base64_is_refused_before_any_request() -> anyhow::Result<()> {
        let (bucket, seen, server) = fake_s3(200, vec![], "").await?;
        let result = Storage::initiate_bucket_multipart(
            bucket,
            "object",
            "application/octet-stream",
            "attachment",
            Some("zM77/A2g\nEEE="),
        )
        .await;
        server.abort();
        assert!(result.is_err());
        assert!(seen.lock().unwrap().is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn regional_presign_signs_the_cloudfront_origin_host() -> anyhow::Result<()> {
        let credentials = Credentials::new(Some("AKIDEXAMPLE"), Some("secret"), None, None, None)?;
        let bucket = Bucket::new("assets-55c9d7e", regional("eu-north-1".into()), credentials)?;
        let urls =
            Storage::presign_parts(&bucket, "clips/one.mp4", "upload", &[(3, Some(1234))], 60)
                .await?;
        let (_, url) = urls
            .first()
            .ok_or_else(|| anyhow::anyhow!("No presigned URL"))?;
        let url = reqwest::Url::parse(url)?;
        assert_eq!(
            url.host_str(),
            Some("assets-55c9d7e.s3.eu-north-1.amazonaws.com")
        );
        assert_eq!(url.path(), "/clips/one.mp4");
        let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query.get("partNumber").map(String::as_str), Some("3"));
        assert_eq!(query.get("uploadId").map(String::as_str), Some("upload"));
        assert_eq!(
            query.get("X-Amz-SignedHeaders").map(String::as_str),
            Some("content-length;host")
        );
        assert!(
            query
                .get("X-Amz-Credential")
                .is_some_and(|c| c.contains("/eu-north-1/s3/"))
        );
        Ok(())
    }

    #[test]
    fn url_lifetime_ends_before_the_credentials() -> anyhow::Result<()> {
        assert_eq!(url_lifetime(3600, None)?, 3600);
        assert_eq!(url_lifetime(3600, Some(6 * 3600))?, 3600);
        assert_eq!(url_lifetime(3600, Some(1000))?, 940);
        assert!(url_lifetime(3600, Some(100)).is_err());
        Ok(())
    }

    fn test_credentials(
        access_key: &str,
        expires_at_unix: Option<i64>,
    ) -> anyhow::Result<Credentials> {
        let mut creds = Credentials::new(Some(access_key), Some("secret"), None, None, None)?;
        if let Some(ts) = expires_at_unix {
            let rfc3339 = chrono::DateTime::from_timestamp(ts, 0)
                .ok_or_else(|| anyhow::anyhow!("invalid test timestamp {ts}"))?
                .to_rfc3339();
            creds.expiration = Some(serde_json::from_str(&format!("\"{rfc3339}\""))?);
        }
        Ok(creds)
    }

    #[tokio::test]
    async fn buckets_share_one_http_client_pool_with_isolated_bucket_state() -> anyhow::Result<()> {
        let first = Storage::bucket_with_credentials(
            "assets",
            regional("eu-north-1".into()),
            test_credentials("AKIAFIRST", None)?,
        )
        .await?;
        let mut second = Storage::bucket_with_credentials(
            "packages",
            s3::Region::UsEast1,
            test_credentials("AKIASECOND", None)?,
        )
        .await?;
        second.add_header("x-test", "value");

        assert!(std::sync::Arc::ptr_eq(
            &first.http_client(),
            &second.http_client()
        ));
        assert_eq!(first.name(), "assets");
        assert_eq!(second.name(), "packages");
        assert_eq!(first.access_key().await?.as_deref(), Some("AKIAFIRST"));
        assert_eq!(second.access_key().await?.as_deref(), Some("AKIASECOND"));
        assert!(first.extra_headers().is_empty());
        assert_eq!(second.extra_headers().len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn credential_cache_reuses_and_proactively_refreshes_expiring_credentials()
    -> anyhow::Result<()> {
        use super::{CREDENTIAL_REFRESH_COOLDOWN, CredentialCache};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{Duration, Instant};

        let cache = CredentialCache::new();
        let loads = AtomicUsize::new(0);
        let t0 = Instant::now();
        let base_unix = 1_700_000_000i64;

        // Initial load fetches credentials expiring in 1 hour.
        let first = cache
            .get_or_refresh(300, base_unix, t0, || async {
                loads.fetch_add(1, Ordering::SeqCst);
                test_credentials("AKIA1", Some(base_unix + 3600))
            })
            .await?;
        assert_eq!(first.access_key.as_deref(), Some("AKIA1"));
        assert_eq!(loads.load(Ordering::SeqCst), 1);

        // Fresh credentials are served from cache without calling the loader.
        let cached = cache
            .get_or_refresh(
                300,
                base_unix + 1000,
                t0 + Duration::from_secs(1000),
                || async {
                    loads.fetch_add(1, Ordering::SeqCst);
                    test_credentials("AKIA_UNUSED", Some(base_unix + 7200))
                },
            )
            .await?;
        assert_eq!(cached.access_key.as_deref(), Some("AKIA1"));
        assert_eq!(loads.load(Ordering::SeqCst), 1);

        // When remaining time drops below min_remaining_secs after cooldown, refresh runs.
        let t_near = t0 + Duration::from_secs(3400);
        let refreshed = cache
            .get_or_refresh(300, base_unix + 3400, t_near, || async {
                loads.fetch_add(1, Ordering::SeqCst);
                // Simulate metadata not rotated yet (still 200s left).
                test_credentials("AKIA2", Some(base_unix + 3600))
            })
            .await?;
        assert_eq!(refreshed.access_key.as_deref(), Some("AKIA2"));
        assert_eq!(loads.load(Ordering::SeqCst), 2);

        // Within cooldown, still-valid credentials are reused instead of hammering metadata.
        let during_cooldown = cache
            .get_or_refresh(
                300,
                base_unix + 3410,
                t_near + Duration::from_secs(10),
                || async {
                    loads.fetch_add(1, Ordering::SeqCst);
                    test_credentials("AKIA_UNUSED2", Some(base_unix + 7200))
                },
            )
            .await?;
        assert_eq!(during_cooldown.access_key.as_deref(), Some("AKIA2"));
        assert_eq!(loads.load(Ordering::SeqCst), 2);

        // A transient refresh error after cooldown falls back to still-valid credentials.
        let t_after_cooldown = t_near + CREDENTIAL_REFRESH_COOLDOWN + Duration::from_secs(1);
        let fallback = cache
            .get_or_refresh(300, base_unix + 3470, t_after_cooldown, || async {
                loads.fetch_add(1, Ordering::SeqCst);
                anyhow::bail!("metadata timeout")
            })
            .await?;
        assert_eq!(fallback.access_key.as_deref(), Some("AKIA2"));
        assert_eq!(loads.load(Ordering::SeqCst), 3);

        // Once truly expired, refresh runs immediately even inside cooldown.
        let expired_refresh = cache
            .get_or_refresh(
                300,
                base_unix + 3600,
                t_after_cooldown + Duration::from_secs(5),
                || async {
                    loads.fetch_add(1, Ordering::SeqCst);
                    test_credentials("AKIA3", Some(base_unix + 10_000))
                },
            )
            .await?;
        assert_eq!(expired_refresh.access_key.as_deref(), Some("AKIA3"));
        assert_eq!(loads.load(Ordering::SeqCst), 4);
        Ok(())
    }

    #[tokio::test]
    async fn credential_cache_coalesces_concurrent_refreshes() -> anyhow::Result<()> {
        use super::CredentialCache;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{Duration, Instant};

        let cache = Arc::new(CredentialCache::new());
        let loads = Arc::new(AtomicUsize::new(0));
        let now = Instant::now();
        let now_unix = 1_700_000_000i64;

        let mut tasks = Vec::new();
        for _ in 0..16 {
            let cache = cache.clone();
            let loads = loads.clone();
            tasks.push(tokio::spawn(async move {
                cache
                    .get_or_refresh(300, now_unix, now, || async move {
                        loads.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        test_credentials("AKIA_SINGLE", Some(now_unix + 3600))
                    })
                    .await
            }));
        }
        for task in tasks {
            let creds = task.await??;
            assert_eq!(creds.access_key.as_deref(), Some("AKIA_SINGLE"));
        }
        assert_eq!(loads.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[test]
    fn completion_rejects_http_and_embedded_errors() {
        assert!(validate_completion(200, "<?xml version=\"1.0\"?>\n<CompleteMultipartUploadResult><ETag>ok</ETag></CompleteMultipartUploadResult>").is_ok());
        for status in [200, 404] {
            let error = validate_completion(status, "<Error><Code>NoSuchUpload</Code></Error>")
                .unwrap_err();
            assert!(super::is_missing_upload(&error));
        }
        for (status, body) in [
            (500, "<Error/>"),
            (200, " \n<Error><Code>InternalError</Code></Error>"),
            (200, "<Error />"),
            (200, ""),
        ] {
            assert!(validate_completion(status, body).is_err());
        }
    }
    use metrics_util::{DebugValue, DebuggingRecorder};

    #[test]
    fn upload_metrics_record_one_outcome_per_upload() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        recorder.install().expect("no other recorder in tests");

        // (name, result at drop, bytes, histogram sample expected)
        let cases = [
            ("ok", "ok", 42u64, Some(42.0)),
            ("s3 error", "error", 7, None),
            ("client error", "client", 7, None),
        ];
        for (name, result, bytes, want_sample) in cases {
            let mut guard = UploadMetrics::start();
            guard.bytes = bytes;
            guard.result = result;
            drop(guard);

            let (mut gauge, mut counter, mut samples) = (None, None, Vec::new());
            for (key, _, _, value) in snapshotter.snapshot().into_vec() {
                let (key, name) = (key.key().clone(), key.key().name().to_string());
                match (name.as_str(), value) {
                    ("smith_uploads_in_flight", DebugValue::Gauge(g)) => {
                        gauge = Some(g.into_inner())
                    }
                    ("smith_uploads_total", DebugValue::Counter(c))
                        if key.labels().any(|l| l.value() == result) =>
                    {
                        counter = Some(c)
                    }
                    ("smith_upload_bytes", DebugValue::Histogram(h)) => {
                        samples.extend(h.into_iter().map(|v| v.into_inner()))
                    }
                    _ => {}
                }
            }
            assert_eq!(gauge, Some(0.0), "{name}: gauge");
            assert_eq!(counter, Some(1), "{name}: counter");
            assert_eq!(
                samples,
                want_sample.into_iter().collect::<Vec<_>>(),
                "{name}: histogram"
            );
        }
    }
}
