pub mod route;
pub mod service;

pub use service::ServiceInfo;

use crate::config::Config;
use crate::storage::Storage;
use serde::Serialize;
use service::{extract_service_name, is_service_file_path, parse_service_file};
use sqlx::PgPool;
use sqlx::types::chrono;
use std::io::{Cursor, Read};
use tracing::{debug, error};

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Package {
    pub id: i32,
    pub name: String,
    pub version: String,
    pub architecture: String,
    pub file: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Extracts all .service files from a deb package's data tar.
/// Returns a list of ServiceInfo with service name and optional WatchdogSec.
pub fn extract_services_from_deb(data: &[u8]) -> anyhow::Result<Vec<ServiceInfo>> {
    let mut cursor = Cursor::new(data);
    let mut pkg = debpkg::DebPkg::parse(&mut cursor)?;
    let mut data_tar = pkg.data()?;

    let mut services = Vec::new();

    for entry_result in data_tar.entries()? {
        let mut entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                debug!("Failed to read tar entry: {:?}", e);
                continue;
            }
        };

        let path = match entry.path() {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(e) => {
                debug!("Failed to get entry path: {:?}", e);
                continue;
            }
        };

        if is_service_file_path(&path) {
            if let Some(service_name) = extract_service_name(&path) {
                // Read the service file content to parse WatchdogSec
                let mut content = Vec::new();
                let watchdog_sec = if entry.read_to_end(&mut content).is_ok() {
                    parse_service_file(Cursor::new(&content))
                } else {
                    None
                };

                debug!(
                    "Found service: {} (watchdog: {:?})",
                    service_name, watchdog_sec
                );
                services.push(ServiceInfo {
                    name: service_name,
                    watchdog_sec,
                });
            }
        }
    }

    Ok(services)
}

impl Package {
    pub async fn new(
        name: &str,
        version: &str,
        architecture: &str,
        file_name: &str,
        file_data: &[u8],
        config: &'static Config,
        pool: &PgPool,
    ) -> anyhow::Result<Package> {
        Storage::save_to_s3(&config.packages_bucket_name, None, file_name, file_data).await?;

        match sqlx::query_as!(
            Package,
            "
          INSERT INTO package (name, version, architecture, file)
          VALUES ($1, $2, $3, $4)
          RETURNING *
          ",
            name,
            version,
            architecture,
            file_name
        )
        .fetch_one(pool)
        .await
        {
            Ok(package) => Ok(package),
            Err(err) => {
                let bucket_name = config.packages_bucket_name.clone();
                let file_name = file_name.to_string();
                tokio::spawn(async move {
                    if let Err(e) = Storage::delete_from_s3(&bucket_name, &file_name).await {
                        error!("Failed to delete S3 object after database error: {:?}", e);
                    }
                });
                Err(err.into())
            }
        }
    }
    pub async fn delete(
        package_id: &i32,
        config: &'static Config,
        pool: &PgPool,
    ) -> anyhow::Result<Package> {
        let package = sqlx::query_as!(
            Package,
            "DELETE FROM package WHERE id = $1 RETURNING *
          ",
            package_id,
        )
        .fetch_one(pool)
        .await?;
        Storage::delete_from_s3(&config.packages_bucket_name, &package.file).await?;
        Ok(package)
    }
}
