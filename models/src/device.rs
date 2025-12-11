use crate::{ip_address::IpAddressInfo, modem::Modem, release::Release};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::types::{
    Json,
    chrono::{DateTime, Utc},
};
use std::collections::HashMap;
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DeviceNetwork {
    pub network_score: Option<i32>,
    pub download_speed_mbps: Option<f64>,
    pub upload_speed_mbps: Option<f64>,
    pub source: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Device {
    pub id: i32,
    pub serial_number: String,
    pub note: Option<String>,
    pub last_seen: Option<DateTime<Utc>>,
    pub created_on: DateTime<Utc>,
    pub approved: bool,
    pub has_token: Option<bool>,
    pub release_id: Option<i32>,
    pub target_release_id: Option<i32>,
    pub system_info: Option<Value>,
    pub modem_id: Option<i32>,
    pub ip_address_id: Option<i32>,
    pub ip_address: Option<IpAddressInfo>,
    pub modem: Option<Modem>,
    pub release: Option<Release>,
    pub target_release: Option<Release>,
    pub network: Option<DeviceNetwork>,
    #[schema(value_type = HashMap<String, String>)]
    pub labels: Json<HashMap<String, String>>,
}

/// Query filter for device listing.
#[derive(Deserialize, Serialize, Debug, Default)]
pub struct DeviceFilter {
    pub serial_number: Option<String>,
    /// Filter by approved status. If None, only approved devices are included by default.
    pub approved: Option<bool>,
    /// Filter by archived status. If None, archived devices are excluded by default.
    pub archived: Option<bool>,
    #[deprecated(
        since = "0.2.64",
        note = "Since labels have been released, tags concept be in version 0.74"
    )]
    pub tag: Option<String>,
    /// Filter by labels. Format: key=value. Multiple labels can be provided.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Filter by online status. If true, only devices online in the last 5 minutes.
    pub online: Option<bool>,
    /// Filter by outdated status. If true, only devices where release_id != target_release_id.
    pub outdated: Option<bool>,
    /// Exclude devices with these labels. Format: key=value. Used by dashboard.
    #[serde(default)]
    pub exclude_labels: Vec<String>,
    /// Maximum number of devices to return. Defaults to 100, max 1000.
    pub limit: Option<i64>,
    /// Number of devices to skip. Used for pagination.
    pub offset: Option<i64>,
    /// Search term to filter devices by serial number, hostname, or model.
    pub search: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema, Clone)]
pub struct DeviceCommandResponse {
    pub device: i32,
    pub serial_number: String,
    pub cmd_id: i32,
    pub issued_at: DateTime<Utc>,
    pub cmd_data: Value,
    pub cancelled: bool,
    pub fetched: bool,
    pub fetched_at: Option<DateTime<Utc>>,
    pub response_id: Option<i32>,
    pub response_at: Option<DateTime<Utc>>,
    pub response: Option<Value>,
    pub status: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CommandsPaginated {
    pub commands: Vec<DeviceCommandResponse>,
    pub next: Option<String>,
    pub previous: Option<String>,
}
