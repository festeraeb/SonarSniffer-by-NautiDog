use dashmap::DashMap;
use chrono::{DateTime, Utc};
use tracing::debug;

use crate::search::WebFinding;

/// Web result cache with TTL expiration
pub struct WebCache {
    cache: DashMap<String, (WebFinding, DateTime<Utc>)>,
    ttl_hours: u64,
}

impl WebCache {
    /// Create a new cache with the specified TTL
    pub fn new(ttl_hours: u64) -> Self {
        Self {
            cache: DashMap::new(),
            ttl_hours,
        }
    }

    /// Get a cached finding if it exists and hasn't expired
    pub fn get(&self, url: &str) -> Option<WebFinding> {
        if let Some(entry) = self.cache.get(url) {
            let (finding, fetched_at) = entry.value();
            
            // Use Utc::now() - fetched_at instead of fetched_at.elapsed()
            let elapsed = Utc::now() - *fetched_at;
            let ttl_duration = chrono::Duration::hours(self.ttl_hours as i64);
            
            if elapsed < ttl_duration {
                debug!("Cache hit for {}", url);
                return Some(finding.clone());
            } else {
                debug!("Cache expired for {}", url);
            }
        }
        
        None
    }

    /// Insert a finding into the cache
    pub fn insert(&self, url: &str, finding: WebFinding) {
        self.cache.insert(url.to_string(), (finding, Utc::now()));
        debug!("Cached {}", url);
    }

    /// Remove a specific entry from the cache
    pub fn remove(&self, url: &str) {
        self.cache.remove(url);
        debug!("Removed {} from cache", url);
    }

    /// Clear all expired entries
    pub fn cleanup_expired(&self) {
        let now = Utc::now();
        let ttl_duration = chrono::Duration::hours(self.ttl_hours as i64);
        
        let mut to_remove = Vec::new();
        for entry in self.cache.iter() {
            let (_, fetched_at) = entry.value();
            if now - *fetched_at >= ttl_duration {
                to_remove.push(entry.key().clone());
            }
        }
        
        for url in &to_remove {
            self.cache.remove(url);
        }
        
        if !to_remove.is_empty() {
            debug!("Cleaned up {} expired cache entries", to_remove.len());
        }
    }
}
