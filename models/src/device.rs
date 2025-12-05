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
