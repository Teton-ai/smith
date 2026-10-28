use super::*;
use crate::{
    auth::{DebugJwksClient, DeviceJwtSigner},
    config::{CloudFrontConfig, Config},
    middlewares::authorization::{AccountsConfig, AuthorizationConfig},
};
use axum::{
    Router,
    routing::{get, post, put},
};
use ed25519_dalek::pkcs8::EncodePrivateKey;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{Mutex, Semaphore, broadcast};
use uuid::Uuid;

fn test_state(pool: PgPool) -> anyhow::Result<State> {
    let pem = ed25519_dalek::SigningKey::from_bytes(&[7; 32]).to_pkcs8_pem(Default::default())?;
    let signer = DeviceJwtSigner::new(pem.as_str(), "upload-test".into(), 3600)?;
    let config = Box::leak(Box::new(Config {
        database_url: String::new(),
        packages_bucket_name: "packages".into(),
        assets_bucket_name: "upload-assets".into(),
        data_engine_bucket_name: "upload-sensitive".into(),
        aws_region: "us-east-1".into(),
        sentry_url: None,
        slack_hook_url: None,
        deployment_slack_hook_url: None,
        victoria_metrics_client: None,
        victoria_metrics_read_client: None,
        ip_api_key: None,
        auth0_issuer: "http://127.0.0.1".into(),
        auth0_audience: "upload-test".into(),
        cloudfront: CloudFrontConfig {
            package_domain_name: String::new(),
            package_key_pair_id: String::new(),
            package_private_key: String::new(),
        },
        dashboard_excluded_labels: vec![],
        device_jwt_private_key_pem: pem.to_string(),
        device_jwt_issuer: "upload-test".into(),
        device_jwt_ttl_seconds: 3600,
        known_holders: HashMap::new(),
        network_gc_interval_seconds: 3600,
        mcp: None,
        max_concurrent_uploads: 4,
        upload_token_key: crate::upload_token::UploadTokenKey::new(&"test-key".repeat(8))?,
    }));
    Ok(State {
        pg_pool: pool,
        config,
        public_events: Arc::new(Mutex::new(broadcast::channel(1).0)),
        authorization: Arc::new(AuthorizationConfig {
            roles: HashMap::new(),
        }),
        accounts: Arc::new(AccountsConfig::default()),
        jwks_client: DebugJwksClient::init("http://127.0.0.1")
            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        device_jwt_signer: signer,
        upload_permits: Arc::new(Semaphore::new(4)),
    })
}

fn router(state: State) -> Router {
    router_at(state, "/smith")
}

fn router_at(state: State, prefix: &str) -> Router {
    Router::new()
        .route(&format!("{prefix}/upload-sessions"), post(open))
        .route(&format!("{prefix}/upload-sessions/status"), get(status))
        .route(
            &format!("{prefix}/upload-sessions/parts/:number"),
            put(put_part),
        )
        .route(
            &format!("{prefix}/upload-sessions/complete"),
            post(complete),
        )
        .layer(Extension(state))
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    device_token: &str,
    token: Option<&str>,
    body: Vec<u8>,
) -> anyhow::Result<axum::response::Response> {
    use tower::ServiceExt;
    let mut request = axum::http::Request::builder().method(method).uri(path);
    if !device_token.is_empty() {
        request = request.header("authorization", format!("Bearer {device_token}"));
    }
    if let Some(token) = token {
        let mut value = axum::http::HeaderValue::from_str(token)?;
        value.set_sensitive(true);
        request = request.header("x-upload-token", value);
    }
    request = request
        .header("content-length", body.len())
        .header("content-type", "application/json");
    Ok(app
        .clone()
        .oneshot(request.body(axum::body::Body::from(body))?)
        .await?)
}

