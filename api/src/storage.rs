use axum::response::Response;
use s3::creds::Credentials;
use s3::{Bucket, Region};

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
    pub async fn delete_from_s3(bucket_name: &str, path: &str) -> anyhow::Result<()> {
        let region = Region::from_default_env()?;
        let credentials = Credentials::default()?;
        let bucket = Bucket::new(bucket_name, region, credentials)?;
        bucket.delete_object(path).await?;
        Ok(())
    }
    pub async fn download_from_s3(
        bucket_name: &str,
        path: Option<&str>,
        file_name: &str,
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

        let pre_signed_url = bucket
            .presign_get(
                object_key, 151200, // 48 hours
                None,
            )
            .await?;

        // Create a response with the location header
        let response = axum::response::Response::builder()
            .header(axum::http::header::LOCATION, pre_signed_url)
            .body(axum::body::Body::empty())
            .map_err(anyhow::Error::from)?;

        Ok(response)
    }
    pub async fn head_from_s3(
        bucket_name: &str,
        path: Option<&str>,
        file_name: &str,
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

        let (head_object, _code) = bucket.head_object(&object_key).await?;

        // Get the values, handling Options
        let content_length = head_object
            .content_length
            .ok_or_else(|| anyhow::anyhow!("Content-Length missing"))?;

        let etag = head_object
            .e_tag
            .ok_or_else(|| anyhow::anyhow!("ETag missing"))?;

        // Create a response with the location header
        let response = axum::response::Response::builder()
            .header(axum::http::header::CONTENT_LENGTH, content_length)
            .header(axum::http::header::ETAG, etag)
            .body(axum::body::Body::empty())
            .map_err(anyhow::Error::from)?;

        Ok(response)
    }
}
