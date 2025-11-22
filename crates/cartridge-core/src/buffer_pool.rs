//! Adaptive Replacement Cache (ARC) buffer pool
//!
//! Provides intelligent page caching with better hit rates than LRU.
//! ARC adapts to workload patterns by maintaining two LRU lists:
//! - T1: Recently accessed pages (once)
//! - T2: Frequently accessed pages (multiple times)
//! - B1: Ghost entries evicted from T1
//! - B2: Ghost entries evicted from T2
//!
//! The algorithm self-tunes parameter `p` based on workload to balance
//! recency vs. frequency, achieving near-optimal cache hit rates.

use crate::page::Page;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// LRU list for cache entries
#[derive(Debug)]
struct LruList {
    /// List of page IDs (front = MRU, back = LRU)
    list: VecDeque<u64>,
    /// Set for fast contains checks
    set: HashMap<u64, ()>,
}

impl LruList {
    fn new() -> Self {
        LruList {
            list: VecDeque::new(),
            set: HashMap::new(),
        }
    }

    fn push_front(&mut self, page_id: u64) {
        if !self.set.contains_key(&page_id) {
            self.list.push_front(page_id);
            self.set.insert(page_id, ());
        }
    }

    fn remove(&mut self, page_id: u64) -> bool {
        if self.set.remove(&page_id).is_some() {
            self.list.retain(|&id| id != page_id);
            true
        } else {
            false
        }
    }

    fn pop_back(&mut self) -> Option<u64> {
        if let Some(page_id) = self.list.pop_back() {
            self.set.remove(&page_id);
            Some(page_id)
        } else {
            None
        }
    }

    fn contains(&self, page_id: u64) -> bool {
        self.set.contains_key(&page_id)
    }

    fn move_to_front(&mut self, page_id: u64) {
        if self.remove(page_id) {
            self.push_front(page_id);
        }
    }

    fn len(&self) -> usize {
        self.list.len()
    }

    fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

/// Ghost list (metadata only, no actual pages)
#[derive(Debug)]
struct GhostList {
    /// Page IDs that were recently evicted
    list: VecDeque<u64>,
    /// Set for fast contains checks
    set: HashMap<u64, ()>,
}

impl GhostList {
    fn new() -> Self {
        GhostList {
            list: VecDeque::new(),
            set: HashMap::new(),
        }
    }

    fn push_front(&mut self, page_id: u64) {
        if !self.set.contains_key(&page_id) {
            self.list.push_front(page_id);
            self.set.insert(page_id, ());
        }
    }

    fn remove(&mut self, page_id: u64) -> bool {
        if self.set.remove(&page_id).is_some() {
            self.list.retain(|&id| id != page_id);
            true
        } else {
            false
        }
    }

    fn pop_back(&mut self) -> Option<u64> {
        if let Some(page_id) = self.list.pop_back() {
            self.set.remove(&page_id);
            Some(page_id)
        } else {
            None
        }
    }

    fn contains(&self, page_id: u64) -> bool {
        self.set.contains_key(&page_id)
    }

    fn len(&self) -> usize {
        self.list.len()
    }
}

/// ARC Buffer Pool Statistics
#[derive(Debug, Clone, Copy)]
pub struct BufferPoolStats {
    /// Total cache hits
    pub hits: u64,
    /// Total cache misses
    pub misses: u64,
    /// Current number of pages in T1
    pub t1_size: usize,
    /// Current number of pages in T2
    pub t2_size: usize,
    /// Current number of ghost entries in B1
    pub b1_size: usize,
    /// Current number of ghost entries in B2
    pub b2_size: usize,
    /// Current adaptive parameter (target size for T1)
    pub p: usize,
    /// Total capacity
    pub capacity: usize,
}

impl BufferPoolStats {
    /// Calculate hit rate as a percentage
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            (self.hits as f64 / total as f64) * 100.0
        }
    }
}

/// Adaptive Replacement Cache buffer pool
pub struct BufferPool {
    /// Recently accessed once (target size: p)
    t1: LruList,
    /// Frequently accessed (target size: c - p)
    t2: LruList,
    /// Ghost entries evicted from T1
    b1: GhostList,
    /// Ghost entries evicted from T2
    b2: GhostList,
    /// Adaptive parameter (0 ≤ p ≤ c)
    p: usize,
    /// Total cache capacity
    capacity: usize,
    /// Actual page storage
    pages: HashMap<u64, Arc<Page>>,
    /// Statistics
    hits: u64,
    misses: u64,
}

