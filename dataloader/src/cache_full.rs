use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
#[cfg(feature = "postgres")]
use databaseschema::models::{
    historical_order::HistoricalOrder,
    historical_snapshot::HistoricalSnapshot,
};
use crate::Candle;

/// Cache entry with TTL and LRU support
#[derive(Debug, Clone)]
struct CacheEntry<T> {
    data: T,
    created_at: Instant,
    last_accessed: Instant,
    ttl: Duration,
}

impl<T> CacheEntry<T> {
    fn new(data: T, ttl: Duration) -> Self {
        let now = Instant::now();
        Self {
            data,
            created_at: now,
            last_accessed: now,
            ttl,
        }
    }

    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.ttl
    }

    fn touch(&mut self) {
        self.last_accessed = Instant::now();
    }

    fn age(&self) -> Duration {
        self.last_accessed.elapsed()
    }
}

/// High-performance in-memory cache for market data with adaptive TTL and LRU eviction
pub struct DataCache {
    orders: RwLock<HashMap<String, CacheEntry<Vec<HistoricalOrder>>>>,
    snapshots: RwLock<HashMap<String, CacheEntry<Vec<HistoricalSnapshot>>>>,
    candles: RwLock<HashMap<String, CacheEntry<Vec<Candle>>>>,
    default_ttl: Duration,
    orders_ttl: Duration,
    snapshots_ttl: Duration,
    max_entries_per_cache: usize,
}

impl DataCache {
    /// Create a new cache with optimized TTL defaults and memory limits
    pub fn new() -> Self {
        Self::with_adaptive_ttl(
            Duration::from_secs(300),  // 5 minutes default
            Duration::from_secs(900),  // 15 minutes for historical orders
            Duration::from_secs(300),  // 5 minutes for snapshots
        )
    }

    /// Create a new cache with custom TTL
    pub fn with_ttl(default_ttl: Duration) -> Self {
        Self::with_adaptive_ttl(
            default_ttl,
            default_ttl,
            default_ttl,
        )
    }

    /// Create a new cache with adaptive TTL for different data types and memory limits
    pub fn with_adaptive_ttl(default_ttl: Duration, orders_ttl: Duration, snapshots_ttl: Duration) -> Self {
        Self {
            orders: RwLock::new(HashMap::new()),
            snapshots: RwLock::new(HashMap::new()),
            candles: RwLock::new(HashMap::new()),
            default_ttl,
            orders_ttl,
            snapshots_ttl,
            max_entries_per_cache: 1000, // Limit to 1000 entries per cache type
        }
    }

    /// Create a cache with custom memory limits
    pub fn with_memory_limits(default_ttl: Duration, max_entries: usize) -> Self {
        Self {
            orders: RwLock::new(HashMap::new()),
            snapshots: RwLock::new(HashMap::new()),
            candles: RwLock::new(HashMap::new()),
            default_ttl,
            orders_ttl: default_ttl,
            snapshots_ttl: default_ttl,
            max_entries_per_cache: max_entries,
        }
    }

    /// Get cached orders for a key with LRU tracking
    pub async fn get_orders(&self, key: &str) -> Option<Vec<HistoricalOrder>> {
        let mut orders = self.orders.write().await;
        if let Some(entry) = orders.get_mut(key) {
            if !entry.is_expired() {
                entry.touch(); // Update LRU timestamp
                return Some(entry.data.clone());
            } else {
                orders.remove(key); // Remove expired entry
            }
        }
        None
    }

    /// Cache orders with optimized TTL and LRU eviction
    pub async fn put_orders(&self, key: String, data: Vec<HistoricalOrder>) {
        self.put_orders_with_ttl(key, data, self.orders_ttl).await;
    }

