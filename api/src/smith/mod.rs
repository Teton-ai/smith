pub mod route;
pub mod uploads;

use smith::utils::schema::ServiceCheck;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

const RELEASE_SERVICES_TTL: Duration = Duration::from_secs(60);
const MAX_RELEASE_SERVICES_ENTRIES: usize = 4_096;

const IP_ADDRESS_TTL: Duration = Duration::from_secs(600);
const MAX_IP_ADDRESS_ENTRIES: usize = 16_384;

const DOWNLOAD_SLOT_FULL_TTL: Duration = Duration::from_secs(10);
const MAX_DOWNLOAD_SLOT_FULL_ENTRIES: usize = 16_384;

/// Short-TTL in-memory caches for immutable or slowly changing lookup rows read
/// on every `/smith/home` poll.
#[derive(Debug, Default)]
pub struct HomeCache {
    release_services: RwLock<HashMap<i32, (Instant, Vec<ServiceCheck>)>>,
    ip_addresses: RwLock<HashMap<IpAddr, (Instant, i64)>>,
    download_slots_full: RwLock<HashMap<i64, Instant>>,
    inflight_slot_checks: Mutex<HashSet<i64>>,
}

/// Releases an in-flight per-IP slot-check marker when the check finishes or is cancelled.
pub struct DownloadSlotCheckGuard<'a> {
    cache: &'a HomeCache,
    ip_address_id: i64,
}

impl Drop for DownloadSlotCheckGuard<'_> {
    fn drop(&mut self) {
        let mut guard = self
            .cache
            .inflight_slot_checks
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        guard.remove(&self.ip_address_id);
    }
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
        if guard.len() >= MAX_RELEASE_SERVICES_ENTRIES && !guard.contains_key(&release_id) {
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
        if guard.len() >= MAX_IP_ADDRESS_ENTRIES && !guard.contains_key(&ip) {
            guard.retain(|_, (expires_at, _)| *expires_at > now);
            if guard.len() >= MAX_IP_ADDRESS_ENTRIES {
                guard.clear();
            }
        }
        guard.insert(ip, (now + IP_ADDRESS_TTL, id));
    }

    pub fn invalidate_ip_id(&self, ip: IpAddr) {
        let mut guard = self
            .ip_addresses
            .write()
            .unwrap_or_else(|err| err.into_inner());
        guard.remove(&ip);
    }

    pub fn is_download_slot_full(&self, ip_address_id: i64) -> bool {
        let now = Instant::now();
        let guard = self
            .download_slots_full
            .read()
            .unwrap_or_else(|err| err.into_inner());
        guard
            .get(&ip_address_id)
            .is_some_and(|expires_at| *expires_at > now)
    }

    pub fn mark_download_slot_full(&self, ip_address_id: i64) {
        let now = Instant::now();
        let mut guard = self
            .download_slots_full
            .write()
            .unwrap_or_else(|err| err.into_inner());
        if guard.len() >= MAX_DOWNLOAD_SLOT_FULL_ENTRIES && !guard.contains_key(&ip_address_id) {
            guard.retain(|_, expires_at| *expires_at > now);
            if guard.len() >= MAX_DOWNLOAD_SLOT_FULL_ENTRIES {
                guard.clear();
            }
        }
        guard.insert(ip_address_id, now + DOWNLOAD_SLOT_FULL_TTL);
    }

    pub fn invalidate_download_slot_full(&self, ip_address_id: i64) {
        let mut guard = self
            .download_slots_full
            .write()
            .unwrap_or_else(|err| err.into_inner());
        guard.remove(&ip_address_id);
    }

    /// Claims the single in-flight slot-check slot for `ip_address_id` without
    /// waiting. Concurrent polls on the same NAT IP return immediately instead
    /// of pinning global `/smith/home` permits behind `ip_address FOR NO KEY UPDATE`.
    pub fn try_enter_download_slot_check(
        &self,
        ip_address_id: i64,
    ) -> Option<DownloadSlotCheckGuard<'_>> {
        let inserted = self
            .inflight_slot_checks
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(ip_address_id);
        if inserted {
            Some(DownloadSlotCheckGuard {
                cache: self,
                ip_address_id,
            })
        } else {
            None
        }
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

        cache.invalidate_ip_id(ip);
        assert_eq!(cache.get_ip_id(ip), None);

        cache.insert_ip_id(ip, 99);
        // Force entry into the past to confirm expiry.
        if let Ok(mut guard) = cache.ip_addresses.write() {
            guard.insert(ip, (Instant::now() - Duration::from_secs(1), 99));
        }
        assert_eq!(cache.get_ip_id(ip), None);
    }

    #[test]
    fn download_slot_full_cache_hit_invalidation_and_expiry() {
        let cache = HomeCache::new();
        assert!(!cache.is_download_slot_full(7));

        cache.mark_download_slot_full(7);
        assert!(cache.is_download_slot_full(7));

        cache.invalidate_download_slot_full(7);
        assert!(!cache.is_download_slot_full(7));

        cache.mark_download_slot_full(7);
        if let Ok(mut guard) = cache.download_slots_full.write() {
            guard.insert(7, Instant::now() - Duration::from_secs(1));
        }
        assert!(!cache.is_download_slot_full(7));
    }

    #[test]
    fn inflight_slot_check_rejects_duplicate_until_drop() {
        let cache = HomeCache::new();
        let first = cache.try_enter_download_slot_check(11);
        assert!(first.is_some());
        assert!(cache.try_enter_download_slot_check(11).is_none());
        let other = cache.try_enter_download_slot_check(12);
        assert!(other.is_some());
        drop(first);
        assert!(cache.try_enter_download_slot_check(11).is_some());
    }
}
