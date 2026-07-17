use anyhow::Context;
use axum::http::HeaderMap;
use std::env;
use std::time::Duration;

#[derive(Debug)]
pub struct VictoriaMetricsClient {
    pub client: reqwest::Client,
    pub url: String,
}

impl VictoriaMetricsClient {
    fn from_env(url_var: &str, token_var: &str) -> anyhow::Result<Option<Self>> {
        match (env::var(url_var).ok(), env::var(token_var).ok()) {
            (Some(url), Some(auth_token)) => {
                let mut headers = HeaderMap::new();
                let auth = format!("Basic {}", auth_token);
                headers.insert(
                    "authorization",
                    auth.parse()
                        .context("built an invalid VictoriaMetrics authorization header")?,
                );
                let client = reqwest::Client::builder()
                    .default_headers(headers)
                    .timeout(Duration::from_secs(60))
                    .build()
                    .context("failed to build VictoriaMetrics HTTP client")?;
                Ok(Some(VictoriaMetricsClient { client, url }))
            }
            _ => Ok(None),
        }
    }

    pub fn from_env_all() -> anyhow::Result<Vec<Self>> {
        let mut clients = Vec::new();
        if let Some(client) = Self::from_env("VICTORIA_METRICS_URL", "VICTORIA_METRICS_AUTH_TOKEN")?
        {
            clients.push(client);
        }
        if let Some(client) = Self::from_env(
            "VICTORIA_METRICS_URL_FLEET",
            "VICTORIA_METRICS_AUTH_TOKEN_FLEET",
        )? {
            clients.push(client);
        }
        Ok(clients)
    }
}

#[derive(Debug)]
pub struct CloudFrontConfig {
    pub package_domain_name: String,
    pub package_key_pair_id: String,
    pub package_private_key: String,
}

impl CloudFrontConfig {
    pub fn new() -> anyhow::Result<CloudFrontConfig> {
        Ok(CloudFrontConfig {
            package_domain_name: env::var("CLOUDFRONT_DOMAIN_NAME")
                .context("CLOUDFRONT_PACKAGE_DOMAIN_NAME is required.")?,
            package_key_pair_id: env::var("CLOUDFRONT_PACKAGE_KEY_PAIR_ID")
                .context("CLOUDFRONT_PACKAGE_KEY_PAIR_ID is required.")?,
            package_private_key: env::var("CLOUDFRONT_PACKAGE_PRIVATE_KEY")
                .context("CLOUDFRONT_PACKAGE_PRIVATE_KEY is required.")?,
        })
    }
}

#[derive(Debug)]
pub struct Config {
    pub database_url: String,
    pub packages_bucket_name: String,
    pub assets_bucket_name: String,
    pub aws_region: String,
    pub sentry_url: Option<String>,
    pub slack_hook_url: Option<String>,
    pub deployment_slack_hook_url: Option<String>,
    pub victoria_metrics_clients: Vec<VictoriaMetricsClient>,
    pub ip_api_key: Option<String>,
    pub auth0_issuer: String,
    pub auth0_audience: String,
    pub cloudfront: CloudFrontConfig,
    /// Labels to exclude from dashboard stats, format: "key=value,key2=value2"
    pub dashboard_excluded_labels: Vec<String>,
    /// Ed25519 private key (PKCS8 PEM) used to sign device JWTs.
    /// Generate with: `openssl genpkey -algorithm Ed25519 -out device_jwt.pem`
    pub device_jwt_private_key_pem: String,
    /// `iss` claim written into device JWTs and verified on incoming JWTs.
    pub device_jwt_issuer: String,
    /// Lifetime of issued device JWTs.
    pub device_jwt_ttl_seconds: u64,
}

impl Config {
    pub fn new() -> anyhow::Result<Config> {
        _ = dotenvy::dotenv();

        Ok(Config {
            database_url: env::var("DATABASE_URL").context("DATABASE_URL is required.")?,
            packages_bucket_name: env::var("PACKAGES_BUCKET_NAME")
                .context("PACKAGES_BUCKET_NAME is required.")?,
            assets_bucket_name: env::var("ASSETS_BUCKET_NAME")
                .context("ASSETS_BUCKET_NAME is required.")?,
            aws_region: env::var("AWS_REGION").context("AWS_REGION is required.")?,
            sentry_url: env::var("SENTRY_URL").ok(),
            slack_hook_url: env::var("SLACK_HOOK_URL").ok(),
            deployment_slack_hook_url: env::var("DEPLOYMENT_SLACK_HOOK_URL").ok(),
            victoria_metrics_clients: VictoriaMetricsClient::from_env_all()?,
            ip_api_key: env::var("IP_API_KEY").ok(),
            auth0_issuer: env::var("AUTH0_ISSUER").context("AUTH0_ISSUER is required.")?,
            auth0_audience: env::var("AUTH0_AUDIENCE").context("AUTH0_AUDIENCE is required.")?,
            cloudfront: CloudFrontConfig::new()?,
            dashboard_excluded_labels: env::var("DASHBOARD_EXCLUDED_LABELS")
                .ok()
                .map(|s| s.split(',').map(|l| l.trim().to_string()).collect())
                .unwrap_or_default(),
            device_jwt_private_key_pem: env::var("DEVICE_JWT_PRIVATE_KEY_PEM")
                .context("DEVICE_JWT_PRIVATE_KEY_PEM is required.")?,
            device_jwt_issuer: env::var("DEVICE_JWT_ISSUER")
                .unwrap_or_else(|_| "smith-api".to_string()),
            device_jwt_ttl_seconds: env::var("DEVICE_JWT_TTL_SECONDS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3600),
        })
    }
}
