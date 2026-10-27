use super::*;
use crate::{
    auth::{DebugJwksClient, DeviceJwtSigner},
    config::{CloudFrontConfig, Config},
    middlewares::authorization::{AccountsConfig, AuthorizationConfig},
};
use axum::{
    Router,
    routing::{delete, post, put},
};
use ed25519_dalek::pkcs8::EncodePrivateKey;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, Semaphore, broadcast};

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
    Router::new()
        .route("/smith/upload-sessions", post(open))
        .route(
            "/smith/upload-sessions/:id/parts/:number",
            put(put_part).layer(axum::extract::DefaultBodyLimit::max(PART_SIZE as usize)),
        )
        .route("/smith/upload-sessions/:id/complete", post(complete))
        .route("/smith/upload-sessions/:id", delete(abort))
        .layer(Extension(state))
}

#[tokio::test]
#[ignore = "Requires task-local Postgres and S3; see api/tests/resumable-uploads.md"]
async fn durable_routes_verify_bytes_binding_completion_and_cleanup() -> anyhow::Result<()> {
    let pool = PgPool::connect(&std::env::var("UPLOAD_TEST_DATABASE_URL")?).await?;
    let state = test_state(pool.clone())?;
    let token = state.device_jwt_signer.mint(1, "one")?;
    let foreign = state.device_jwt_signer.mint(2, "two")?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}/smith/upload-sessions", listener.local_addr()?);
    let app = router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let client = reqwest::Client::new();
    let bytes: Vec<u8> = (0..PART_SIZE + 29).map(|n| (n % 251) as u8).collect();
    let checksum = format!("{:x}", Sha256::digest(&bytes));
    for sensitive in [false, true] {
        let key = format!("clips/{}.mp4", Uuid::new_v4());
        let identity = serde_json::json!({"key":key,"size_bytes":bytes.len(),"checksum":checksum,"sensitive":sensitive});
        let response = client.post(&base).json(&identity).send().await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = client
            .post(&base)
            .bearer_auth(&token)
            .json(&identity)
            .send()
            .await?;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{}",
            response.text().await?
        );
        let opened: UploadPlan = client
            .post(&base)
            .bearer_auth(&token)
            .json(&identity)
            .send()
            .await?
            .json()
            .await?;
        let session_url = format!("{base}/{}", opened.session_id);
        assert_eq!(opened.missing_parts, vec![1, 2]);
        for (method, suffix) in [
            (reqwest::Method::DELETE, ""),
            (reqwest::Method::POST, "/complete"),
            (reqwest::Method::PUT, "/parts/1"),
        ] {
            let response = client
                .request(method, format!("{session_url}{suffix}"))
                .bearer_auth(&foreign)
                .header(
                    "x-content-sha256",
                    format!("{:x}", Sha256::digest(&bytes[..PART_SIZE as usize])),
                )
                .body(bytes[..PART_SIZE as usize].to_vec())
                .send()
                .await?;
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
        let first = &bytes[..PART_SIZE as usize];
        let put = |number, data: Vec<u8>| {
            client
                .put(format!("{session_url}/parts/{number}"))
                .bearer_auth(&token)
                .header("x-content-sha256", format!("{:x}", Sha256::digest(&data)))
                .body(data)
        };
        let response = put(1, first.to_vec()).send().await?;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{}",
            response.text().await?
        );
        let resumed: UploadPlan = client
            .post(&base)
            .bearer_auth(&token)
            .json(&identity)
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(resumed.session_id, opened.session_id);
        assert_eq!(resumed.missing_parts, vec![2]);
        assert_eq!(resumed.confirmed_parts.len(), 1);
        assert_eq!(
            client
                .post(format!("{session_url}/complete"))
                .bearer_auth(&token)
                .send()
                .await?
                .status(),
            StatusCode::CONFLICT
        );
        let response = put(2, bytes[PART_SIZE as usize..].to_vec()).send().await?;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{}",
            response.text().await?
        );
        let bucket = if sensitive {
            "upload-sensitive"
        } else {
            "upload-assets"
        };
        // Covers a lost S3 completion reply before the database receipt.
        if sensitive {
            let mut tx = pool.begin().await?;
            let session = owned_session(&mut tx, opened.session_id, 1)
                .await
                .map_err(|s| anyhow::anyhow!(s))?;
            let uploaded = parts(&mut tx, opened.session_id)
                .await
                .map_err(|s| anyhow::anyhow!(s))?;
            Storage::complete_multipart(
                bucket,
                &key,
                session
                    .upload_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("No upload ID"))?,
                uploaded
                    .into_iter()
                    .map(|p| s3::serde_types::Part {
                        part_number: p.part_number as u32,
                        etag: p.etag,
                    })
                    .collect(),
            )
            .await?;
            tx.rollback().await?;
        }
        for _ in 0..2 {
            let response = client
                .post(format!("{session_url}/complete"))
                .bearer_auth(&token)
                .send()
                .await?;
            assert_eq!(response.status(), StatusCode::OK);
            let result: serde_json::Value = response.json().await?;
            assert_eq!(result["url"], format!("s3://{bucket}/{key}"));
        }
        assert_eq!(Storage::download_from_s3(bucket, &key).await?, bytes);
        let receipt: UploadPlan = client
            .post(&base)
            .bearer_auth(&token)
            .json(&identity)
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(receipt.url, Some(format!("s3://{bucket}/{key}")));
    }
    let invalid: UploadPlan = client.post(&base).bearer_auth(&token).json(&serde_json::json!({
        "key":format!("bad-checksum/{}",Uuid::new_v4()), "size_bytes":3, "checksum":"0".repeat(64), "sensitive":false,
    })).send().await?.json().await?;
    let response = client
        .put(format!("{base}/{}/parts/1", invalid.session_id))
        .bearer_auth(&token)
        .header("x-content-sha256", format!("{:x}", Sha256::digest(b"abc")))
        .body("abc")
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let response = client
        .post(format!("{base}/{}/complete", invalid.session_id))
        .bearer_auth(&token)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let completed: bool = sqlx::query_scalar(
        "SELECT completed_at IS NOT NULL FROM device_upload_session WHERE id=$1",
    )
    .bind(invalid.session_id)
    .fetch_one(&pool)
    .await?;
    assert!(!completed);
    let key = format!("abandoned/{}", Uuid::new_v4());
    let identity = serde_json::json!({"key":key,"size_bytes":bytes.len(),"checksum":checksum,"sensitive":true});
    let abandoned: UploadPlan = client
        .post(&base)
        .bearer_auth(&token)
        .json(&identity)
        .send()
        .await?
        .json()
        .await?;
    let mut tx = pool.begin().await?;
    let session = owned_session(&mut tx, abandoned.session_id, 1)
        .await
        .map_err(|s| anyhow::anyhow!(s))?;
    let upload_id = session
        .upload_id
        .ok_or_else(|| anyhow::anyhow!("No upload ID"))?;
    tx.rollback().await?;
    let response = client
        .put(format!("{base}/{}/parts/1", abandoned.session_id))
        .bearer_auth(&token)
        .header(
            "x-content-sha256",
            format!("{:x}", Sha256::digest(&bytes[..PART_SIZE as usize])),
        )
        .body(bytes[..PART_SIZE as usize].to_vec())
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    sqlx::query("UPDATE device_upload_session SET touched_at=now()-interval '15 days' WHERE id=$1")
        .bind(abandoned.session_id)
        .execute(&pool)
        .await?;
    sweep(&pool).await.map_err(|s| anyhow::anyhow!(s))?;
    let remaining: i64 =
        sqlx::query_scalar("SELECT count(*) FROM device_upload_session WHERE id=$1")
            .bind(abandoned.session_id)
            .fetch_one(&pool)
            .await?;
    assert_eq!(remaining, 0);
    assert!(
        Storage::upload_session_part(
            "upload-sensitive",
            &key,
            &upload_id,
            1,
            bytes[..PART_SIZE as usize].to_vec()
        )
        .await
        .is_err()
    );
    server.abort();
    Ok(())
}

