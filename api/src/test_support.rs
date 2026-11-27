use crate::{
    State,
    auth::{DebugJwksClient, DeviceJwtSigner},
    config::{CloudFrontConfig, Config},
    middlewares::authorization::{AccountsConfig, AuthorizationConfig},
};
use ed25519_dalek::pkcs8::EncodePrivateKey;
use sqlx::PgPool;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, Semaphore, broadcast};

pub(crate) fn test_state(pool: PgPool) -> anyhow::Result<State> {
    test_state_with_cdn(pool, None)
}

pub(crate) fn test_state_with_cdn(pool: PgPool, cdn: Option<&str>) -> anyhow::Result<State> {
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
        device_upload_cdn_url: cdn.map(str::to_string),
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
        home_permits: Arc::new(Semaphore::new(4)),
        home_cache: Arc::new(crate::smith::HomeCache::new()),
    })
}
