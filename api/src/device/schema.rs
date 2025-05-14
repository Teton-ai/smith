use serde::Serialize;
use sqlx::types::chrono;

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Device {
    pub id: i32,
    pub serial_number: String,
    pub note: Option<String>,
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
    pub created_on: chrono::DateTime<chrono::Utc>,
    pub approved: bool,
    pub has_token: Option<bool>,
    pub release_id: Option<i32>,
    pub target_release_id: Option<i32>,
    pub system_info: Option<serde_json::Value>,
    pub modem_id: Option<i32>,
}
