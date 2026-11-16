pub mod route;
use axum::http::HeaderMap;
use std::net::{IpAddr, SocketAddr};

pub fn extract_client_ip(headers: &HeaderMap, fallback_addr: SocketAddr) -> IpAddr {
    // Check X-Forwarded-For header first (load balancer/proxy)
    if let Some(forwarded_for) = headers.get("x-forwarded-for")
        && let Ok(forwarded_str) = forwarded_for.to_str()
        // X-Forwarded-For can contain multiple IPs, take the first (original client)
        && let Some(first_ip) = forwarded_str.split(',').next()
        && let Ok(ip) = first_ip.trim().parse::<IpAddr>()
    {
        return ip;
    }
    // Check X-Real-IP header (alternative proxy header)
    if let Some(real_ip) = headers.get("x-real-ip")
        && let Ok(real_ip_str) = real_ip.to_str()
        && let Ok(ip) = real_ip_str.parse::<IpAddr>()
    {
        return ip;
    }

    // Fall back to direct connection IP
    fallback_addr.ip()
}

/// Registration is behind one AWS ALB in append mode. The final entry is the
/// client observed by the ALB; earlier entries and X-Real-IP are client-controlled.
/// Network access to the API must remain restricted to trusted infrastructure.
/// Invalid forwarded headers fail closed instead of falling back to a proxy IP.
pub fn registration_client_ip(headers: &HeaderMap, peer: SocketAddr) -> Option<IpAddr> {
    match headers.get_all("x-forwarded-for").iter().next_back() {
        Some(value) => value.to_str().ok()?.rsplit(',').next()?.trim().parse().ok(),
        None => Some(peer.ip()),
    }
}

#[cfg(test)]
mod registration_tests {
    use super::*;

    #[test]
    fn registration_ip_uses_the_address_appended_by_the_alb() {
        let peer = "172.31.0.1:8080".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", "192.0.2.1".parse().unwrap());
        assert_eq!(registration_client_ip(&headers, peer), Some(peer.ip()));
        headers.insert("x-forwarded-for", "192.0.2.1, 2001:db8::1".parse().unwrap());
        assert_eq!(
            registration_client_ip(&headers, peer),
            Some("2001:db8::1".parse().unwrap())
        );
        headers.append("x-forwarded-for", "192.0.2.2".parse().unwrap());
        assert_eq!(
            registration_client_ip(&headers, peer),
            Some("192.0.2.2".parse().unwrap())
        );
        for invalid in ["192.0.2.1, invalid", "192.0.2.1,", ""] {
            headers.insert("x-forwarded-for", invalid.parse().unwrap());
            assert_eq!(registration_client_ip(&headers, peer), None);
        }
    }
}
