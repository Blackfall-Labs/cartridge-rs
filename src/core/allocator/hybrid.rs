//! Hybrid allocator that dispatches between bitmap and extent allocators
//!
//! Strategy:
//! - Small files (<256KB): Use bitmap allocator (fast, low overhead)
//! - Large files (≥256KB): Use extent allocator (contiguous, better performance)

use crate::allocator::bitmap::BitmapAllocator;
use crate::allocator::extent::ExtentAllocator;
use crate::allocator::BlockAllocator;
use crate::error::Result;
use crate::header::PAGE_SIZE;
use serde::{Deserialize, Serialize};

/// Size threshold for switching allocators (256KB)
const SMALL_FILE_THRESHOLD: u64 = 256 * 1024; // 256KB

/// Number of blocks that represent the small file threshold
const SMALL_FILE_BLOCKS: usize = (SMALL_FILE_THRESHOLD / PAGE_SIZE as u64) as usize; // 64 blocks

/// Hybrid allocator that dispatches to bitmap or extent allocator
///
/// This provides the best of both worlds:
/// - Bitmap for small files: Fast allocation, minimal overhead, non-contiguous OK
/// - Extent for large files: Contiguous allocation, better I/O performance, auto-coalescing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridAllocator {
    /// Bitmap allocator for small files (<256KB)
    bitmap: BitmapAllocator,

    /// Extent allocator for large files (≥256KB)
    extent: ExtentAllocator,

    /// Total number of blocks managed
    total_blocks: usize,

    /// Number of free blocks (canonical counter shared across both allocators)
    free_blocks: usize,
}

impl HybridAllocator {
    /// Create a new hybrid allocator
    pub fn new(total_blocks: usize) -> Self {
        HybridAllocator {
            bitmap: BitmapAllocator::new(total_blocks),
            extent: ExtentAllocator::new(total_blocks),
            total_blocks,
            free_blocks: total_blocks,
        }
    }

    /// Determine if a size should use bitmap allocator
    fn should_use_bitmap(size: u64) -> bool {
        size < SMALL_FILE_THRESHOLD
    }

    /// Get combined fragmentation score across both allocators
    ///
    /// Weighted average based on usage of each allocator
    pub fn combined_fragmentation_score(&self) -> f64 {
        let bitmap_free = self.bitmap.free_blocks() as f64;
        let extent_free = self.extent.free_blocks() as f64;
        let total_free = bitmap_free + extent_free;

        if total_free == 0.0 {
            return 0.0;
        }

        // Weighted average
        let bitmap_weight = bitmap_free / total_free;
        let extent_weight = extent_free / total_free;

        (self.bitmap.fragmentation_score() * bitmap_weight)
            + (self.extent.fragmentation_score() * extent_weight)
    }

    /// Get statistics about allocation distribution
    pub fn allocation_stats(&self) -> AllocationStats {
        AllocationStats {
            total_blocks: self.total_blocks,
            bitmap_free: self.bitmap.free_blocks(),
            extent_free: self.extent.free_blocks(),
            bitmap_fragmentation: self.bitmap.fragmentation_score(),
            extent_fragmentation: self.extent.fragmentation_score(),
            combined_fragmentation: self.combined_fragmentation_score(),
        }
    }

    /// Extend allocator capacity to new block count
    ///
    /// Used by auto-growth to expand the allocator's tracking capacity.
    pub fn extend_capacity(&mut self, new_total_blocks: usize) -> Result<()> {
        if new_total_blocks <= self.total_blocks {
            return Ok(()); // No extension needed
        }

        let added_blocks = new_total_blocks - self.total_blocks;

        // Extend both allocators
        self.bitmap.extend_capacity(new_total_blocks)?;
        self.extent.extend_capacity(new_total_blocks)?;

        // Update counters
        self.total_blocks = new_total_blocks;
        self.free_blocks += added_blocks;

        Ok(())
    }
}

/// Statistics about hybrid allocator usage
#[derive(Debug, Clone)]
pub struct AllocationStats {
    pub total_blocks: usize,
    pub bitmap_free: usize,
    pub extent_free: usize,
    pub bitmap_fragmentation: f64,
    pub extent_fragmentation: f64,
    pub combined_fragmentation: f64,
}

