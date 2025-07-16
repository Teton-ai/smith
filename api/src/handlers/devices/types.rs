use crate::handlers::distributions::types::Release;
use serde::{Deserialize, Serialize};
use sqlx::types::chrono;

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub struct LeanResponse {
    pub limit: i64,
    pub reverse: bool,
    pub devices: Vec<LeanDevice>,
}

#[derive(Debug, Serialize, utoipa::ToSchema, sqlx::FromRow)]
pub struct LeanDevice {
    pub id: i32,
    pub serial_number: String,
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
    pub approved: bool,
    pub up_to_date: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateDeviceRelease {
    pub target_release_id: i32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateDevicesRelease {
    pub target_release_id: i32,
    pub devices: Vec<i32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Tag {
    pub id: i32,
    pub device: i32,
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Variable {
    pub id: i32,
    pub device: i32,
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewVariable {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct DeviceCommandResponse {
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

#[derive(Debug, Serialize)]
pub struct CommandsPaginated {
    pub commands: Vec<DeviceCommandResponse>,
    pub next: Option<String>,
    pub previous: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct Note {
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DeviceLedgerItem {
    pub id: i32,
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub class: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DeviceLedgerItemPaginated {
    pub ledger: Vec<DeviceLedgerItem>,
    pub next: Option<String>,
    pub previous: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceIdentifiers {
    pub fa_areas: Vec<String>,
    pub identifiers: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct DeviceRelease {
    pub previous_release: Option<Release>,
    pub release: Option<Release>,
    pub target_release: Option<Release>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeviceHealth {
    pub id: i32,
    pub serial_number: String,
    pub last_ping: Option<chrono::DateTime<chrono::Utc>>,
    pub is_healthy: Option<bool>,
}
