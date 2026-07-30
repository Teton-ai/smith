use axum::response::Response;
use cloudfront_sign::{SignedOptions, get_signed_url};
use s3::creds::Credentials;
use s3::{Bucket, Region};
use std::borrow::Cow;
use std::time::{SystemTime, UNIX_EPOCH};

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

    pub async fn download_package_from_cdn(
        bucket_name: &str,
        path: Option<&str>,
        file_name: &str,
        cdn_domain: &str,
        cdn_key_pair_id: &str,
        cdn_private_key: &str,
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
        let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

        let options = SignedOptions {
            key_pair_id: Cow::from(cdn_key_pair_id.to_string()),
            private_key: Cow::from(cdn_private_key.to_string()),
            date_less_than: since_epoch.as_secs() + (60 * 60), // 1 hour
            // date_less_than: expiration_timeout,
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
}
