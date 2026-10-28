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
        metrics: &mut UploadMetrics,
    ) -> anyhow::Result<u16>
    where
        R: tokio::io::AsyncRead + Unpin + ?Sized,
    {
        const CONTENT_TYPE: &str = "application/octet-stream";

        let bucket = Self::bucket(bucket_name)?;

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

        validate_completion(response.status_code(), response.as_str()?)?;

        Ok(())
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
        let response = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?
            .put(url)
            .header("content-length", size)
            .body(body)
            .send()
            .await
            .map_err(|e| e.without_url())?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.map_err(|e| e.without_url())?;
            if missing_upload_body(&body) {
                return Err(s3::error::S3Error::HttpFailWithBody(
                    status,
                    "<Error><Code>NoSuchUpload</Code></Error>".into(),
                )
                .into());
            }
            anyhow::bail!("S3 rejected upload part ({status})");
        }
        Ok(response
            .headers()
            .get("etag")
            .ok_or_else(|| anyhow::anyhow!("S3 part has no ETag"))?
            .to_str()?
            .to_owned())
    }

    pub async fn list_upload_parts(
        bucket_name: &str,
        object_key: &str,
        upload_id: &str,
    ) -> anyhow::Result<Vec<UploadPart>> {
        let bucket = Self::bucket(bucket_name)?;
        let queries = HashMap::from([
            ("uploadId".to_owned(), upload_id.to_owned()),
            ("max-parts".to_owned(), "1000".to_owned()),
        ]);
        let url = bucket.presign_get(object_key, 900, Some(queries)).await?;
        let response = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()?
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
        let bucket = Self::bucket(bucket_name)?;
        Self::matches_bucket_object(&bucket, object_key, size, checksum).await
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

pub fn is_missing_upload(error: &anyhow::Error) -> bool {
    error.downcast_ref::<s3::error::S3Error>().is_some_and(|error| {
        matches!(error, s3::error::S3Error::HttpFailWithBody(_, body) if missing_upload_body(body))
    })
}

fn missing_upload_body(body: &str) -> bool {
    let mut reader = quick_xml::Reader::from_str(body);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(e)) if e.local_name().as_ref() == b"Code" => {
                return reader
                    .read_text(e.name())
                    .is_ok_and(|code| code == "NoSuchUpload");
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => return false,
            _ => {}
        }
    }
}

fn validate_completion(status: u16, body: &str) -> anyhow::Result<()> {
    if missing_upload_body(body) {
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
    use super::{UploadMetrics, validate_completion};

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