impl BufferPool {
    /// Create a new ARC buffer pool
    ///
    /// # Arguments
    /// * `capacity` - Maximum number of pages to cache
    pub fn new(capacity: usize) -> Self {
        BufferPool {
            t1: LruList::new(),
            t2: LruList::new(),
            b1: GhostList::new(),
            b2: GhostList::new(),
            p: 0,
            capacity,
            pages: HashMap::new(),
            hits: 0,
            misses: 0,
        }
    }

    /// Get a page from the cache
    ///
    /// # Returns
    /// - `Some(page)` if page is in cache (also updates ARC metadata)
    /// - `None` if page is not in cache (caller must load from disk)
    pub fn get(&mut self, page_id: u64) -> Option<Arc<Page>> {
        if let Some(page) = self.pages.get(&page_id).cloned() {
            // Cache hit in T1 or T2
            self.hits += 1;
            self.on_hit(page_id);
            Some(page)
        } else if self.b1.contains(page_id) {
            // Hit in B1 (ghost) - page was in T1 but evicted
            // Increase p to favor recency
            self.misses += 1;
            let delta = if self.b1.len() >= self.b2.len() {
                1
            } else {
                self.b2.len() / self.b1.len()
            };
            self.p = std::cmp::min(self.p + delta, self.capacity);
            self.replace(page_id);
            self.b1.remove(page_id);
            None
        } else if self.b2.contains(page_id) {
            // Hit in B2 (ghost) - page was in T2 but evicted
            // Decrease p to favor frequency
            self.misses += 1;
            let delta = if self.b2.len() >= self.b1.len() {
                1
            } else {
                self.b1.len() / self.b2.len()
            };
            self.p = self.p.saturating_sub(delta);
            self.replace(page_id);
            self.b2.remove(page_id);
            None
        } else {
            // Complete miss
            self.misses += 1;
            None
        }
    }

    /// Put a page into the cache
    ///
    /// # Arguments
    /// * `page_id` - Page identifier
    /// * `page` - Page data (wrapped in Arc for cheap cloning)
    pub fn put(&mut self, page_id: u64, page: Arc<Page>) {
        // If cache is full, evict according to ARC policy
        if self.pages.len() >= self.capacity && !self.pages.contains_key(&page_id) {
            self.replace(page_id);
        }

        // Insert the page
        self.pages.insert(page_id, page);

        // Add to T1 (first access)
        self.t1.push_front(page_id);

        // Maintain ghost list sizes
        self.maintain_ghost_lists();
    }

    /// Handle cache hit - move page between T1 and T2
    fn on_hit(&mut self, page_id: u64) {
        if self.t1.remove(page_id) {
            // Second access - move from T1 to T2
            self.t2.push_front(page_id);
        } else if self.t2.contains(page_id) {
            // Subsequent access - move to front of T2
            self.t2.move_to_front(page_id);
        }
    }

    /// Replace a page according to ARC policy
    fn replace(&mut self, _page_id: u64) {
        if self.t1.len() > 0
            && ((self.t1.len() > self.p) || (self.b2.contains(_page_id) && self.t1.len() == self.p))
        {
            // Evict from T1
            if let Some(evicted_id) = self.t1.pop_back() {
                self.pages.remove(&evicted_id);
                self.b1.push_front(evicted_id);
            }
        } else {
            // Evict from T2
            if let Some(evicted_id) = self.t2.pop_back() {
                self.pages.remove(&evicted_id);
                self.b2.push_front(evicted_id);
            }
        }
    }

    /// Maintain ghost list sizes (B1 + B2 ≤ 2c)
    fn maintain_ghost_lists(&mut self) {
        let total_ghost = self.b1.len() + self.b2.len();
        let max_ghost = 2 * self.capacity;

        while total_ghost > max_ghost {
            if self.b1.len() > self.b2.len() {
                self.b1.pop_back();
            } else {
                self.b2.pop_back();
            }
        }
    }

    /// Get buffer pool statistics
    pub fn stats(&self) -> BufferPoolStats {
        BufferPoolStats {
            hits: self.hits,
            misses: self.misses,
            t1_size: self.t1.len(),
            t2_size: self.t2.len(),
            b1_size: self.b1.len(),
            b2_size: self.b2.len(),
            p: self.p,
            capacity: self.capacity,
        }
    }

