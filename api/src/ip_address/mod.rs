pub mod route;
use axum::http::HeaderMap;
use std::net::{IpAddr, SocketAddr};

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
