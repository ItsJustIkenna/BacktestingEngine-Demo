use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
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
}

/// In-memory cache for historical data with TTL and LRU eviction
pub struct DataCache {
    candles: RwLock<HashMap<String, CacheEntry<Vec<Candle>>>>,
    max_entries: usize,
    default_ttl: Duration,
}

impl DataCache {
    /// Create a new cache with default settings
    pub fn new() -> Self {
        Self::with_capacity(1000)
    }

    /// Create a new cache with specified capacity
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            candles: RwLock::new(HashMap::new()),
            max_entries,
            default_ttl: Duration::from_secs(300), // 5 minutes default TTL
        }
    }

    /// Get candles from cache
    pub async fn get_candles(&self, key: &str) -> Option<Vec<Candle>> {
        let mut candles = self.candles.write().await;
        
        if let Some(entry) = candles.get_mut(key) {
            if !entry.is_expired() {
                entry.touch();
                return Some(entry.data.clone());
            } else {
                candles.remove(key);
            }
        }
        None
    }

    /// Put candles in cache with default TTL
    pub async fn put_candles(&self, key: String, data: Vec<Candle>) {
        self.put_candles_with_ttl(key, data, self.default_ttl).await;
    }

    /// Put candles in cache with custom TTL
    pub async fn put_candles_with_ttl(&self, key: String, data: Vec<Candle>, ttl: Duration) {
        let mut candles = self.candles.write().await;
        
        // Check if we need to evict entries
        if candles.len() >= self.max_entries {
            self.evict_lru(&mut candles);
        }
        
        let entry = CacheEntry::new(data, ttl);
        candles.insert(key, entry);
    }

    /// Clear all cached data
    pub async fn clear(&self) {
        let mut candles = self.candles.write().await;
        candles.clear();
    }

    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        let candles = self.candles.read().await;
        
        let total_entries = candles.len();
        let expired_entries = candles.values()
            .filter(|entry| entry.is_expired())
            .count();

        CacheStats {
            total_entries,
            expired_entries,
            capacity: self.max_entries,
            candles_cached: candles.len(),
        }
    }

    /// Evict least recently used entries
    fn evict_lru(&self, cache: &mut HashMap<String, CacheEntry<Vec<Candle>>>) {
        // Remove expired entries first
        cache.retain(|_, entry| !entry.is_expired());
        
        // If still over capacity, remove oldest accessed entries
        if cache.len() >= self.max_entries {
            let mut entries: Vec<_> = cache.iter().collect();
            entries.sort_by_key(|(_, entry)| entry.last_accessed);
            
            // Remove the oldest 25% of entries
            let to_remove = cache.len() / 4;
            for (key, _) in entries.iter().take(to_remove) {
                cache.remove(*key);
            }
        }
    }

    /// Cleanup expired entries
    pub async fn cleanup_expired(&self) {
        let mut candles = self.candles.write().await;
        candles.retain(|_, entry| !entry.is_expired());
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub expired_entries: usize,
    pub capacity: usize,
    pub candles_cached: usize,
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
    async fn test_cache_basic_operations() {
        let cache = DataCache::new();
        let key = "BTCUSD_1m".to_string();
        let candles = vec![]; // Empty candles for testing
        
        // Test put and get
        cache.put_candles(key.clone(), candles.clone()).await;
        let retrieved = cache.get_candles(&key).await;
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_cache_expiry() {
        let cache = DataCache::new();
        let key = "BTCUSD_1m".to_string();
        let candles = vec![]; // Empty candles for testing
        
        // Put with very short TTL
        cache.put_candles_with_ttl(key.clone(), candles, Duration::from_millis(50)).await;
        
        // Should be available immediately
        assert!(cache.get_candles(&key).await.is_some());
        
        // Wait for expiry
        tokio::time::sleep(Duration::from_millis(60)).await;
        
        // Should be expired
        assert!(cache.get_candles(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let cache = DataCache::new();
        let candles = vec![]; // Empty candles for testing
        
        cache.put_candles("key1".to_string(), candles.clone()).await;
        cache.put_candles("key2".to_string(), candles).await;
        
        // Clear all
        cache.clear().await;
        
        // Should be empty
        assert!(cache.get_candles("key1").await.is_none());
        assert!(cache.get_candles("key2").await.is_none());
    }

    #[test]
    fn test_cache_entry_expiry() {
        let data = vec![];
        let entry = CacheEntry::new(data, Duration::from_millis(50));
        
        // Should not be expired immediately
        assert!(!entry.is_expired());
        
        // Wait and check
        std::thread::sleep(Duration::from_millis(60));
        assert!(entry.is_expired());
    }
}
