use jwks_client_rs::{JwksClient, source::WebSource};
use reqwest::Url;
use std::sync::Arc;
use std::time::Duration;

// Wrapper to provide Debug implementation for JwksClient
#[derive(Clone)]
pub struct DebugJwksClient(Arc<JwksClient<WebSource>>);

impl std::fmt::Debug for DebugJwksClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<JwksClient>")
    }
}

impl std::ops::Deref for DebugJwksClient {
    type Target = Arc<JwksClient<WebSource>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DebugJwksClient {
    pub fn init(auth0_issuer: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let jwks_url = Url::parse(auth0_issuer)?.join(".well-known/jwks.json")?;
        let jwks_source = WebSource::builder().build(jwks_url)?;
        let jwks_client = Arc::new(
            JwksClient::builder()
                .time_to_live(Duration::from_secs(60))
                .build(jwks_source),
        );
        Ok(DebugJwksClient(jwks_client))
    }
}
