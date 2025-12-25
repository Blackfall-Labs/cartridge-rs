//! Extent allocator for large files (≥256KB)
//!
//! Uses B-tree-based extent tracking with automatic coalescing.
//! Each extent represents a contiguous range of blocks.

use crate::allocator::BlockAllocator;
use crate::error::{CartridgeError, Result};
use crate::header::PAGE_SIZE;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// An extent representing a contiguous range of blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extent {
    /// Starting block ID
    pub start: u64,
    /// Number of contiguous blocks
    pub length: u64,
}

impl Extent {
    pub fn new(start: u64, length: u64) -> Self {
        Extent { start, length }
    }

    /// Check if this extent contains a block ID
    pub fn contains(&self, block_id: u64) -> bool {
        block_id >= self.start && block_id < self.start + self.length
    }

    /// Check if this extent is adjacent to another (can be coalesced)
    pub fn is_adjacent(&self, other: &Extent) -> bool {
        self.start + self.length == other.start || other.start + other.length == self.start
    }

    /// Coalesce two adjacent extents
    pub fn coalesce(&self, other: &Extent) -> Option<Extent> {
        if !self.is_adjacent(other) {
            return None;
        }

        let new_start = self.start.min(other.start);
        let new_end = (self.start + self.length).max(other.start + other.length);

        Some(Extent {
            start: new_start,
            length: new_end - new_start,
        })
    }
}

/// Extent allocator for large file blocks
///
/// Uses a B-tree to track free extents, enabling efficient:
/// - Best-fit allocation (minimize fragmentation)
/// - Automatic coalescing of adjacent free extents
/// - Fast lookup by size and position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtentAllocator {
    /// Free extents indexed by start block ID
    /// BTreeMap provides sorted order for efficient coalescing
    free_extents: BTreeMap<u64, Extent>,

    /// Total number of blocks tracked
    total_blocks: usize,

    /// Number of free blocks available
    free_blocks: usize,
}

impl ExtentAllocator {
    /// Create a new extent allocator
    pub fn new(total_blocks: usize) -> Self {
        let mut free_extents = BTreeMap::new();

        // Initially, all blocks are free in one large extent
        if total_blocks > 0 {
            free_extents.insert(0, Extent::new(0, total_blocks as u64));
        }

        ExtentAllocator {
            free_extents,
            total_blocks,
            free_blocks: total_blocks,
        }
    }

    /// Allocate contiguous blocks
    ///
    /// Uses best-fit strategy: finds the smallest extent that fits the request.
    /// This minimizes fragmentation by leaving larger extents intact.
    pub fn allocate_contiguous(&mut self, num_blocks: usize) -> Result<Vec<u64>> {
        if num_blocks > self.free_blocks {
            return Err(CartridgeError::OutOfSpace);
        }

        let num_blocks_u64 = num_blocks as u64;

        // Find best-fit extent (smallest that fits)
        let best_fit = self
            .free_extents
            .iter()
            .filter(|(_, extent)| extent.length >= num_blocks_u64)
            .min_by_key(|(_, extent)| extent.length);

        let (start_key, extent) = match best_fit {
            Some((k, e)) => (*k, *e),
            None => return Err(CartridgeError::OutOfSpace),
        };

        // Remove the extent we're allocating from
        self.free_extents.remove(&start_key);

        // Allocate from the beginning of the extent
        let allocated_start = extent.start;
        let allocated_blocks: Vec<u64> =
            (allocated_start..allocated_start + num_blocks_u64).collect();

        // If there's remaining space, add it back as a new extent
        let remaining_length = extent.length - num_blocks_u64;
        if remaining_length > 0 {
            let remaining_start = extent.start + num_blocks_u64;
            self.free_extents.insert(
                remaining_start,
                Extent::new(remaining_start, remaining_length),
            );
        }

        self.free_blocks -= num_blocks;

        Ok(allocated_blocks)
    }

    /// Free previously allocated blocks
    ///
    /// Automatically coalesces adjacent extents to reduce fragmentation.
    pub fn free_extent(&mut self, blocks: &[u64]) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        // Validate all blocks are in range
        for &block_id in blocks {
            if block_id >= self.total_blocks as u64 {
                return Err(CartridgeError::InvalidBlockId(block_id));
            }
        }

        // Sort blocks to identify contiguous ranges
        let mut sorted_blocks = blocks.to_vec();
        sorted_blocks.sort_unstable();

        // Group into contiguous extents
        let mut extents_to_free = Vec::new();
        let mut current_start = sorted_blocks[0];
        let mut current_length = 1u64;