    /// Clear all cached pages
    pub fn clear(&mut self) {
        self.t1 = LruList::new();
        self.t2 = LruList::new();
        self.b1 = GhostList::new();
        self.b2 = GhostList::new();
        self.pages.clear();
        self.p = 0;
        self.hits = 0;
        self.misses = 0;
    }

    /// Get current cache size
    pub fn len(&self) -> usize {
        self.pages.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::PageType;

    fn create_test_page(_page_id: u64) -> Arc<Page> {
        // Note: Page doesn't store its ID internally - the buffer pool
        // manages the page_id -> page mapping
        let page = Page::new(PageType::ContentData);
        Arc::new(page)
    }

    #[test]
    fn test_buffer_pool_creation() {
        let pool = BufferPool::new(100);
        assert_eq!(pool.capacity, 100);
        assert_eq!(pool.len(), 0);
        assert!(pool.is_empty());
    }

    #[test]
    fn test_simple_put_get() {
        let mut pool = BufferPool::new(10);

        let page = create_test_page(1);
        pool.put(1, Arc::clone(&page));

        assert_eq!(pool.len(), 1);

        let retrieved = pool.get(1);
        assert!(retrieved.is_some());

        let stats = pool.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 0);
    }

    #[test]
    fn test_cache_miss() {
        let mut pool = BufferPool::new(10);

        let result = pool.get(999);
        assert!(result.is_none());

        let stats = pool.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn test_promotion_to_t2() {
        let mut pool = BufferPool::new(10);

        let page = create_test_page(1);
        pool.put(1, page);

        // First access - should be in T1
        pool.get(1);
        assert_eq!(pool.t2.len(), 1); // Should have moved to T2

        // Second access - should still be in T2
        pool.get(1);
        assert_eq!(pool.t2.len(), 1);
    }

    #[test]
    fn test_eviction() {
        let mut pool = BufferPool::new(3);

        // Fill cache
        for i in 0..3 {
            pool.put(i, create_test_page(i));
        }

        assert_eq!(pool.len(), 3);

        // Add one more - should evict
        pool.put(3, create_test_page(3));
        assert_eq!(pool.len(), 3); // Still at capacity

        // Check that one page was evicted
        let stats = pool.stats();
        assert!(stats.b1_size > 0 || stats.b2_size > 0);
    }

    #[test]
    fn test_ghost_list_adaptation() {
        let mut pool = BufferPool::new(2);

        // Access pattern that should adapt p
        pool.put(1, create_test_page(1));
        pool.put(2, create_test_page(2));

        // Evict page 1
        pool.put(3, create_test_page(3));

        // Access evicted page 1 (now in B1)
        let result = pool.get(1);
        assert!(result.is_none()); // Not in cache

        // p should have increased (favoring recency)
        let stats = pool.stats();
        assert!(stats.p > 0);
    }

    #[test]
    fn test_clear() {
        let mut pool = BufferPool::new(10);

        pool.put(1, create_test_page(1));
        pool.put(2, create_test_page(2));

        pool.clear();

        assert_eq!(pool.len(), 0);
        assert!(pool.is_empty());

        let stats = pool.stats();
        assert_eq!(stats.t1_size, 0);
        assert_eq!(stats.t2_size, 0);
        assert_eq!(stats.b1_size, 0);
        assert_eq!(stats.b2_size, 0);
    }

    #[test]
    fn test_hit_rate_calculation() {
        let mut pool = BufferPool::new(10);

        pool.put(1, create_test_page(1));

        pool.get(1); // Hit
        pool.get(2); // Miss
        pool.get(1); // Hit

        let stats = pool.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate() - 66.66).abs() < 0.1);
    }

    #[test]
    fn test_sequential_access_pattern() {
        let mut pool = BufferPool::new(5);

        // Sequential scan - should fill cache
        for i in 0..5 {
            pool.put(i, create_test_page(i));
        }

        // All should be accessible
        for i in 0..5 {
            assert!(pool.get(i).is_some());
        }

        let stats = pool.stats();
        assert_eq!(stats.hits, 5);
    }

    #[test]
    fn test_random_access_pattern() {
        let mut pool = BufferPool::new(10);

        // Random access pattern
        let pattern = vec![1, 5, 2, 1, 3, 5, 1, 2, 4, 1];

        for &page_id in &pattern {
            if pool.get(page_id).is_none() {
                pool.put(page_id, create_test_page(page_id));
            }
        }

        // Frequently accessed pages should be in cache
        assert!(pool.get(1).is_some()); // Most frequent
        assert!(pool.get(5).is_some());
        assert!(pool.get(2).is_some());
    }
}