#[tokio::test]
#[ignore = "Requires Postgres, S3, and the Plex test binary; see api/tests/resumable-uploads.md"]
async fn plex_contract_resumes_across_real_smith_routes() -> anyhow::Result<()> {
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
    let app = router(state).layer(middleware::from_fn(
        move |request: axum::extract::Request, next: middleware::Next| {
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
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
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
    let base = format!("http://{}/smith", listener.local_addr()?);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let key = format!("contract/{}.mp4", Uuid::new_v4());
    let output = tokio::time::timeout(
        Duration::from_secs(180),
        tokio::process::Command::new(std::env::var("UPLOAD_TEST_PLEX_BINARY")?)
            .args([
                "--ignored",
                "--exact",
                "server::upload::resume_tests::real_smith_upload",
                "--nocapture",
            ])
            .env("UPLOAD_TEST_SMITH_URL", base)
            .env("UPLOAD_TEST_KEY", &key)
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(*attempts.lock().await, vec![1, 2, 2]);
    let expected: Vec<u8> = (0..PART_SIZE + 27).map(|n| (n % 251) as u8).collect();
    assert_eq!(
        Storage::download_from_s3("upload-sensitive", &key).await?,
        expected
    );
    let stored: (String,String,Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT bucket,object_key,completed_at FROM device_upload_session WHERE device_id=1 AND object_key=$1",
    ).bind(&key).fetch_one(&pool).await?;
    assert_eq!(stored.0, "upload-sensitive");
    assert_eq!(stored.1, key);
    assert!(stored.2.is_some());
    server.abort();
    Ok(())
}
