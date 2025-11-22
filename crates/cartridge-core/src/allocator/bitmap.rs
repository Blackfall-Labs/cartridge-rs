//! Bitmap allocator for small files (<256KB)
//!
//! Uses a multi-level bitmap with <2% overhead for tracking free blocks.
//! Each bit represents one 4KB block.

use crate::allocator::BlockAllocator;
use crate::error::{CartridgeError, Result};
use crate::header::PAGE_SIZE;
use serde::{Deserialize, Serialize};

/// Bitmap allocator for small file blocks
///
/// Represents free/allocated state with bits:
/// - 0 = free block
/// - 1 = allocated block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitmapAllocator {
    /// Bitmap words (each word = 64 bits = 64 blocks)
    bitmap: Vec<u64>,

    /// Total number of blocks tracked
    total_blocks: usize,

    /// Number of free blocks available
    free_blocks: usize,
}

impl BitmapAllocator {
    /// Create a new bitmap allocator
    pub fn new(total_blocks: usize) -> Self {
        let num_words = (total_blocks + 63) / 64;
        BitmapAllocator {
            bitmap: vec![0u64; num_words],
            total_blocks,
            free_blocks: total_blocks,
        }
    }

    /// Allocate a specific number of blocks
    ///
    /// Blocks don't need to be contiguous. Returns block IDs that were allocated.
    pub fn allocate_blocks(&mut self, num_blocks: usize) -> Result<Vec<u64>> {
        if num_blocks > self.free_blocks {
            return Err(CartridgeError::OutOfSpace);
        }

        let mut allocated = Vec::with_capacity(num_blocks);

        'outer: for (word_idx, word) in self.bitmap.iter_mut().enumerate() {
            if *word == u64::MAX {
                continue; // All bits set (all allocated)
            }

            // Find free bits in this word
            for bit_idx in 0..64 {
                if allocated.len() == num_blocks {
                    break 'outer;
                }

                if (*word & (1u64 << bit_idx)) == 0 {
                    // Free block found
                    let block_id = (word_idx * 64 + bit_idx) as u64;

                    // Don't allocate beyond total_blocks
                    if block_id >= self.total_blocks as u64 {
                        break 'outer;
                    }

                    allocated.push(block_id);
                    *word |= 1u64 << bit_idx; // Mark as allocated
                }
            }
        }

        if allocated.len() != num_blocks {
            // Rollback allocations
            for &block_id in &allocated {
                let word_idx = (block_id / 64) as usize;
                let bit_idx = (block_id % 64) as usize;
                self.bitmap[word_idx] &= !(1u64 << bit_idx);
            }
            return Err(CartridgeError::OutOfSpace);
        }

        self.free_blocks -= num_blocks;
        Ok(allocated)
    }

    /// Free previously allocated blocks
    pub fn free_allocated_blocks(&mut self, blocks: &[u64]) -> Result<()> {
        for &block_id in blocks {
            if block_id >= self.total_blocks as u64 {
                return Err(CartridgeError::InvalidBlockId(block_id));
            }

            let word_idx = (block_id / 64) as usize;
            let bit_idx = (block_id % 64) as usize;

            // Check if already free
            if (self.bitmap[word_idx] & (1u64 << bit_idx)) == 0 {
                // Already free - this is a double-free bug
                tracing::warn!("Double-free detected for block {}", block_id);
                continue;
            }

            self.bitmap[word_idx] &= !(1u64 << bit_idx); // Clear bit
        }

        self.free_blocks += blocks.len();
        Ok(())
    }

    /// Check if a specific block is allocated
    pub fn is_allocated(&self, block_id: u64) -> bool {
        if block_id >= self.total_blocks as u64 {
            return false;
        }

        let word_idx = (block_id / 64) as usize;
        let bit_idx = (block_id % 64) as usize;

        (self.bitmap[word_idx] & (1u64 << bit_idx)) != 0
    }

    /// Extend bitmap capacity to track more blocks
    ///
    /// Used by auto-growth to expand the allocator's capacity.
    pub fn extend_capacity(&mut self, new_total_blocks: usize) -> Result<()> {
        if new_total_blocks <= self.total_blocks {
            return Ok(()); // No need to extend
        }

        let new_num_words = (new_total_blocks + 63) / 64;
        let old_num_words = self.bitmap.len();

        // Extend bitmap with zeros (representing free blocks)
        self.bitmap.resize(new_num_words, 0u64);

        let added_blocks = new_total_blocks - self.total_blocks;
        self.total_blocks = new_total_blocks;
        self.free_blocks += added_blocks;

        Ok(())
    }
}

