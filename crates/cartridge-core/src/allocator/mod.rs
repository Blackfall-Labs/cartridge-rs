//! Block allocation strategies for the Cartridge format
//!
//! The allocator uses a hybrid approach:
//! - Bitmap allocation for small files (<256KB)
//! - Extent-based allocation for large files (≥256KB)

pub mod bitmap;
pub mod extent;
pub mod hybrid;

use crate::error::Result;

/// Block allocator trait
///
/// Defines the interface for allocating and freeing blocks in the archive.
pub trait BlockAllocator {
    /// Allocate blocks for a given size in bytes
    ///
    /// Returns a vector of block IDs that have been allocated.
    fn allocate(&mut self, size: u64) -> Result<Vec<u64>>;

    /// Free previously allocated blocks
    fn free(&mut self, blocks: &[u64]) -> Result<()>;

    /// Calculate fragmentation score (0.0 = no fragmentation, higher = more fragmented)
    fn fragmentation_score(&self) -> f64;

    /// Get total number of blocks managed
    fn total_blocks(&self) -> usize;

    /// Get number of free blocks available
    fn free_blocks(&self) -> usize;
}
