pub mod route;
pub mod uploads;

use smith::utils::schema::ServiceCheck;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::RwLock;
use std::time::{Duration, Instant};

const RELEASE_SERVICES_TTL: Duration = Duration::from_secs(60);
const MAX_RELEASE_SERVICES_ENTRIES: usize = 4_096;

const IP_ADDRESS_TTL: Duration = Duration::from_secs(600);
const MAX_IP_ADDRESS_ENTRIES: usize = 16_384;

/// Short-TTL in-memory caches for immutable or slowly changing lookup rows read
/// on every `/smith/home` poll.
#[derive(Debug, Default)]
pub struct HomeCache {
    release_services: RwLock<HashMap<i32, (Instant, Vec<ServiceCheck>)>>,
    ip_addresses: RwLock<HashMap<IpAddr, (Instant, i64)>>,
}

impl HomeCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_release_services(&self, release_id: i32) -> Option<Vec<ServiceCheck>> {
        let now = Instant::now();
        let guard = self
            .release_services
            .read()
            .unwrap_or_else(|err| err.into_inner());
        guard
            .get(&release_id)
            .filter(|(expires_at, _)| *expires_at > now)
            .map(|(_, services)| services.clone())
    }

    pub fn insert_release_services(&self, release_id: i32, services: Vec<ServiceCheck>) {
        let now = Instant::now();
        let mut guard = self
            .release_services
            .write()
            .unwrap_or_else(|err| err.into_inner());
        if guard.len() >= MAX_RELEASE_SERVICES_ENTRIES {
            guard.retain(|_, (expires_at, _)| *expires_at > now);
            if guard.len() >= MAX_RELEASE_SERVICES_ENTRIES {
                guard.clear();
            }
        }
        guard.insert(release_id, (now + RELEASE_SERVICES_TTL, services));
    }

    pub fn invalidate_release_services(&self, release_id: i32) {
        let mut guard = self
            .release_services
            .write()
            .unwrap_or_else(|err| err.into_inner());
        guard.remove(&release_id);
    }

    pub fn get_ip_id(&self, ip: IpAddr) -> Option<i64> {
        let now = Instant::now();
        let guard = self
            .ip_addresses
            .read()
            .unwrap_or_else(|err| err.into_inner());
        guard
            .get(&ip)
            .filter(|(expires_at, _)| *expires_at > now)
            .map(|(_, id)| *id)
    }

    pub fn insert_ip_id(&self, ip: IpAddr, id: i64) {
        let now = Instant::now();
        let mut guard = self
            .ip_addresses
            .write()
            .unwrap_or_else(|err| err.into_inner());
        if guard.len() >= MAX_IP_ADDRESS_ENTRIES {
            guard.retain(|_, (expires_at, _)| *expires_at > now);
            if guard.len() >= MAX_IP_ADDRESS_ENTRIES {
                guard.clear();
            }
        }
        guard.insert(ip, (now + IP_ADDRESS_TTL, id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn release_services_cache_hit_and_invalidation() {
        let cache = HomeCache::new();
        assert!(cache.get_release_services(42).is_none());

        let checks = vec![ServiceCheck {
            id: 7,
            name: "brain.service".to_string(),
        }];
        cache.insert_release_services(42, checks);
        let hit = cache.get_release_services(42);
        assert_eq!(hit.as_ref().map(Vec::len), Some(1));
        assert_eq!(
            hit.and_then(|mut v| v.pop()).map(|c| c.name),
            Some("brain.service".to_string())
        );

        cache.invalidate_release_services(42);
        assert!(cache.get_release_services(42).is_none());
    }

    #[test]
    fn ip_address_cache_hit_and_expiry() {
        let cache = HomeCache::new();
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10));
        assert_eq!(cache.get_ip_id(ip), None);

        cache.insert_ip_id(ip, 99);
        assert_eq!(cache.get_ip_id(ip), Some(99));

        // Force entry into the past to confirm expiry.
        if let Ok(mut guard) = cache.ip_addresses.write() {
            guard.insert(ip, (Instant::now() - Duration::from_secs(1), 99));
        }
        assert_eq!(cache.get_ip_id(ip), None);
    }
}
