//! Caching utilities for performance optimization.
//!
//! Provides async caching using moka for frequently accessed data.
//!
//! Requires the `async` feature to be enabled.

use moka::future::Cache;
use std::time::Duration;

/// Simple async cache wrapper for agent data.
///
/// # Example
///
/// ```no_run
/// use a2a_agents_common::caching::AgentCache;
/// use std::time::Duration;
///
/// #[tokio::main]
/// async fn main() {
///     let cache = AgentCache::<String, String>::new()
///         .with_max_capacity(1000)
///         .with_ttl(Duration::from_secs(300));
///
///     cache.insert("key", "value").await;
///     let value = cache.get("key").await;
///     assert_eq!(value, Some("value"));
/// }
/// ```
#[derive(Clone)]
pub struct AgentCache<K, V>
where
    K: std::hash::Hash + Eq + Send + Sync + Clone + 'static,
    V: Clone + Send + Sync + 'static,
{
    cache: Cache<K, V>,
}

impl<K, V> AgentCache<K, V>
where
    K: std::hash::Hash + Eq + Send + Sync + Clone + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Create a new cache with default settings (max 10,000 entries, no TTL).
    pub fn new() -> Self {
        Self {
            cache: Cache::new(10_000),
        }
    }

    /// Set the maximum capacity.
    pub fn with_max_capacity(self, capacity: u64) -> Self {
        Self {
            cache: Cache::new(capacity),
        }
    }

    /// Set time-to-live for entries.
    pub fn with_ttl(self, ttl: Duration) -> Self {
        Self {
            cache: Cache::builder().time_to_live(ttl).build(),
        }
    }

    /// Insert a value into the cache.
    pub async fn insert(&self, key: K, value: V) {
        self.cache.insert(key, value).await;
    }

    /// Get a value from the cache.
    pub async fn get(&self, key: &K) -> Option<V> {
        self.cache.get(key).await
    }

    /// Remove a value from the cache.
    pub async fn remove(&self, key: &K) {
        self.cache.invalidate(key).await;
    }

    /// Clear all entries from the cache.
    pub async fn clear(&self) {
        self.cache.invalidate_all();
    }

    /// Get the number of entries in the cache.
    pub async fn len(&self) -> u64 {
        self.cache.entry_count()
    }

    /// Check if the cache is empty.
    pub async fn is_empty(&self) -> bool {
        self.cache.entry_count() == 0
    }

    /// Get or insert a value using a provided function.
    pub async fn get_or_insert_with<F>(&self, key: K, f: F) -> V
    where
        F: std::future::Future<Output = V>,
    {
        match self.cache.get(&key).await {
            Some(value) => value,
            None => {
                let value = f.await;
                self.cache.insert(key, value.clone()).await;
                value
            }
        }
    }
}

impl<K, V> Default for AgentCache<K, V>
where
    K: std::hash::Hash + Eq + Send + Sync + Clone + 'static,
    V: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Cache for JSON-serializable data with string keys.
///
/// This is a convenience wrapper for common use cases where you want to cache
/// JSON-serializable structs.
pub type JsonCache<V> = AgentCache<String, V>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_operations() {
        let cache = AgentCache::new();

        cache.insert("key1", "value1").await;
        assert_eq!(cache.get(&"key1").await, Some("value1"));

        cache.remove(&"key1").await;
        assert_eq!(cache.get(&"key1").await, None);
    }

    #[tokio::test]
    async fn test_get_or_insert() {
        let cache = AgentCache::new();

        let value = cache
            .get_or_insert_with("key", async { "computed value" })
            .await;
        assert_eq!(value, "computed value");

        // Should return cached value without recomputing
        let value2 = cache
            .get_or_insert_with("key", async { "different value" })
            .await;
        assert_eq!(value2, "computed value");
    }

    #[tokio::test]
    async fn test_ttl() {
        let cache = AgentCache::new().with_ttl(Duration::from_millis(100));

        cache.insert("key", "value").await;
        assert_eq!(cache.get(&"key").await, Some("value"));

        // Wait for TTL to expire
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Run cache maintenance
        cache.cache.run_pending_tasks().await;

        // Value should be gone
        assert_eq!(cache.get(&"key").await, None);
    }

    #[tokio::test]
    async fn test_clear() {
        let cache = AgentCache::new();

        cache.insert("key1", "value1").await;
        cache.insert("key2", "value2").await;
        assert_eq!(cache.len().await, 2);

        cache.clear().await;
        assert_eq!(cache.len().await, 0);
    }
}
