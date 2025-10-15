pub mod route;

use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use sqlx::types::{chrono, ipnetwork::IpNetwork};
use std::net::{IpAddr, SocketAddr};
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

impl IpAddressInfo {
    pub fn extract_client_ip(headers: &HeaderMap, fallback_addr: SocketAddr) -> IpAddr {
        // Check X-Forwarded-For header first (load balancer/proxy)
        if let Some(forwarded_for) = headers.get("x-forwarded-for") {
            if let Ok(forwarded_str) = forwarded_for.to_str() {
                // X-Forwarded-For can contain multiple IPs, take the first (original client)
                if let Some(first_ip) = forwarded_str.split(',').next() {
                    if let Ok(ip) = first_ip.trim().parse::<IpAddr>() {
                        return ip;
                    }
                }
            }
        }

        // Check X-Real-IP header (alternative proxy header)
        if let Some(real_ip) = headers.get("x-real-ip") {
            if let Ok(real_ip_str) = real_ip.to_str() {
                if let Ok(ip) = real_ip_str.parse::<IpAddr>() {
                    return ip;
                }
            }
        }

        // Fall back to direct connection IP
        fallback_addr.ip()
    }
}