impl BlockAllocator for BitmapAllocator {
    fn allocate(&mut self, size: u64) -> Result<Vec<u64>> {
        let num_blocks = ((size + PAGE_SIZE as u64 - 1) / PAGE_SIZE as u64) as usize;
        self.allocate_blocks(num_blocks)
    }

    fn free(&mut self, blocks: &[u64]) -> Result<()> {
        self.free_allocated_blocks(blocks)
    }

    fn fragmentation_score(&self) -> f64 {
        // For bitmap allocator, fragmentation is based on how scattered allocations are
        // We count the number of transitions from free→allocated
        let mut transitions = 0usize;
        let mut prev_allocated = false;

        for &word in &self.bitmap {
            for bit_idx in 0..64 {
                let is_allocated = (word & (1u64 << bit_idx)) != 0;
                if is_allocated != prev_allocated {
                    transitions += 1;
                }
                prev_allocated = is_allocated;
            }
        }

        // Normalize by total blocks
        // Perfect allocation = few transitions, fragmented = many transitions
        (transitions as f64) / (self.total_blocks as f64)
    }

    fn total_blocks(&self) -> usize {
        self.total_blocks
    }

    fn free_blocks(&self) -> usize {
        self.free_blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitmap_creation() {
        let alloc = BitmapAllocator::new(1000);
        assert_eq!(alloc.total_blocks(), 1000);
        assert_eq!(alloc.free_blocks(), 1000);
    }

    #[test]
    fn test_bitmap_allocation() {
        let mut alloc = BitmapAllocator::new(1000);

        // Allocate 10 blocks
        let blocks = alloc.allocate_blocks(10).unwrap();
        assert_eq!(blocks.len(), 10);
        assert_eq!(alloc.free_blocks(), 990);

        // Verify blocks are marked allocated
        for &block_id in &blocks {
            assert!(alloc.is_allocated(block_id));
        }
    }

    #[test]
    fn test_bitmap_free() {
        let mut alloc = BitmapAllocator::new(1000);

        let blocks = alloc.allocate_blocks(10).unwrap();
        assert_eq!(alloc.free_blocks(), 990);

        alloc.free_allocated_blocks(&blocks).unwrap();
        assert_eq!(alloc.free_blocks(), 1000);

        // Verify blocks are freed
        for &block_id in &blocks {
            assert!(!alloc.is_allocated(block_id));
        }
    }

    #[test]
    fn test_bitmap_out_of_space() {
        let mut alloc = BitmapAllocator::new(10);

        // Allocate all blocks
        let blocks = alloc.allocate_blocks(10).unwrap();
        assert_eq!(alloc.free_blocks(), 0);

        // Try to allocate more
        let result = alloc.allocate_blocks(1);
        assert!(matches!(result, Err(CartridgeError::OutOfSpace)));
    }

    #[test]
    fn test_bitmap_invalid_block_id() {
        let mut alloc = BitmapAllocator::new(100);

        let result = alloc.free_allocated_blocks(&[1000]); // Beyond range
        assert!(matches!(result, Err(CartridgeError::InvalidBlockId(_))));
    }

    #[test]
    fn test_bitmap_via_trait() {
        let mut alloc = BitmapAllocator::new(1000);

        // Allocate 4KB (1 block)
        let blocks = alloc.allocate(4096).unwrap();
        assert_eq!(blocks.len(), 1);

        // Allocate 10KB (3 blocks: ceil(10240 / 4096))
        let blocks2 = alloc.allocate(10240).unwrap();
        assert_eq!(blocks2.len(), 3);

        assert_eq!(alloc.free_blocks(), 996);
    }

    #[test]
    fn test_fragmentation_score() {
        let mut alloc = BitmapAllocator::new(1000);

        // No allocation = no fragmentation
        let score1 = alloc.fragmentation_score();

        // Allocate some blocks
        alloc.allocate_blocks(100).unwrap();
        let score2 = alloc.fragmentation_score();

        // Fragmentation should increase (some transitions exist)
        assert!(score2 > score1);
    }
}
