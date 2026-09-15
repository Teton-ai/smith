use anyhow::Context;
use axum::http::HeaderMap;
use std::collections::HashMap;
use std::env;
use std::time::Duration;
use tracing::{info, warn};

#[derive(Debug)]
pub struct VictoriaMetricsClient {
    pub client: reqwest::Client,
    pub url: String,
}

impl VictoriaMetricsClient {
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        Self::from_vars(
            "VICTORIA_METRICS_URL_FLEET",
            "VICTORIA_METRICS_AUTH_TOKEN_FLEET",
        )
    }

    /// Reads use a separate vmauth user: the writer credential is scoped to
    /// `/opentelemetry/v1/metrics` and is rejected on the query paths. The URL
    /// here is the vmauth base, not a specific endpoint.
    pub fn from_env_read() -> anyhow::Result<Option<Self>> {
        Self::from_vars(
            "VICTORIA_METRICS_READ_URL_FLEET",
            "VICTORIA_METRICS_READ_AUTH_TOKEN_FLEET",
        )
    }

    fn from_vars(url_var: &str, token_var: &str) -> anyhow::Result<Option<Self>> {
        let (Some(url), Some(auth_token)) = (env::var(url_var).ok(), env::var(token_var).ok())
        else {
            warn!("VictoriaMetrics is not configured: {url_var} and {token_var} must both be set");
            return Ok(None);
        };

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

        info!(url = %url, "Configured VictoriaMetrics target ({url_var})");
        Ok(Some(VictoriaMetricsClient { client, url }))
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

/// The remote MCP server at `/mcp`.
///
/// It reuses the dashboard's Auth0 API as its OAuth resource, so MCP clients
/// get the same tokens as the dashboard. MCP clients accept a resource that is
/// a parent of the server URL, which is why `https://api.example.com` works for
/// `https://api.example.com/mcp`.
#[derive(Debug)]
pub struct McpConfig {
    /// `AUTH0_AUDIENCE`: the resource identifier clients request tokens for.
    pub resource_url: String,
    /// RFC 9728 metadata location: the well-known path inserted before the
    /// resource URL's path.
    pub resource_metadata_url: String,
    /// `Host` values the MCP transport accepts, guarding against DNS rebinding.
    pub allowed_hosts: Vec<String>,
}

impl McpConfig {
    /// None when the audience is not an absolute URL: an MCP resource has to be
    /// one, but that should not stop the rest of the API from starting.
    fn from_audience(auth0_audience: &str) -> Option<Self> {
        match Self::from_url(auth0_audience.to_string()) {
            Ok(mcp) => {
                info!(resource_url = %mcp.resource_url, "Configured MCP server");
                Some(mcp)
            }
            Err(err) => {
                warn!("MCP server disabled: AUTH0_AUDIENCE is not usable as a resource URL: {err}");
                None
            }
        }
    }

    fn from_url(resource_url: String) -> anyhow::Result<Self> {
        let url = reqwest::Url::parse(&resource_url).context("must be an absolute URL")?;
        let host = url.host_str().context("must include a host")?;
        let mut allowed_hosts = vec![host.to_string()];
        if let Some(port) = url.port() {
            allowed_hosts.push(format!("{host}:{port}"));
        }
        let resource_metadata_url = format!(
            "{}/.well-known/oauth-protected-resource{}",
            url.origin().ascii_serialization(),
            url.path().trim_end_matches('/')
        );

        Ok(McpConfig {
            resource_url,
            resource_metadata_url,
            allowed_hosts,
        })
    }
}

#[cfg(test)]
mod mcp_config_tests {
    use super::McpConfig;

    #[test]
    fn origin_audience_serves_metadata_at_the_root() -> anyhow::Result<()> {
        let mcp = McpConfig::from_url("https://api.example.com".to_string())?;
        assert_eq!(mcp.resource_url, "https://api.example.com");
        assert_eq!(
            mcp.resource_metadata_url,
            "https://api.example.com/.well-known/oauth-protected-resource"
        );
        assert_eq!(mcp.allowed_hosts, vec!["api.example.com"]);
        Ok(())
    }

    #[test]
    fn non_url_audience_disables_mcp() {
        assert!(McpConfig::from_audience("smith-api").is_none());
    }

    #[test]
    fn metadata_url_inserts_well_known_before_the_path() -> anyhow::Result<()> {
        let mcp = McpConfig::from_url("https://api.example.com/mcp".to_string())?;
        assert_eq!(mcp.resource_url, "https://api.example.com/mcp");
        assert_eq!(
            mcp.resource_metadata_url,
            "https://api.example.com/.well-known/oauth-protected-resource/mcp"
        );
        assert_eq!(mcp.allowed_hosts, vec!["api.example.com"]);
        Ok(())
    }

    #[test]
    fn explicit_ports_are_allowed_hosts_too() -> anyhow::Result<()> {
        let mcp = McpConfig::from_url("http://localhost:8080/mcp".to_string())?;
        assert_eq!(
            mcp.resource_metadata_url,
            "http://localhost:8080/.well-known/oauth-protected-resource/mcp"
        );
        assert_eq!(mcp.allowed_hosts, vec!["localhost", "localhost:8080"]);
        Ok(())
    }

    #[test]
    fn relative_urls_are_rejected() {
        assert!(McpConfig::from_url("/mcp".to_string()).is_err());
    }
}

#[derive(Debug)]
pub struct Config {
    pub database_url: String,
    pub packages_bucket_name: String,
    pub assets_bucket_name: String,
    pub data_engine_bucket_name: String,
    pub aws_region: String,
    pub sentry_url: Option<String>,
    pub slack_hook_url: Option<String>,
    pub deployment_slack_hook_url: Option<String>,
    pub victoria_metrics_client: Option<VictoriaMetricsClient>,
    pub victoria_metrics_read_client: Option<VictoriaMetricsClient>,
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
    /// Maps an M2M token's Auth0 `sub` claim (e.g. `"<client_id>@clients"`) to
    /// the network-reference-ledger `holder` name it is trusted to act as.
    /// `holder` is never client-supplied: this map is the sole source of truth
    /// for who a caller is allowed to hold references as.
    pub known_holders: HashMap<String, String>,
    /// How often the network garbage collection sweep (`network::gc`) runs. A plain tunable,
    /// not an on/off switch - the sweep only ships once seeding is already
    /// confirmed.
    pub network_gc_interval_seconds: u64,
    pub mcp: Option<McpConfig>,
}

/// Builds the M2M-sub -> holder map from one env var per known holder. Missing
/// (unset in dev/CI, where no App API integration is exercised) only warns,
/// matching how every other optional integration in this file behaves
/// (`VictoriaMetricsClient`, `ip_api_key`, ...); a misconfigured production
/// deploy is caught operationally (App API's ledger calls 403) rather than by
/// refusing to boot the whole API over one holder's credentials.
fn known_holders_from_env() -> HashMap<String, String> {
    let mut holders = HashMap::new();
    match env::var("APP_API_M2M_CLIENT_ID") {
        Ok(client_id) => {
            holders.insert(format!("{client_id}@clients"), "app_api".to_string());
        }
        Err(_) => warn!(
            "APP_API_M2M_CLIENT_ID is not set: App API's ledger calls (acquire/release/reconcile, \
             POST /networks with `reference`) will all 403"
        ),
    }
    holders
}

impl Config {
    pub fn new() -> anyhow::Result<Config> {
        _ = dotenvy::dotenv();

        let auth0_audience = env::var("AUTH0_AUDIENCE").context("AUTH0_AUDIENCE is required.")?;
        let mcp = McpConfig::from_audience(&auth0_audience);

        Ok(Config {
            database_url: env::var("DATABASE_URL").context("DATABASE_URL is required.")?,
            packages_bucket_name: env::var("PACKAGES_BUCKET_NAME")
                .context("PACKAGES_BUCKET_NAME is required.")?,
            assets_bucket_name: env::var("ASSETS_BUCKET_NAME")
                .context("ASSETS_BUCKET_NAME is required.")?,
            data_engine_bucket_name: env::var("DATA_ENGINE_BUCKET_NAME")
                .context("DATA_ENGINE_BUCKET_NAME is required.")?,
            aws_region: env::var("AWS_REGION").context("AWS_REGION is required.")?,
            sentry_url: env::var("SENTRY_URL").ok(),
            slack_hook_url: env::var("SLACK_HOOK_URL").ok(),
            deployment_slack_hook_url: env::var("DEPLOYMENT_SLACK_HOOK_URL").ok(),
            victoria_metrics_client: VictoriaMetricsClient::from_env()?,
            victoria_metrics_read_client: VictoriaMetricsClient::from_env_read()?,
            ip_api_key: env::var("IP_API_KEY").ok(),
            auth0_issuer: env::var("AUTH0_ISSUER").context("AUTH0_ISSUER is required.")?,
            auth0_audience,
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
            known_holders: known_holders_from_env(),
            network_gc_interval_seconds: env::var("NETWORK_GC_INTERVAL_SECONDS")
                .ok()
                .and_then(|s| s.parse().ok())
                // Zero treated as unset, not honored: tokio::time::interval
                // panics on a zero duration.
                .filter(|&secs: &u64| secs > 0)
                // App API's reconcile job (the only current holder) pushes at
                // most every 2 days, so sweeping more often than that just
                // finds nothing new.
                .unwrap_or(2 * 24 * 60 * 60),
            mcp,
        })
    }
}
