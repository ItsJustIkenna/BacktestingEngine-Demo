use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use databaseschema::models::{
    historical_order::HistoricalOrder,
    historical_snapshot::HistoricalSnapshot,
};

/// Cache entry with TTL support
#[derive(Debug, Clone)]
struct CacheEntry<T> {
    data: T,
    created_at: Instant,
    ttl: Duration,
}

impl<T> CacheEntry<T> {
    fn new(data: T, ttl: Duration) -> Self {
        Self {
            data,
            created_at: Instant::now(),
            ttl,
        }
    }

    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.ttl
    }
}

/// High-performance in-memory cache for market data
pub struct DataCache {
    orders: RwLock<HashMap<String, CacheEntry<Vec<HistoricalOrder>>>>,
    snapshots: RwLock<HashMap<String, CacheEntry<Vec<HistoricalSnapshot>>>>,
    default_ttl: Duration,
}

impl DataCache {
    /// Create a new cache with default TTL of 5 minutes
    pub fn new() -> Self {
        Self::with_ttl(Duration::from_secs(300))
    }

    /// Create a new cache with custom TTL
    pub fn with_ttl(default_ttl: Duration) -> Self {
        Self {
            orders: RwLock::new(HashMap::new()),
            snapshots: RwLock::new(HashMap::new()),
            default_ttl,
        }
    }

    /// Get cached orders for a key
    pub async fn get_orders(&self, key: &str) -> Option<Vec<HistoricalOrder>> {
        let orders = self.orders.read().await;
        if let Some(entry) = orders.get(key) {
            if !entry.is_expired() {
                return Some(entry.data.clone());
            }
        }
        None
    }

    /// Cache orders with default TTL
    pub async fn put_orders(&self, key: String, data: Vec<HistoricalOrder>) {
        self.put_orders_with_ttl(key, data, self.default_ttl).await;
    }

    /// Cache orders with custom TTL
    pub async fn put_orders_with_ttl(&self, key: String, data: Vec<HistoricalOrder>, ttl: Duration) {
        let mut orders = self.orders.write().await;
        orders.insert(key, CacheEntry::new(data, ttl));
    }

    /// Get cached snapshots for a key
    pub async fn get_snapshots(&self, key: &str) -> Option<Vec<HistoricalSnapshot>> {
        let snapshots = self.snapshots.read().await;
        if let Some(entry) = snapshots.get(key) {
            if !entry.is_expired() {
                return Some(entry.data.clone());
            }
        }
        None
    }

    /// Cache snapshots with default TTL
    pub async fn put_snapshots(&self, key: String, data: Vec<HistoricalSnapshot>) {
        self.put_snapshots_with_ttl(key, data, self.default_ttl).await;
    }

    /// Cache snapshots with custom TTL
    pub async fn put_snapshots_with_ttl(&self, key: String, data: Vec<HistoricalSnapshot>, ttl: Duration) {
        let mut snapshots = self.snapshots.write().await;
        snapshots.insert(key, CacheEntry::new(data, ttl));
    }

    /// Clear all cached data
    pub async fn clear(&self) {
        let mut orders = self.orders.write().await;
        let mut snapshots = self.snapshots.write().await;
        orders.clear();
        snapshots.clear();
    }

    /// Remove expired entries from all caches
    pub async fn cleanup_expired(&self) {
        {
            let mut orders = self.orders.write().await;
            orders.retain(|_, entry| !entry.is_expired());
        }
        {
            let mut snapshots = self.snapshots.write().await;
            snapshots.retain(|_, entry| !entry.is_expired());
        }
    }
}

impl Default for DataCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_cache_entry_expiration() {
        let entry = CacheEntry::new(Vec::<HistoricalOrder>::new(), Duration::from_millis(50));
        assert!(!entry.is_expired());
        
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(entry.is_expired());
    }

    #[tokio::test]
    async fn test_cache_operations() {
        let cache = DataCache::new();
        let key = "test_key".to_string();
        let data = Vec::<HistoricalOrder>::new();

        // Test miss
        assert!(cache.get_orders(&key).await.is_none());

        // Test put and hit
        cache.put_orders(key.clone(), data.clone()).await;
        assert!(cache.get_orders(&key).await.is_some());
    }
}