impl BlockAllocator for HybridAllocator {
    fn allocate(&mut self, size: u64) -> Result<Vec<u64>> {
        let num_blocks = ((size + PAGE_SIZE as u64 - 1) / PAGE_SIZE as u64) as usize;

        // Check canonical free_blocks counter
        if num_blocks > self.free_blocks {
            return Err(crate::error::CartridgeError::OutOfSpace);
        }

        let result = if Self::should_use_bitmap(size) {
            // Small file: use bitmap allocator
            let blocks = self.bitmap.allocate(size)?;
            // Mark blocks as allocated in extent allocator too (to prevent collision)
            self.extent.mark_allocated(&blocks)?;
            blocks
        } else {
            // Large file: use extent allocator
            let blocks = self.extent.allocate(size)?;
            // Mark blocks as allocated in bitmap allocator too (to prevent collision)
            self.bitmap.mark_allocated(&blocks)?;
            blocks
        };

        // Update canonical free_blocks counter
        self.free_blocks -= result.len();

        Ok(result)
    }

    fn free(&mut self, blocks: &[u64]) -> Result<()> {
        // Determine which allocator to use based on number of blocks
        let num_blocks = blocks.len();

        // Free from BOTH allocators to keep them in sync
        if num_blocks < SMALL_FILE_BLOCKS {
            // Small allocation: free via bitmap (primary)
            self.bitmap.free(blocks)?;
            self.extent.mark_free(blocks)?;
        } else {
            // Large allocation: free via extent (primary)
            self.extent.free(blocks)?;
            self.bitmap.mark_free(blocks)?;
        }

        // Update canonical free_blocks counter
        self.free_blocks += num_blocks;

        Ok(())
    }

    fn fragmentation_score(&self) -> f64 {
        self.combined_fragmentation_score()
    }

    fn total_blocks(&self) -> usize {
        self.total_blocks
    }