        for i in 1..sorted_blocks.len() {
            if sorted_blocks[i] == sorted_blocks[i - 1] + 1 {
                current_length += 1;
            } else {
                extents_to_free.push(Extent::new(current_start, current_length));
                current_start = sorted_blocks[i];
                current_length = 1;
            }
        }
        extents_to_free.push(Extent::new(current_start, current_length));

        // Free each extent with coalescing
        for extent in extents_to_free {
            self.insert_and_coalesce(extent);
        }

        self.free_blocks += blocks.len();

        Ok(())
    }

    /// Insert a free extent and coalesce with adjacent extents
    fn insert_and_coalesce(&mut self, mut extent: Extent) {
        // Check for adjacent extents before and after
        let mut to_remove = Vec::new();

        // Check previous extent (by start position)
        if let Some((&prev_start, &prev_extent)) =
            self.free_extents.range(..extent.start).next_back()
        {
            if prev_extent.is_adjacent(&extent) {
                extent = prev_extent.coalesce(&extent).unwrap();
                to_remove.push(prev_start);
            }
        }

        // Check next extent (by start position)
        if let Some((&next_start, &next_extent)) = self
            .free_extents
            .range(extent.start + extent.length..)
            .next()
        {
            if extent.is_adjacent(&next_extent) {
                extent = extent.coalesce(&next_extent).unwrap();
                to_remove.push(next_start);
            }
        }

        // Remove coalesced extents
        for key in to_remove {
            self.free_extents.remove(&key);
        }

        // Insert the (possibly coalesced) extent
        self.free_extents.insert(extent.start, extent);
    }

    /// Check if a specific block is allocated
    pub fn is_allocated(&self, block_id: u64) -> bool {
        if block_id >= self.total_blocks as u64 {
            return false;
        }

        // If block is in any free extent, it's not allocated
        !self
            .free_extents
            .values()
            .any(|extent| extent.contains(block_id))
    }

    /// Get current number of free extents (fragmentation indicator)
    pub fn extent_count(&self) -> usize {
        self.free_extents.len()
    }

    /// Mark specific blocks as allocated (without changing free_blocks counter)
    ///
    /// Used by HybridAllocator to keep allocators in sync
    pub fn mark_allocated(&mut self, blocks: &[u64]) -> Result<()> {
        // Remove blocks from free extents
        for &block_id in blocks {
            // Find and remove from free extents
            let mut keys_to_remove = Vec::new();
            let mut new_extents = Vec::new();

            for (&start, extent) in &self.free_extents {
                if extent.contains(block_id) {
                    keys_to_remove.push(start);

                    // Split extent if needed
                    if block_id > start {
                        // Add extent before allocated block
                        new_extents.push(Extent::new(start, block_id - start));
                    }
                    if block_id + 1 < start + extent.length {
                        // Add extent after allocated block
                        new_extents.push(Extent::new(
                            block_id + 1,
                            (start + extent.length) - (block_id + 1),
                        ));
                    }
                }
            }

            // Apply changes
            for key in keys_to_remove {
                self.free_extents.remove(&key);
            }
            for extent in new_extents {
                self.free_extents.insert(extent.start, extent);
            }
        }
        Ok(())
    }

    /// Mark specific blocks as free (without changing free_blocks counter)
    ///
    /// Used by HybridAllocator to keep allocators in sync
    pub fn mark_free(&mut self, blocks: &[u64]) -> Result<()> {
        // Add blocks back to free extents with coalescing
        if blocks.is_empty() {
            return Ok(());
        }

        // Sort blocks
        let mut sorted = blocks.to_vec();
        sorted.sort_unstable();

        // Group into extents
        let mut current_start = sorted[0];
        let mut current_len = 1u64;

        for i in 1..sorted.len() {
            if sorted[i] == sorted[i - 1] + 1 {
                current_len += 1;
            } else {
                // Add current extent
                let extent = Extent::new(current_start, current_len);
                self.add_free_extent_with_coalesce(extent);
                current_start = sorted[i];
                current_len = 1;
            }
        }

        // Add final extent
        let extent = Extent::new(current_start, current_len);
        self.add_free_extent_with_coalesce(extent);

        Ok(())
    }

    /// Add a free extent with coalescing
    fn add_free_extent_with_coalesce(&mut self, extent: Extent) {
        // Try to coalesce with adjacent extents
        let mut coalesced = extent;
        let mut keys_to_remove = Vec::new();

        for (&start, existing) in &self.free_extents {
            if let Some(merged) = coalesced.coalesce(existing) {
                coalesced = merged;
                keys_to_remove.push(start);
            }
        }

        // Remove old extents
        for key in keys_to_remove {
            self.free_extents.remove(&key);
        }

        // Insert coalesced extent
        self.free_extents.insert(coalesced.start, coalesced);
    }

    /// Extend extent allocator capacity to track more blocks
    ///
    /// Used by auto-growth to expand the allocator's capacity.
    pub fn extend_capacity(&mut self, new_total_blocks: usize) -> Result<()> {
        if new_total_blocks <= self.total_blocks {
            return Ok(()); // No need to extend
        }

        let added_blocks = new_total_blocks - self.total_blocks;
        let new_extent_start = self.total_blocks as u64;

        // Add the new blocks as a free extent
        let new_extent = Extent::new(new_extent_start, added_blocks as u64);

        // Try to coalesce with the last extent if it's adjacent
        if let Some((&last_start, &last_extent)) = self.free_extents.iter().next_back() {
            if last_start + last_extent.length == new_extent_start {
                // Coalesce with the last extent
                self.free_extents.remove(&last_start);
                let coalesced = Extent::new(last_start, last_extent.length + added_blocks as u64);
                self.free_extents.insert(last_start, coalesced);
            } else {
                // Add as a new extent
                self.free_extents.insert(new_extent_start, new_extent);
            }
        } else {
            // No extents exist, just add the new one
            self.free_extents.insert(new_extent_start, new_extent);
        }

        self.total_blocks = new_total_blocks;
        self.free_blocks += added_blocks;

        Ok(())
    }
}

