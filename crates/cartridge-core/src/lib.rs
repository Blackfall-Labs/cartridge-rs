//! Cartridge Archive Format
//!
//! A high-performance, mutable archive format optimized for embedded systems.
//!
//! ## Features
//!
//! - **Fixed 4KB pages** for optimal filesystem alignment
//! - **Hybrid allocation**: Bitmap for small files (<256KB), extent-based for large files
//! - **B-tree catalog** for efficient path lookups
//! - **SHA-256 checksums** for data integrity (optional)
//! - **SQLite VFS** for embedded database support
//! - **IAM policies** for fine-grained access control
//! - **Audit logging** with <1% overhead
//!
//! ## Phase 1: Core Storage Layer (Complete)
//!
//! This phase provides the foundational data structures:
//!
//! - [`error`] - Error types for cartridge operations
//! - [`header`] - Binary format header (Page 0) with magic number and metadata
//! - [`page`] - Page types and management (4KB storage units)
//! - [`allocator`] - Block allocation strategies:
//!   - [`allocator::bitmap`] - Bitmap allocator for small files
//!   - [`allocator::extent`] - Extent allocator for large files
//!   - [`allocator::hybrid`] - Hybrid dispatcher (recommended)
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use cartridge::allocator::{BlockAllocator, hybrid::HybridAllocator};
//! use cartridge::header::Header;
//! use cartridge::page::{Page, PageType};
//!
//! // Create a new cartridge header
//! let mut header = Header::new();
//! header.total_blocks = 10000;
//! header.free_blocks = 10000;
//!
//! // Validate header
//! header.validate().unwrap();
//!
//! // Create a hybrid allocator
//! let mut allocator = HybridAllocator::new(10000);
//!
//! // Allocate space for a small file (uses bitmap allocator)
//! let small_blocks = allocator.allocate(10 * 1024).unwrap();
//! println!("Allocated {} blocks for small file", small_blocks.len());
//!
//! // Allocate space for a large file (uses extent allocator)
//! let large_blocks = allocator.allocate(1024 * 1024).unwrap();
//! println!("Allocated {} contiguous blocks for large file", large_blocks.len());
//!
//! // Create a content data page
//! let mut page = Page::new(PageType::ContentData);
//! page.data[0..5].copy_from_slice(b"Hello");
//! page.compute_checksum();
//!
//! // Verify checksum
//! assert!(page.verify_checksum());
//!
//! // Free allocated blocks
//! allocator.free(&small_blocks).unwrap();
//! allocator.free(&large_blocks).unwrap();
//! ```
//!
//! ## Performance Targets
//!
//! - **100K blocks** allocated in <100ms
//! - **Sub-10μs** cached reads
//! - **<50μs** writes with audit logging
//! - **<1%** audit overhead
//! - **<2%** allocation overhead (bitmap)
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │          Cartridge Archive File             │
//! ├─────────────────────────────────────────────┤
//! │ Page 0: Header (4KB)                        │
//! │  - Magic: "CART\x00\x01\x00\x00"            │
//! │  - Version: 1.0                             │
//! │  - Block count, B-tree root pointer         │
//! ├─────────────────────────────────────────────┤
//! │ Page 1+: B-tree Catalog Nodes               │
//! │  - Path → block mapping                     │
//! │  - Metadata (size, timestamps, permissions) │
//! ├─────────────────────────────────────────────┤
//! │ Pages N+: Content Data                      │
//! │  - File contents (4KB pages)                │
//! │  - SHA-256 checksums (optional)             │
//! ├─────────────────────────────────────────────┤
//! │ Pages M+: Freelist & Audit Log              │
//! │  - Free block tracking                      │
//! │  - Tamper-evident audit trail               │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! ## Integration with Engram
//!
//! Cartridges are **mutable workspaces** that can be frozen and exported to
//! immutable **Engram archives** for distribution:
//!
//! ```text
//! Cartridge (mutable)  →  freeze()  →  Engram (immutable)
//!                      →  vacuum()  →  Ed25519 signed
//!                                   →  LZ4/Zstd compressed
//! ```
//!
//! See [`PLAN_CARTRIDGE_IMPLEMENTATION.md`](../../PLAN_CARTRIDGE_IMPLEMENTATION.md)
//! for the complete implementation roadmap.

pub mod allocator;
pub mod audit;
pub mod buffer_pool;
pub mod cartridge;
pub mod catalog;
pub mod compression;
pub mod encryption;
pub mod engram_integration;
pub mod error;
pub mod header;
pub mod iam;
pub mod integration_tests;
pub mod io;
pub mod page;
pub mod snapshot;
pub mod vfs;

// Re-export commonly used types
pub use allocator::{
    bitmap::BitmapAllocator, extent::ExtentAllocator, hybrid::HybridAllocator, BlockAllocator,
};
pub use cartridge::{Cartridge, CartridgeStats};
pub use catalog::{Catalog, FileMetadata, FileType};
pub use engram_integration::EngramFreezer;
pub use error::{CartridgeError, Result};
pub use header::{Header, PAGE_SIZE};
pub use iam::{
    Action, Condition, ConditionOperator, ConditionValue, Effect, Policy, PolicyCache,
    PolicyEngine, Statement,
};
pub use io::CartridgeFile;
pub use page::{Page, PageHeader, PageType};
pub use snapshot::{SnapshotManager, SnapshotMetadata};

/// Cartridge format version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Cartridge format magic number
pub const MAGIC: &[u8; 8] = &header::MAGIC;
