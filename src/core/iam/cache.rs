//! LRU cache for IAM policy evaluation results
//!
//! Caches evaluation results to achieve 10,000+ evals/sec performance.

use lru::LruCache;
use std::num::NonZeroUsize;

/// Cache key for policy evaluation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    action: String,
    resource: String,
}

/// LRU cache for policy evaluation results
pub struct PolicyCache {
    cache: LruCache<CacheKey, bool>,
}

impl PolicyCache {
    /// Create a new policy cache with given capacity
    pub fn new(capacity: usize) -> Self {
        PolicyCache {
            cache: LruCache::new(NonZeroUsize::new(capacity).unwrap()),
        }
    }

    /// Get cached evaluation result
    pub fn get(&mut self, action: &str, resource: &str) -> Option<bool> {
        let key = CacheKey {
            action: action.to_string(),
            resource: resource.to_string(),
        };
        self.cache.get(&key).copied()
    }

    /// Put evaluation result in cache
    pub fn put(&mut self, action: &str, resource: &str, result: bool) {
        let key = CacheKey {
            action: action.to_string(),
            resource: resource.to_string(),
        };
        self.cache.put(key, result);
    }

    /// Clear the cache
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Get cache statistics
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_basic() {
        let mut cache = PolicyCache::new(10);

        assert!(cache.get("read", "/test").is_none());

        cache.put("read", "/test", true);
        assert_eq!(cache.get("read", "/test"), Some(true));

        cache.put("write", "/test", false);
        assert_eq!(cache.get("write", "/test"), Some(false));
    }

    #[test]
    fn test_cache_lru_eviction() {
        let mut cache = PolicyCache::new(2);

        cache.put("read", "/a", true);
        cache.put("read", "/b", true);
        cache.put("read", "/c", true); // Should evict /a

        assert!(cache.get("read", "/a").is_none()); // Evicted
        assert_eq!(cache.get("read", "/b"), Some(true));
        assert_eq!(cache.get("read", "/c"), Some(true));
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = PolicyCache::new(10);

        cache.put("read", "/test", true);
        assert_eq!(cache.len(), 1);

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_different_actions() {
        let mut cache = PolicyCache::new(10);

        cache.put("read", "/test", true);
        cache.put("write", "/test", false);

        assert_eq!(cache.get("read", "/test"), Some(true));
        assert_eq!(cache.get("write", "/test"), Some(false));
    }
}
