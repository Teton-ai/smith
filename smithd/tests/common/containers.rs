use anyhow::{Context, Result};
use std::collections::HashMap;
use std::time::Duration;
use testcontainers::{ContainerAsync, GenericImage, ImageExt, runners::AsyncRunner};
use tokio::time::sleep;

pub struct SmithTestEnvironment {
    pub postgres: ContainerAsync<GenericImage>,
    pub api: ContainerAsync<GenericImage>,
    pub bore: ContainerAsync<GenericImage>,
    network_name: String,
}

impl SmithTestEnvironment {
    pub async fn start() -> Result<Self> {
        let network_name = format!("smith-test-{}", uuid::Uuid::new_v4());

        // Start PostgreSQL
        let postgres = GenericImage::new("postgres", "16-alpine")
            .with_env_var("POSTGRES_PASSWORD", "postgres")
            .with_env_var("POSTGRES_USER", "postgres")
            .with_env_var("POSTGRES_DB", "postgres")
            .start()
            .await
            .context("Failed to start PostgreSQL")?;

        // Wait a bit for postgres to be fully ready
        sleep(Duration::from_secs(5)).await;

        // Build and start API
        let postgres_host = postgres.get_host().await?;
        let postgres_port = postgres.get_host_port_ipv4(5432).await?;
        
        let api = GenericImage::new("smith-api", "test")
            .with_env_var("DATABASE_URL", &format!("postgres://postgres:postgres@{}:{}/postgres", postgres_host, postgres_port))
            .with_env_var("RUST_LOG", "info")
            // Required environment variables for API
            .with_env_var("PACKAGES_BUCKET_NAME", "test-packages-bucket")
            .with_env_var("ASSETS_BUCKET_NAME", "test-assets-bucket")
            .with_env_var("AWS_REGION", "us-east-1")
            .with_env_var("AUTH0_ISSUER", "https://test-issuer.auth0.com/")
            .with_env_var("AUTH0_AUDIENCE", "https://test-api")
            .with_env_var("CLOUDFRONT_DOMAIN_NAME", "")
            .with_env_var("CLOUDFRONT_PACKAGE_KEY_PAIR_ID", "")
            .with_env_var("CLOUDFRONT_PACKAGE_PRIVATE_KEY", "")
            .start()
            .await
            .context("Failed to start API")?;

        // Wait for API to start
        sleep(Duration::from_secs(3)).await;

        // Start bore tunnel server
        let bore = GenericImage::new("ekzhang/bore", "latest")
            .start()
            .await
            .context("Failed to start bore")?;

        Ok(Self {
            postgres,
            api,
            bore,
            network_name,
        })
    }

    pub async fn api_base_url(&self) -> Result<String> {
        let host = self.api.get_host().await?;
        let port = self.api.get_host_port_ipv4(8080).await?;
        Ok(format!("http://{}:{}", host, port))
    }

    pub async fn postgres_url(&self) -> Result<String> {
        let host = self.postgres.get_host().await?;
        let port = self.postgres.get_host_port_ipv4(5432).await?;
        Ok(format!(
            "postgres://postgres:postgres@{}:{}/postgres",
            host, port
        ))
    }

    pub async fn spawn_device(&self, config: DeviceConfig) -> Result<DeviceHandle> {
        DeviceHandle::spawn(self, config).await
    }

    pub fn network_name(&self) -> &str {
        &self.network_name
    }
}

#[derive(Clone)]
pub struct DeviceConfig {
    pub serial_number: String,
    pub wifi_mac: String,
    pub server_url: String,
    pub env_vars: HashMap<String, String>,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            serial_number: format!("TEST-{}", uuid::Uuid::new_v4()),
            wifi_mac: "00:11:22:33:44:55".to_string(),
            server_url: String::new(),
            env_vars: HashMap::new(),
        }
    }
}

pub struct DeviceHandle {
    pub container: ContainerAsync<GenericImage>,
    pub serial_number: String,
    pub config: DeviceConfig,
}

impl DeviceHandle {
    pub async fn spawn(env: &SmithTestEnvironment, mut config: DeviceConfig) -> Result<Self> {
        if config.server_url.is_empty() {
            config.server_url = env.api_base_url().await?;
        }

        // Build device container
        let mut image =
            GenericImage::new("smith-device", "test").with_env_var("RUST_LOG", "info,smithd=debug");

        for (key, value) in &config.env_vars {
            image = image.with_env_var(key, value);
        }

        let container = image
            .start()
            .await
            .context("Failed to start device container")?;

        Ok(Self {
            container,
            serial_number: config.serial_number.clone(),
            config,
        })
    }

    pub async fn stop(self) -> Result<()> {
        self.container.stop().await?;
        Ok(())
    }

    pub async fn logs(&self) -> Result<String> {
        // testcontainers 0.23 doesn't have a direct logs API
        // For now, return empty string
        Ok(String::new())
    }
}