async fn response_json<T: serde::de::DeserializeOwned>(
    response: axum::response::Response,
) -> anyhow::Result<T> {
    anyhow::ensure!(
        response.status().is_success(),
        "Unexpected route status: {}",
        response.status()
    );
    let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn open_upload(
    app: &Router,
    credential: &str,
    identity: &serde_json::Value,
) -> anyhow::Result<OpenedUpload> {
    response_json(
        call(
            app,
            "POST",
            "/smith/upload-sessions",
            credential,
            None,
            serde_json::to_vec(identity)?,
        )
        .await?,
    )
    .await
}

async fn upload_plan(app: &Router, credential: &str, token: &str) -> anyhow::Result<UploadPlan> {
    response_json(
        call(
            app,
            "GET",
            "/smith/upload-sessions/status",
            credential,
            Some(token),
            vec![],
        )
        .await?,
    )
    .await
}

#[tokio::test]
#[ignore = "Requires isolated Postgres and S3; see api/tests/resumable-uploads.md"]
async fn stateless_routes_verify_tokens_parts_and_completion() -> anyhow::Result<()> {
    let pool = PgPool::connect(&std::env::var("UPLOAD_TEST_DATABASE_URL")?).await?;
    let state = test_state(pool)?;
    let credential = state.device_jwt_signer.mint(1, "one")?;
    let foreign = state.device_jwt_signer.mint(2, "two")?;
    let app = router(state.clone());
    let base = "/smith/upload-sessions";
    let capacity = state.upload_permits.acquire_many(4).await?;
    let response = call(&app, "POST", base, &credential, None, b"{}".to_vec()).await?;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let identity =
        serde_json::json!({"key":"capacity/test", "size_bytes":3,"checksum":"a".repeat(64)});
    let response = call(
        &app,
        "POST",
        base,
        &credential,
        None,
        serde_json::to_vec(&identity)?,
    )
    .await?;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["retry-after"], "5");
    drop(capacity);
    let bytes: Vec<u8> = (0..PART_SIZE + 29).map(|n| (n % 251) as u8).collect();
    let checksum = format!("{:x}", Sha256::digest(&bytes));
    for sensitive in [false, true] {
        let key = format!("clips/{}.mp4", uuid::Uuid::new_v4());
        let identity = serde_json::json!({"key":key,"size_bytes":bytes.len(),"checksum":checksum,"sensitive":sensitive});
        assert_eq!(
            call(&app, "POST", base, "", None, serde_json::to_vec(&identity)?)
                .await?
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let opened = open_upload(&app, &credential, &identity).await?;
        assert!(!opened.upload_id.is_empty());
        assert_eq!(opened.part_size, PART_SIZE);
        let listed = upload_plan(&app, &credential, &opened.token).await?;
        assert_eq!(listed.missing_parts, vec![1, 2]);
        assert!(listed.confirmed_parts.is_empty());
        for (method, suffix) in [
            ("GET", "/status"),
            ("POST", "/complete"),
            ("PUT", "/parts/1"),
        ] {
            assert_eq!(
                call(
                    &app,
                    method,
                    &format!("{base}{suffix}"),
                    &foreign,
                    Some(&opened.token),
                    bytes[..PART_SIZE as usize].to_vec()
                )
                .await?
                .status(),
                StatusCode::FORBIDDEN
            );
        }
        assert_eq!(
            call(
                &app,
                "GET",
                &format!("{base}/status"),
                &credential,
                Some("not-a-token"),
                vec![]
            )
            .await?
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(
                &app,
                "PUT",
                &format!("{base}/parts/1"),
                &credential,
                Some(&opened.token),
                vec![1]
            )
            .await?
            .status(),
            StatusCode::BAD_REQUEST
        );
        let first: UploadPart = response_json(
            call(
                &app,
                "PUT",
                &format!("{base}/parts/1"),
                &credential,
                Some(&opened.token),
                bytes[..PART_SIZE as usize].to_vec(),
            )
            .await?,
        )
        .await?;
        assert_eq!(first.part_number, 1);
        assert_eq!(first.size_bytes, PART_SIZE);
        let listed = upload_plan(&app, &credential, &opened.token).await?;
        assert_eq!(listed.missing_parts, vec![2]);
        assert_eq!(listed.confirmed_parts.len(), 1);
        assert_eq!(listed.confirmed_parts[0].etag, first.etag);
        assert_eq!(
            call(
                &app,
                "POST",
                &format!("{base}/complete"),
                &credential,
                Some(&opened.token),
                vec![]
            )
            .await?
            .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            call(
                &app,
                "PUT",
                &format!("{base}/parts/2"),
                &credential,
                Some(&opened.token),
                bytes[PART_SIZE as usize..].to_vec()
            )
            .await?
            .status(),
            StatusCode::OK
        );
        let bucket = if sensitive {
            "upload-sensitive"
        } else {
            "upload-assets"
        };
        // Covers a lost storage completion reply.
        if sensitive {
            let parts = Storage::list_upload_parts(bucket, &key, &opened.upload_id).await?;
            Storage::complete_multipart(
                bucket,
                &key,
                &opened.upload_id,
                parts
                    .into_iter()
                    .map(|p| s3::serde_types::Part {
                        part_number: p.part_number as u32,
                        etag: p.etag,
                    })
                    .collect(),
            )
            .await?;
        }
        for _ in 0..2 {
            let result: serde_json::Value = response_json(
                call(
                    &app,
                    "POST",
                    &format!("{base}/complete"),
                    &credential,
                    Some(&opened.token),
                    vec![],
                )
                .await?,
            )
            .await?;
            assert_eq!(result["url"], format!("s3://{bucket}/{key}"));
        }
        assert_eq!(Storage::download_from_s3(bucket, &key).await?, bytes);
        let finished = upload_plan(&app, &credential, &opened.token).await?;
        assert_eq!(finished.url, Some(format!("s3://{bucket}/{key}")));
    }
    let body = b"replacement upload";
    for missing_on_complete in [false, true] {
        let key = format!("missing/{}", uuid::Uuid::new_v4());
        let identity = serde_json::json!({"key":key,"size_bytes":body.len(),"checksum":format!("{:x}",Sha256::digest(body))});
        let opened = open_upload(&app, &credential, &identity).await?;
        if missing_on_complete {
            assert_eq!(
                call(
                    &app,
                    "PUT",
                    &format!("{base}/parts/1"),
                    &credential,
                    Some(&opened.token),
                    body.to_vec()
                )
                .await?
                .status(),
                StatusCode::OK
            );
        }
        Storage::abort_multipart("upload-assets", &key, &opened.upload_id).await?;
        for (method, path, data) in [
            ("GET", "/status", vec![]),
            ("PUT", "/parts/1", body.to_vec()),
            ("POST", "/complete", vec![]),
        ] {
            let response = call(
                &app,
                method,
                &format!("{base}{path}"),
                &credential,
                Some(&opened.token),
                data,
            )
            .await?;
            assert_eq!(response.status(), StatusCode::GONE);
            let error: serde_json::Value =
                serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 1024).await?)?;
            assert_eq!(error["code"], "NoSuchUpload");
        }
        let fresh = open_upload(&app, &credential, &identity).await?;
        assert_ne!(fresh.upload_id, opened.upload_id);
        assert_eq!(
            upload_plan(&app, &credential, &fresh.token)
                .await?
                .missing_parts,
            vec![1]
        );
        assert_eq!(
            call(
                &app,
                "PUT",
                &format!("{base}/parts/1"),
                &credential,
                Some(&fresh.token),
                body.to_vec()
            )
            .await?
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            call(
                &app,
                "POST",
                &format!("{base}/complete"),
                &credential,
                Some(&fresh.token),
                vec![]
            )
            .await?
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            Storage::download_from_s3("upload-assets", &key).await?,
            body
        );
    }
    let bad = open_upload(&app,&credential,&serde_json::json!({"key":format!("checksum/{}",uuid::Uuid::new_v4()),"size_bytes":3,"checksum":"0".repeat(64)})).await?;
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("{base}/parts/1"),
            &credential,
            Some(&bad.token),
            b"abc".to_vec()
        )
        .await?
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{base}/complete"),
            &credential,
            Some(&bad.token),
            vec![]
        )
        .await?
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let expired = state.config.upload_token_key.sign(&UploadClaims {
        device_id: 1,
        bucket: "upload-assets".into(),
        key: "expired".into(),
        upload_id: "expired".into(),
        size_bytes: 3,
        checksum: "a".repeat(64),
        expires_at: chrono::Utc::now().timestamp() - 1,
    })?;
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("{base}/status"),
            &credential,
            Some(&expired),
            vec![]
        )
        .await?
        .status(),
        StatusCode::GONE
    );
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Postgres, S3, and an upload client executable; see api/tests/resumable-uploads.md"]
async fn client_contract_resumes_across_real_smith_routes() -> anyhow::Result<()> {
    use axum::{middleware, response::IntoResponse};
    use std::sync::atomic::{AtomicBool, Ordering};
    let pool = PgPool::connect(&std::env::var("UPLOAD_TEST_DATABASE_URL")?).await?;
    sqlx::query("UPDATE device SET token='test-device-jwt' WHERE id=1")
        .execute(&pool)
        .await?;
    let state = test_state(pool.clone())?;
    let attempts = Arc::new(Mutex::new(Vec::new()));
    let trace = attempts.clone();
    let fail_part = Arc::new(AtomicBool::new(true));
    let lose_complete = Arc::new(AtomicBool::new(true));
    let app = router_at(state, "").layer(middleware::from_fn(
        move |mut request: axum::extract::Request, next: middleware::Next| {
            let fail_part = fail_part.clone();
            let lose_complete = lose_complete.clone();
            let trace = trace.clone();
            async move {
                let path = request.uri().path().to_owned();
                if path.contains("/parts/")
                    && let Some(number) =
                        path.rsplit('/').next().and_then(|s| s.parse::<i32>().ok())
                {
                    trace.lock().await.push(number);
                }
                if path.ends_with("/parts/2") && fail_part.swap(false, Ordering::SeqCst) {
                    axum::body::to_bytes(request.into_body(), PART_SIZE as usize)
                        .await
                        .unwrap();
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
                request
                    .headers_mut()
                    .insert("authorization", "Bearer test-device-jwt".parse().unwrap());
                let response = next.run(request).await;
                if path.ends_with("/complete")
                    && response.status().is_success()
                    && lose_complete.swap(false, Ordering::SeqCst)
                {
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
                response
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let key = format!("contract/{}.mp4", Uuid::new_v4());
    let output = tokio::time::timeout(
        Duration::from_secs(180),
        tokio::process::Command::new(std::env::var("UPLOAD_TEST_CLIENT_BINARY")?)
            .env("UPLOAD_TEST_SMITH_URL", base)
            .env("UPLOAD_TEST_KEY", &key)
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    assert!(
        output.status.success(),
        "{}\n{}\nattempts: {:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        *attempts.lock().await
    );
    assert_eq!(*attempts.lock().await, vec![1, 2, 2]);
    let expected: Vec<u8> = (0..PART_SIZE + 27).map(|n| (n % 251) as u8).collect();
    assert_eq!(
        Storage::download_from_s3("upload-sensitive", &key).await?,
        expected
    );
    server.abort();
    Ok(())
}