    /// Cache orders with custom TTL and LRU eviction
    pub async fn put_orders_with_ttl(&self, key: String, data: Vec<HistoricalOrder>, ttl: Duration) {
        let mut orders = self.orders.write().await;
        
        // Perform LRU eviction if needed
        if orders.len() >= self.max_entries_per_cache {
            self.evict_lru_entries(&mut orders);
        }
        
        orders.insert(key, CacheEntry::new(data, ttl));
    }

    /// Get cached snapshots for a key with LRU tracking
    pub async fn get_snapshots(&self, key: &str) -> Option<Vec<HistoricalSnapshot>> {
        let mut snapshots = self.snapshots.write().await;
        if let Some(entry) = snapshots.get_mut(key) {
            if !entry.is_expired() {
                entry.touch(); // Update LRU timestamp
                return Some(entry.data.clone());
            } else {
                snapshots.remove(key); // Remove expired entry
            }
        }
        None
    }

    /// Cache snapshots with optimized TTL and LRU eviction
    pub async fn put_snapshots(&self, key: String, data: Vec<HistoricalSnapshot>) {
        self.put_snapshots_with_ttl(key, data, self.snapshots_ttl).await;
    }

    /// Cache snapshots with custom TTL and LRU eviction
    pub async fn put_snapshots_with_ttl(&self, key: String, data: Vec<HistoricalSnapshot>, ttl: Duration) {
        let mut snapshots = self.snapshots.write().await;
        
        // Perform LRU eviction if needed
        if snapshots.len() >= self.max_entries_per_cache {
            self.evict_lru_entries(&mut snapshots);
        }
        
        snapshots.insert(key, CacheEntry::new(data, ttl));
    }

    /// Get cached candles for a key with LRU tracking
    pub async fn get_candles(&self, key: &str) -> Option<Vec<Candle>> {
        let mut candles = self.candles.write().await;
        if let Some(entry) = candles.get_mut(key) {
            if !entry.is_expired() {
                entry.touch(); // Update LRU timestamp
                return Some(entry.data.clone());
            } else {
                candles.remove(key); // Remove expired entry
            }
        }
        None
    }

    /// Cache candles with default TTL and LRU eviction
    pub async fn put_candles(&self, key: String, data: Vec<Candle>) {
        self.put_candles_with_ttl(key, data, self.default_ttl).await;
    }

    /// Cache candles with custom TTL and LRU eviction
    pub async fn put_candles_with_ttl(&self, key: String, data: Vec<Candle>, ttl: Duration) {
        let mut candles = self.candles.write().await;
        
        // Perform LRU eviction if needed
        if candles.len() >= self.max_entries_per_cache {
            self.evict_lru_entries(&mut candles);
        }
        
        candles.insert(key, CacheEntry::new(data, ttl));
    }

    /// Clear all cached data
    pub async fn clear(&self) {
        let mut orders = self.orders.write().await;
        let mut snapshots = self.snapshots.write().await;
        let mut candles = self.candles.write().await;
        orders.clear();
        snapshots.clear();
        candles.clear();
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
        {
            let mut candles = self.candles.write().await;
            candles.retain(|_, entry| !entry.is_expired());
        }
    }

    /// Evict least recently used entries when cache is full
    fn evict_lru_entries<T>(&self, cache: &mut HashMap<String, CacheEntry<T>>) {
        // Remove expired entries first
        cache.retain(|_, entry| !entry.is_expired());
        
        // If still over limit, remove oldest accessed entries
        if cache.len() >= self.max_entries_per_cache {
            let evict_count = cache.len() - (self.max_entries_per_cache * 3 / 4); // Remove 25% when full
            
            // Collect keys of oldest entries to remove
            let entries_to_remove: Vec<String> = {
                let mut entries: Vec<_> = cache.iter().collect();
                entries.sort_by_key(|(_, entry)| entry.last_accessed);
                entries.into_iter()
                    .take(evict_count)
                    .map(|(key, _)| key.clone())
                    .collect()
            };
            
            // Remove the collected keys
            for key in entries_to_remove {
                cache.remove(&key);
            }
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
