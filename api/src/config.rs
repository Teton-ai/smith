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
    pub fn new() -> Option<Self> {
        match (
            env::var("VICTORIA_METRICS_URL").ok(),
            env::var("VICTORIA_METRICS_AUTH_TOKEN").ok(),
        ) {
            (Some(url), Some(auth_token)) => {
                let mut headers = HeaderMap::new();
                let auth = format!("Basic {}", auth_token);
                headers.insert("authorization", auth.parse().unwrap());
                let victoria_client = reqwest::Client::builder()
                    .default_headers(headers)
                    .timeout(Duration::from_secs(60))
                    .build()
                    .unwrap();
                Some(VictoriaMetricsClient {
                    client: victoria_client,
                    url,
                })
            }
            _ => None,
        }
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
    pub victoria_metrics_client: Option<VictoriaMetricsClient>,
    pub ip_api_key: Option<String>,
    pub auth0_issuer: String,
    pub auth0_audience: String,
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
            victoria_metrics_client: VictoriaMetricsClient::new(),
            ip_api_key: env::var("IP_API_KEY").ok(),
            auth0_issuer: env::var("AUTH0_ISSUER").context("AUTH0_ISSUER is required.")?,
            auth0_audience: env::var("AUTH0_AUDIENCE").context("AUTH0_AUDIENCE is required.")?,
        })
    }
}