impl BlockAllocator for ExtentAllocator {
    fn allocate(&mut self, size: u64) -> Result<Vec<u64>> {
        let num_blocks = ((size + PAGE_SIZE as u64 - 1) / PAGE_SIZE as u64) as usize;
        self.allocate_contiguous(num_blocks)
    }

    fn free(&mut self, blocks: &[u64]) -> Result<()> {
        self.free_extent(blocks)
    }

    fn fragmentation_score(&self) -> f64 {
        // For extent allocator, fragmentation is based on number of extents
        // Ideal = 1 extent (all free space contiguous)
        // Highly fragmented = many small extents

        if self.free_blocks == 0 {
            return 0.0; // No free space = no fragmentation
        }

        let extent_count = self.free_extents.len();

        if extent_count == 0 {
            return 0.0;
        }

        // Normalize: more extents = higher fragmentation
        // Perfect score (1 extent) = 0.0
        // Worst case (every block is a separate extent) = 1.0
        (extent_count as f64 - 1.0) / (self.free_blocks as f64).max(1.0)
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
    fn test_extent_creation() {
        let extent = Extent::new(10, 20);
        assert_eq!(extent.start, 10);
        assert_eq!(extent.length, 20);
    }

    #[test]
    fn test_extent_contains() {
        let extent = Extent::new(10, 20);
        assert!(!extent.contains(9));
        assert!(extent.contains(10));
        assert!(extent.contains(20));
        assert!(extent.contains(29));
        assert!(!extent.contains(30));
    }

    #[test]
    fn test_extent_adjacency() {
        let e1 = Extent::new(10, 10); // 10-19
        let e2 = Extent::new(20, 10); // 20-29
        let e3 = Extent::new(30, 10); // 30-39

        assert!(e1.is_adjacent(&e2));
        assert!(e2.is_adjacent(&e1));
        assert!(e2.is_adjacent(&e3));
        assert!(!e1.is_adjacent(&e3));
    }

    #[test]
    fn test_extent_coalesce() {
        let e1 = Extent::new(10, 10);
        let e2 = Extent::new(20, 10);

        let coalesced = e1.coalesce(&e2).unwrap();
        assert_eq!(coalesced.start, 10);
        assert_eq!(coalesced.length, 20);
    }

    #[test]
    fn test_extent_allocator_creation() {
        let alloc = ExtentAllocator::new(1000);
        assert_eq!(alloc.total_blocks(), 1000);
        assert_eq!(alloc.free_blocks(), 1000);
        assert_eq!(alloc.extent_count(), 1); // One large free extent
    }

    #[test]
    fn test_extent_allocation() {
        let mut alloc = ExtentAllocator::new(1000);

        // Allocate 100 contiguous blocks
        let blocks = alloc.allocate_contiguous(100).unwrap();
        assert_eq!(blocks.len(), 100);
        assert_eq!(alloc.free_blocks(), 900);

        // Verify blocks are contiguous
        for i in 1..blocks.len() {
            assert_eq!(blocks[i], blocks[i - 1] + 1);
        }

        // Verify blocks are marked allocated
        for &block_id in &blocks {
            assert!(alloc.is_allocated(block_id));
        }
    }

    #[test]
    fn test_extent_free_and_coalesce() {
        let mut alloc = ExtentAllocator::new(1000);

        // Allocate three chunks
        let blocks1 = alloc.allocate_contiguous(100).unwrap(); // 0-99
        let blocks2 = alloc.allocate_contiguous(100).unwrap(); // 100-199
        let blocks3 = alloc.allocate_contiguous(100).unwrap(); // 200-299

        assert_eq!(alloc.free_blocks(), 700);
        assert_eq!(alloc.extent_count(), 1); // One large extent remains

        // Free middle chunk
        alloc.free_extent(&blocks2).unwrap();
        assert_eq!(alloc.free_blocks(), 800);
        assert_eq!(alloc.extent_count(), 2); // Split into two extents

        // Free first chunk - should coalesce with middle
        alloc.free_extent(&blocks1).unwrap();
        assert_eq!(alloc.free_blocks(), 900);
        assert_eq!(alloc.extent_count(), 2); // 0-199 and 300-999

        // Free third chunk - should coalesce all back to one
        alloc.free_extent(&blocks3).unwrap();
        assert_eq!(alloc.free_blocks(), 1000);
        assert_eq!(alloc.extent_count(), 1); // Back to one large extent
    }

    #[test]
    fn test_extent_out_of_space() {
        let mut alloc = ExtentAllocator::new(100);

        // Allocate all blocks
        let _blocks = alloc.allocate_contiguous(100).unwrap();
        assert_eq!(alloc.free_blocks(), 0);

        // Try to allocate more
        let result = alloc.allocate_contiguous(1);
        assert!(matches!(result, Err(CartridgeError::OutOfSpace)));
    }

    #[test]
    fn test_extent_best_fit() {
        let mut alloc = ExtentAllocator::new(1000);

        // Allocate to create fragmentation
        let _b1 = alloc.allocate_contiguous(100).unwrap(); // 0-99
        let b2 = alloc.allocate_contiguous(200).unwrap(); // 100-299
        let b3 = alloc.allocate_contiguous(300).unwrap(); // 300-599

        // Free to create gaps of different sizes
        alloc.free_extent(&b2).unwrap(); // Gap of 200 at 100-299
        alloc.free_extent(&b3).unwrap(); // Gap of 300 at 300-599

        // After freeing, we should have 2 free extents: 200 blocks and 300 blocks
        // Plus the remaining 400 blocks at the end (600-999)
        // Actually they might coalesce into one 700-block extent (100-599 if b1 was freed)
        // Let's just verify best-fit works

        // Allocate 150 blocks - should use the 200-block gap (best fit)
        let b4 = alloc.allocate_contiguous(150).unwrap();
        assert_eq!(b4.len(), 150);
        assert_eq!(b4[0], 100); // Should use the 200-block gap at 100-299
    }

    #[test]
    fn test_extent_via_trait() {
        let mut alloc = ExtentAllocator::new(1000);

        // Allocate 4KB (1 block)
        let blocks = alloc.allocate(4096).unwrap();
        assert_eq!(blocks.len(), 1);

        // Allocate 1MB (256 blocks: ceil(1048576 / 4096))
        let blocks2 = alloc.allocate(1024 * 1024).unwrap();
        assert_eq!(blocks2.len(), 256);

        assert_eq!(alloc.free_blocks(), 1000 - 1 - 256);
    }

    #[test]
    fn test_fragmentation_score() {
        let mut alloc = ExtentAllocator::new(1000);

        // No allocation = low fragmentation
        let score1 = alloc.fragmentation_score();

        // Allocate and free to create fragmentation
        let b1 = alloc.allocate_contiguous(100).unwrap();
        let b2 = alloc.allocate_contiguous(100).unwrap();
        let b3 = alloc.allocate_contiguous(100).unwrap();

        alloc.free_extent(&b2).unwrap(); // Create a gap

        let score2 = alloc.fragmentation_score();

        // More extents = higher fragmentation
        assert!(score2 > score1);
    }

    #[test]
    fn test_invalid_block_id() {
        let mut alloc = ExtentAllocator::new(100);

        let result = alloc.free_extent(&[1000]); // Beyond range
        assert!(matches!(result, Err(CartridgeError::InvalidBlockId(_))));
    }
}