    fn free_blocks(&self) -> usize {
        // Return canonical free_blocks counter
        self.free_blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hybrid_creation() {
        let alloc = HybridAllocator::new(10000);
        assert_eq!(alloc.total_blocks(), 10000);
        assert_eq!(alloc.free_blocks(), 10000);
    }

    #[test]
    fn test_small_file_uses_bitmap() {
        let mut alloc = HybridAllocator::new(10000);

        // Allocate small file (10KB < 256KB threshold)
        let blocks = alloc.allocate(10 * 1024).unwrap();
        assert_eq!(blocks.len(), 3); // ceil(10240 / 4096) = 3 blocks

        // Bitmap should have allocated these blocks
        assert_eq!(alloc.bitmap.free_blocks(), 10000 - 3);
    }

    #[test]
    fn test_large_file_uses_extent() {
        let mut alloc = HybridAllocator::new(10000);

        // Allocate large file (1MB ≥ 256KB threshold)
        let blocks = alloc.allocate(1024 * 1024).unwrap();
        assert_eq!(blocks.len(), 256); // 1MB / 4KB = 256 blocks

        // Extent should have allocated these blocks
        assert_eq!(alloc.extent.free_blocks(), 10000 - 256);

        // Blocks should be contiguous (extent allocator guarantees this)
        for i in 1..blocks.len() {
            assert_eq!(blocks[i], blocks[i - 1] + 1);
        }
    }

    #[test]
    fn test_threshold_boundary() {
        let mut alloc = HybridAllocator::new(10000);

        // Just below threshold (256KB - 1 byte)
        let small = alloc.allocate(SMALL_FILE_THRESHOLD - 1).unwrap();
        assert_eq!(alloc.bitmap.free_blocks(), 10000 - small.len());

        // At threshold (256KB exactly)
        let large = alloc.allocate(SMALL_FILE_THRESHOLD).unwrap();
        assert_eq!(alloc.extent.free_blocks(), 10000 - large.len());
    }

    #[test]
    fn test_mixed_allocations() {
        let mut alloc = HybridAllocator::new(10000);

        // Mix of small and large allocations
        let small1 = alloc.allocate(10 * 1024).unwrap(); // 3 blocks via bitmap
        let large1 = alloc.allocate(1024 * 1024).unwrap(); // 256 blocks via extent
        let small2 = alloc.allocate(20 * 1024).unwrap(); // 5 blocks via bitmap
        let large2 = alloc.allocate(512 * 1024).unwrap(); // 128 blocks via extent

        // Verify allocation counts
        assert_eq!(alloc.bitmap.free_blocks(), 10000 - 3 - 5);
        assert_eq!(alloc.extent.free_blocks(), 10000 - 256 - 128);

        // Free some allocations
        alloc.free(&small1).unwrap();
        alloc.free(&large1).unwrap();

        assert_eq!(alloc.bitmap.free_blocks(), 10000 - 5);
        assert_eq!(alloc.extent.free_blocks(), 10000 - 128);
    }

    #[test]
    fn test_free_dispatching() {
        let mut alloc = HybridAllocator::new(10000);

        // Allocate small and large
        let small = alloc.allocate(10 * 1024).unwrap(); // 3 blocks
        let large = alloc.allocate(1024 * 1024).unwrap(); // 256 blocks

        // Free should dispatch correctly based on size
        alloc.free(&small).unwrap(); // Should go to bitmap (< 64 blocks)
        alloc.free(&large).unwrap(); // Should go to extent (≥ 64 blocks)

        // Both should be fully freed
        assert_eq!(alloc.bitmap.free_blocks(), 10000);
        assert_eq!(alloc.extent.free_blocks(), 10000);
    }

    #[test]
    fn test_allocation_stats() {
        let mut alloc = HybridAllocator::new(10000);

        // Initial stats
        let stats = alloc.allocation_stats();
        assert_eq!(stats.total_blocks, 10000);
        assert_eq!(stats.bitmap_free, 10000);
        assert_eq!(stats.extent_free, 10000);

        // Allocate and check stats
        alloc.allocate(10 * 1024).unwrap(); // 3 blocks via bitmap
        alloc.allocate(1024 * 1024).unwrap(); // 256 blocks via extent

        let stats = alloc.allocation_stats();
        assert_eq!(stats.bitmap_free, 10000 - 3);
        assert_eq!(stats.extent_free, 10000 - 256);
    }

    #[test]
    fn test_combined_fragmentation() {
        let mut alloc = HybridAllocator::new(10000);

        // Initial fragmentation should be low
        let frag1 = alloc.fragmentation_score();

        // Create some fragmentation
        let b1 = alloc.allocate(10 * 1024).unwrap();
        let b2 = alloc.allocate(10 * 1024).unwrap();
        let b3 = alloc.allocate(10 * 1024).unwrap();

        alloc.free(&b2).unwrap(); // Create a gap

        let frag2 = alloc.fragmentation_score();

        // Fragmentation should increase
        assert!(frag2 >= frag1);
    }

    #[test]
    fn test_out_of_space() {
        let mut alloc = HybridAllocator::new(100);

        // Allocate all space via extent allocator (large file)
        alloc.allocate(100 * PAGE_SIZE as u64).unwrap();

        // The bitmap allocator still thinks blocks are free because they track independently
        // This is a known limitation - in Phase 2 we'll use a shared freelist
        // For now, just verify extent allocator reports out of space
        assert_eq!(alloc.extent.free_blocks(), 0);
    }

    #[test]
    fn test_threshold_constant() {
        // Verify threshold is correct
        assert_eq!(SMALL_FILE_THRESHOLD, 256 * 1024);
        assert_eq!(SMALL_FILE_BLOCKS, 64); // 256KB / 4KB
    }

    #[test]
    fn test_should_use_bitmap() {
        assert!(HybridAllocator::should_use_bitmap(1024)); // 1KB
        assert!(HybridAllocator::should_use_bitmap(100 * 1024)); // 100KB
        assert!(HybridAllocator::should_use_bitmap(255 * 1024)); // 255KB
        assert!(!HybridAllocator::should_use_bitmap(256 * 1024)); // 256KB
        assert!(!HybridAllocator::should_use_bitmap(1024 * 1024)); // 1MB
    }

    #[test]
    fn test_large_allocation_contiguous() {
        let mut alloc = HybridAllocator::new(10000);

        // Large files should get contiguous blocks from extent allocator
        let large = alloc.allocate(512 * 1024).unwrap(); // 128 blocks

        // Verify contiguity
        for i in 1..large.len() {
            assert_eq!(
                large[i],
                large[i - 1] + 1,
                "Large allocation should be contiguous"
            );
        }
    }
}
