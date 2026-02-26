use models::device::DeviceCommandResponse;
use sentry::types::Uuid;
use serde::{Deserialize, Serialize};
use smith::utils::schema::SafeCommandRequest;
use sqlx::types::chrono;

pub mod route;

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BundleWithCommands {
    #[schema(value_type = String)]
    pub uuid: Uuid,
    pub created_on: chrono::DateTime<chrono::Utc>,
    pub responses: Vec<DeviceCommandResponse>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BundleWithCommandsPaginated {
    pub bundles: Vec<BundleWithCommands>,
    pub next: Option<String>,
    pub previous: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct BundleCommands {
    pub devices: Vec<i32>,
    #[schema(value_type = Vec<Object>)]
    pub commands: Vec<SafeCommandRequest>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct BundleWithRawResponsesExplicit {
    pub uuid: Uuid,
    pub created_on: chrono::DateTime<chrono::Utc>,
    pub device: i32,
    pub serial_number: String,
    pub cmd_id: i32,
    pub issued_at: chrono::DateTime<chrono::Utc>,
    pub cmd_data: serde_json::Value,
    pub cancelled: bool,
    pub fetched: bool,
    pub fetched_at: Option<chrono::DateTime<chrono::Utc>>,
    pub response_id: Option<i32>,
    pub response_at: Option<chrono::DateTime<chrono::Utc>>,
    pub response: Option<serde_json::Value>,
    pub status: Option<i32>,
}
