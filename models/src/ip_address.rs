use serde::{Deserialize, Serialize};
use sqlx::types::{chrono, ipnetwork::IpNetwork};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema, Debug)]
pub struct IpAddressInfo {
    pub id: i32,
    #[schema(value_type = String, example = "192.168.1.1")]
    pub ip_address: IpNetwork,
    pub name: Option<String>,
    pub continent: Option<String>,
    pub continent_code: Option<String>,
    pub country_code: Option<String>,
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub isp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinates: Option<(f64, f64)>,
    pub proxy: Option<bool>,
    pub hosting: Option<bool>,
    pub device_count: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}
