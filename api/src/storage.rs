use axum::response::Response;
use cloudfront_sign::{SignedOptions, get_signed_url};
use s3::creds::Credentials;
use s3::serde_types::Part;
use s3::{Bucket, Region};
use std::borrow::Cow;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

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

    /// Stream a reader straight into S3 without ever holding the object in
    /// memory. `put_object_stream` uploads in multipart chunks, so a 512 MiB
    /// device file costs a chunk of RSS, not 512 MiB — unlike `save_to_s3`,
    /// which takes a fully-buffered slice.
    pub async fn stream_to_s3<R>(
        bucket_name: &str,
        object_key: &str,
        reader: &mut R,
    ) -> anyhow::Result<u16>
    where
        R: tokio::io::AsyncRead + Unpin + ?Sized,
    {
        let region = Region::from_default_env()?;
        let credentials = Credentials::default()?;
        let bucket = Bucket::new(bucket_name, region, credentials)?;

        let status = bucket.put_object_stream(reader, object_key).await?;
        Ok(status.status_code())
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

    /// Opens a multipart upload and returns its id. Content type is fixed here
    /// rather than signed into the part URLs: a signed `Content-Type` has to be
    /// echoed back byte-identically by the client or S3 rejects the part with
    /// `SignatureDoesNotMatch`, which is a miserable way to lose an upload that
    /// is already 18 GB in.
    pub async fn initiate_multipart(
        bucket_name: &str,
        object_key: &str,
        content_type: &str,
    ) -> anyhow::Result<String> {
        let bucket = Self::bucket(bucket_name)?;
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
