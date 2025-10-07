use serde::{Deserialize, Serialize};
use sqlx::types::chrono;
use utoipa::ToSchema;

#[derive(sqlx::Type, Serialize, Deserialize, Debug, Clone, ToSchema, PartialEq)]
#[sqlx(type_name = "deployment_status", rename_all = "snake_case")]
pub enum DeploymentStatus {
    InProgress,
    Failed,
    Canceled,
    Done,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct Deployment {
    pub id: i32,
    pub release_id: i32,
    pub status: DeploymentStatus,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DeploymentDevice {
    pub deployment_id: i32,
    pub device_id: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct DeploymentDeviceWithStatus {
    pub device_id: i32,
    pub serial_number: String,
    pub release_id: Option<i32>,
    pub target_release_id: Option<i32>,
    pub last_ping: Option<chrono::DateTime<chrono::Utc>>,
    pub added_at: chrono::DateTime<chrono::Utc>,
}
