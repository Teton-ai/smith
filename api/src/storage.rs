use axum::response::Response;
use cloudfront_sign::{SignedOptions, get_signed_url};
use s3::bucket::CHUNK_SIZE;
use s3::creds::Credentials;
use s3::serde_types::Part;
use s3::utils::read_chunk_async;
use s3::{Bucket, Region};
use std::borrow::Cow;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::error;

/// Lifetime of a package's CloudFront URL. Packages are small enough that an
/// hour comfortably covers a download plus retries.
pub const PACKAGE_SIGNED_URL_TTL_SECONDS: u64 = 60 * 60;

/// Lifetime of an OS image's CloudFront URL. These run to tens of gigabytes and
/// devices pull them over whatever link they have, so the URL has to outlive a
/// slow transfer -- smithd reuses the same one when it resumes.
pub const OS_SIGNED_URL_TTL_SECONDS: u64 = 24 * 60 * 60;

pub struct Storage;

impl Storage {
    pub async fn save_to_s3(
        bucket_name: &str,
        path: Option<&str>,
        file_name: &str,
        data: &[u8],
    ) -> anyhow::Result<()> {
        let region = Region::from_default_env()?;
        let credentials = Credentials::default()?;
        let bucket = Bucket::new(bucket_name, region, credentials)?;

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
    ) -> anyhow::Result<u16>
    where
        R: tokio::io::AsyncRead + Unpin + ?Sized,
    {
        const CONTENT_TYPE: &str = "application/octet-stream";

        let bucket = Self::bucket(bucket_name)?;

        // Anything that fits in one chunk skips multipart entirely.
        let first_chunk = read_chunk_async(reader).await?;
        if first_chunk.len() < CHUNK_SIZE {
            let response = bucket
                .put_object_with_content_type(object_key, &first_chunk, CONTENT_TYPE)
                .await?;
            return Ok(response.status_code());
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
                chunk = read_chunk_async(reader).await?;
                // An exact multiple of CHUNK_SIZE has no trailing part to send.
                if chunk.is_empty() {
                    break;
                }
            }

            let response = bucket
                .complete_multipart_upload(object_key, &upload_id, parts)
                .await?;
            // A failed complete must be an Err so the abort below runs.
            let status = response.status_code();
            if !(200..300).contains(&status) {
                anyhow::bail!(
                    "CompleteMultipartUpload for {object_key} failed with status {status}"
                )
            }
            // S3 can embed the real error in a 200 body instead of the status.
            let body = response.as_str()?;
            if body.contains("<Error>") {
                anyhow::bail!(
                    "CompleteMultipartUpload for {object_key} returned an error body: {}",
                    body.chars().take(200).collect::<String>()
                )
            }
            Ok::<u16, anyhow::Error>(status)
        }
        .await;

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
        let region = Region::from_default_env()?;
        let credentials = Credentials::default()?;
        let bucket = Bucket::new(bucket_name, region, credentials)?;
        bucket.delete_object(path).await?;
        Ok(())
    }

    pub async fn download_from_s3(bucket_name: &str, file_name: &str) -> anyhow::Result<Vec<u8>> {
        let region = Region::from_default_env()?;
        let credentials = Credentials::default()?;
        let bucket = Bucket::new(bucket_name, region, credentials)?;
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
        let region = Region::from_default_env()?;
        let credentials = Credentials::default()?;
        let bucket = Bucket::new(bucket_name, region, credentials)?;

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

    /// A `Bucket` is a config handle rather than a connection, so building one
    /// per call is free and avoids threading state through every helper.
    fn bucket(bucket_name: &str) -> anyhow::Result<Box<Bucket>> {
        let region = Region::from_default_env()?;
        let credentials = Credentials::default()?;
        Ok(Bucket::new(bucket_name, region, credentials)?)
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

        let mut bucket = Self::bucket(bucket_name)?;
        bucket.add_header("content-disposition", content_disposition);

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
        let bucket = Self::bucket(bucket_name)?;

        let mut urls = Vec::with_capacity(part_numbers.len());
        for part_number in part_numbers {
            let mut queries = HashMap::new();
            queries.insert("partNumber".to_string(), part_number.to_string());
            queries.insert("uploadId".to_string(), upload_id.to_string());

            let url = bucket
                .presign_put(object_key, expiry_secs, None, Some(queries))
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
        let bucket = Self::bucket(bucket_name)?;
        let response = bucket
            .complete_multipart_upload(object_key, upload_id, parts)
            .await?;

        // `complete_multipart_upload` hands back the response without checking
        // it, and S3 reports some failures here with a 200 body, so the status
        // is inspected rather than assumed.
        let status = response.status_code();
        if !(200..300).contains(&status) {
            anyhow::bail!("CompleteMultipartUpload for {object_key} failed with status {status}");
        }

        Ok(())
    }

    /// Discards an upload and the parts already stored for it. Uncompleted
    /// parts are billed until this runs.
    pub async fn abort_multipart(
        bucket_name: &str,
        object_key: &str,
        upload_id: &str,
    ) -> anyhow::Result<()> {
        let bucket = Self::bucket(bucket_name)?;
        bucket.abort_upload(object_key, upload_id).await?;
        Ok(())
    }

    /// Size of a stored object, used to check an assembled upload against the
    /// size its client declared.
    pub async fn object_size(bucket_name: &str, object_key: &str) -> anyhow::Result<i64> {
        let bucket = Self::bucket(bucket_name)?;
        let (head, _code) = bucket.head_object(object_key).await?;
        head.content_length
            .ok_or_else(|| anyhow::anyhow!("Content-Length missing for {object_key}"))
    }
}
