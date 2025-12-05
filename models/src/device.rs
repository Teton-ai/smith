use std::collections::HashMap;

use crate::{ip_address::IpAddressInfo, modem::Modem, release::Release, system::SystemInfo};
use serde::{Deserialize, Serialize};
use sqlx::types::{
    Json,
    chrono::{DateTime, Utc},
};
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
    #[schema(value_type = Option<SystemInfo>)]
    pub system_info: Option<Json<SystemInfo>>,
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
}
