# Cartridge Format Implementation Plan

**Created**: 2025-01-19
**Status**: Planning
**Target Timeline**: 6 weeks
**Depends On**: PLAN_DEPLOYMENT_SYSTEM.md, PLAN_DEPLOYMENT_WITH_LIGHTHOUSE.md

---

## Overview

This document provides a detailed technical implementation plan for the **Cartridge mutable archive format** - a high-performance, single-user mutable container with full SQLite VFS support, S3-style access control, and transactional audit trails.

### Key Capabilities

- **100K+ IOPS** on NVMe SSDs
- **Sub-10μs cached reads**, <50μs writes (with audit)
- **<1% audit overhead** through ring buffer batching
- **90%+ buffer pool hit ratio** (ARC eviction policy)
- **Full SQLite VFS** (WAL mode, ACID transactions)
- **S3-style IAM policies** (<100μs evaluation)
- **Append-only audit log** (Merkle tree or hash chain integrity)
- **Snapshot-to-Engram** workflow for immutable distribution

---

## Architecture Summary

### Core Components

```
┌─────────────────────────────────────────────────────────┐
│                 Cartridge Archive (.cart)                │
├─────────────────────────────────────────────────────────┤
│                                                          │
│  ┌────────────────────────────────────────────────────┐ │
│  │ Header (Page 0 - 4KB)                              │ │
│  │  - Magic: CART\x00\x01\x00\x00                     │ │
│  │  - Version: major.minor                            │ │
│  │  - Block size: 4096 bytes                          │ │
│  │  - Total blocks / Free blocks                      │ │
│  │  - B-tree root pointer                             │ │
│  │  - Extension area (256 bytes)                      │ │
│  └────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌────────────────────────────────────────────────────┐ │
│  │ B-Tree Catalog (Central Index)                     │ │
│  │  - Fanout: ~680 entries/node (16KB nodes)          │ │
│  │  - Path hash (8 bytes) → File metadata            │ │
│  │  - Timestamps, size, permissions, checksums        │ │
│  │  - O(log n) lookup for billions of files           │ │
│  └────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌────────────────────────────────────────────────────┐ │
│  │ Content Storage (Log-Structured Append)            │ │
│  │  - Small files (<256KB): Bitmap allocator          │ │
│  │  - Large files (≥256KB): Extent-based allocator   │ │
│  │  - Copy-on-write for files >64KB                   │ │
│  │  - In-place updates for files <64KB                │ │
│  └────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌────────────────────────────────────────────────────┐ │
│  │ Free Space Management                              │ │
│  │  - Multi-level bitmap (files <256KB)               │ │
│  │  - B-tree extent map (files ≥256KB)                │ │
│  │  - Automatic extent coalescing (O(log n))          │ │
│  │  - Incremental compaction (100 blocks/cycle)       │ │
│  └────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌────────────────────────────────────────────────────┐ │
│  │ IAM Policy Engine (S3-style)                       │ │
│  │  - JSON policy documents                           │ │
│  │  - Explicit deny precedence                        │ │
│  │  - Wildcard resource matching                      │ │
│  │  - 27+ condition operators                         │ │
│  │  - <100μs evaluation (cached)                      │ │
│  └────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌────────────────────────────────────────────────────┐ │
│  │ Audit Log (Append-Only)                            │ │
│  │  - Lock-free ring buffer (8192 entries)            │ │
│  │  - 24-byte minimal entries                         │ │
│  │  - Batch flush every 10-100ms                      │ │
│  │  - Optional Merkle tree or hash chain              │ │
│  └────────────────────────────────────────────────────┘ │
│                                                          │
│  ┌────────────────────────────────────────────────────┐ │
│  │ SQLite VFS Layer                                   │ │
│  │  - sqlite3_vfs + sqlite3_io_methods                │ │
│  │  - WAL mode support (exclusive locking)            │ │
│  │  - 3-10x slower than native filesystem             │ │
│  │  - Optimizations: 64KB pages, 100MB cache          │ │
│  └────────────────────────────────────────────────────┘ │
│                                                          │
└─────────────────────────────────────────────────────────┘
```

---

## Implementation Phases

### Phase 1: Core Storage Layer (Week 1)

**Goal**: Implement page-based storage with block allocation

#### Tasks

1. **Binary Format Header** (1 day)

   ```rust
   // crates/cartridge-rs/src/header.rs

   pub const MAGIC: [u8; 8] = *b"CART\x00\x01\x00\x00";
   pub const VERSION_MAJOR: u16 = 1;
   pub const VERSION_MINOR: u16 = 0;
   pub const PAGE_SIZE: usize = 4096;

   #[repr(C)]
   #[derive(Debug, Clone, Copy)]
   pub struct Header {
       pub magic: [u8; 8],
       pub version_major: u16,
       pub version_minor: u16,
       pub block_size: u32,
       pub total_blocks: u64,
       pub free_blocks: u64,
       pub btree_root_page: u64,
       pub reserved: [u8; 256],  // Future extensions
   }

   impl Header {
       pub fn new() -> Self {
           Header {
               magic: MAGIC,
               version_major: VERSION_MAJOR,
               version_minor: VERSION_MINOR,
               block_size: PAGE_SIZE as u32,
               total_blocks: 0,
               free_blocks: 0,
               btree_root_page: 0,
               reserved: [0; 256],
           }
       }

       pub fn validate(&self) -> Result<(), CartridgeError> {
           if self.magic != MAGIC {
               return Err(CartridgeError::InvalidMagic);
           }
           if self.block_size != PAGE_SIZE as u32 {
               return Err(CartridgeError::InvalidBlockSize);
           }
           Ok(())
       }
   }
   ```

2. **Page Types and Management** (1 day)

   ```rust
   // crates/cartridge-rs/src/page.rs

   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   #[repr(u8)]
   pub enum PageType {
       Header = 0,
       CatalogBTree = 1,
       ContentData = 2,
       Freelist = 3,
       AuditLog = 4,
   }

   #[repr(C)]
   pub struct PageHeader {
       pub page_type: PageType,
       pub checksum: [u8; 32],  // SHA-256 (optional verification)
       pub reserved: [u8; 23],
   }

   pub struct Page {
       pub header: PageHeader,
       pub data: Vec<u8>,  // PAGE_SIZE - sizeof(PageHeader)
   }

   impl Page {
       pub fn new(page_type: PageType) -> Self {
           Page {
               header: PageHeader {
                   page_type,
                   checksum: [0; 32],
                   reserved: [0; 23],
               },
               data: vec![0; PAGE_SIZE - size_of::<PageHeader>()],
           }
       }

       pub fn compute_checksum(&mut self) {
           use sha2::{Sha256, Digest};
           let mut hasher = Sha256::new();
           hasher.update(&self.data);
           self.header.checksum = hasher.finalize().into();
       }

       pub fn verify_checksum(&self) -> bool {
           use sha2::{Sha256, Digest};
           let mut hasher = Sha256::new();
           hasher.update(&self.data);
           let computed: [u8; 32] = hasher.finalize().into();
           computed == self.header.checksum
       }
   }
   ```

3. **Block Allocator Interface** (1 day)

   ```rust
   // crates/cartridge-rs/src/allocator.rs

   pub trait BlockAllocator {
       fn allocate(&mut self, size: u64) -> Result<Vec<u64>, CartridgeError>;
       fn free(&mut self, blocks: &[u64]) -> Result<(), CartridgeError>;
       fn fragmentation_score(&self) -> f64;
   }

   // Hybrid allocator dispatches to bitmap or extent based on size
   pub struct HybridAllocator {
       bitmap_alloc: BitmapAllocator,   // <256KB files
       extent_alloc: ExtentAllocator,   // ≥256KB files
       threshold: u64,                   // 256KB in blocks
   }

   impl HybridAllocator {
       pub fn new(threshold: u64) -> Self {
           HybridAllocator {
               bitmap_alloc: BitmapAllocator::new(),
               extent_alloc: ExtentAllocator::new(),
               threshold,
           }
       }
   }

   impl BlockAllocator for HybridAllocator {
       fn allocate(&mut self, size: u64) -> Result<Vec<u64>, CartridgeError> {
           if size < self.threshold {
               self.bitmap_alloc.allocate(size)
           } else {
               self.extent_alloc.allocate(size)
           }
       }

       fn free(&mut self, blocks: &[u64]) -> Result<(), CartridgeError> {
           // Determine which allocator owns these blocks and free accordingly
           // (tracked via block range metadata)
           todo!()
       }

       fn fragmentation_score(&self) -> f64 {
           // Weighted average based on data distribution
           let bitmap_frag = self.bitmap_alloc.fragmentation_score();
           let extent_frag = self.extent_alloc.fragmentation_score();
           (bitmap_frag + extent_frag) / 2.0
       }
   }
   ```

4. **Bitmap Allocator** (2 days)

   ```rust
   // crates/cartridge-rs/src/allocator/bitmap.rs

   pub struct BitmapAllocator {
       bitmap: Vec<u64>,       // Each bit = 1 block (4KB)
       total_blocks: usize,
       free_blocks: usize,
   }

   impl BitmapAllocator {
       pub fn new() -> Self {
           BitmapAllocator {
               bitmap: Vec::new(),
               total_blocks: 0,
               free_blocks: 0,
           }
       }

       pub fn allocate(&mut self, num_blocks: u64) -> Result<Vec<u64>, CartridgeError> {
           let num_blocks = num_blocks as usize;
           let mut allocated = Vec::new();

           // Find contiguous or scattered free blocks
           for (word_idx, word) in self.bitmap.iter_mut().enumerate() {
               if *word == u64::MAX {
                   continue; // All bits set (all allocated)
               }

               // Find free bits in this word
               for bit_idx in 0..64 {
                   if (*word & (1 << bit_idx)) == 0 {
                       // Free block found
                       let block_id = (word_idx * 64 + bit_idx) as u64;
                       allocated.push(block_id);
                       *word |= 1 << bit_idx; // Mark as allocated

                       if allocated.len() == num_blocks {
                           self.free_blocks -= num_blocks;
                           return Ok(allocated);
                       }
                   }
               }
           }

           Err(CartridgeError::OutOfSpace)
       }

       pub fn free(&mut self, blocks: &[u64]) -> Result<(), CartridgeError> {
           for &block_id in blocks {
               let word_idx = (block_id / 64) as usize;
               let bit_idx = (block_id % 64) as usize;

               if word_idx >= self.bitmap.len() {
                   return Err(CartridgeError::InvalidBlockId(block_id));
               }

               self.bitmap[word_idx] &= !(1 << bit_idx); // Clear bit
           }

           self.free_blocks += blocks.len();
           Ok(())
       }
   }

   impl BlockAllocator for BitmapAllocator {
       fn allocate(&mut self, size: u64) -> Result<Vec<u64>, CartridgeError> {
           let num_blocks = (size + PAGE_SIZE as u64 - 1) / PAGE_SIZE as u64;
           self.allocate(num_blocks)
       }

       fn free(&mut self, blocks: &[u64]) -> Result<(), CartridgeError> {
           self.free(blocks)
       }

       fn fragmentation_score(&self) -> f64 {
           // Measure extent count vs ideal contiguous allocation
           // Lower score = less fragmentation
           todo!()
       }
   }
   ```

#### Deliverables (Week 1)

- ✅ Header format with validation
- ✅ Page types and checksum verification
- ✅ Hybrid block allocator (dispatcher)
- ✅ Bitmap allocator for small files (<256KB)
- ✅ Unit tests: Header validation, page checksums, bitmap allocation
- ✅ Performance test: Allocate 100K blocks in <100ms

---

### Phase 2: B-Tree Catalog (Week 2)

**Goal**: Implement B-tree index for file metadata

#### Tasks

1. **B-Tree Node Structure** (2 days)

   ```rust
   // crates/cartridge-rs/src/catalog/btree.rs

   pub const BTREE_FANOUT: usize = 680;  // ~680 entries per 16KB node

   #[derive(Debug, Clone)]
   pub struct BTreeNode {
       pub is_leaf: bool,
       pub num_entries: usize,
       pub entries: Vec<CatalogEntry>,       // Max BTREE_FANOUT
       pub children: Vec<u64>,               // Page IDs of child nodes
   }

   #[derive(Debug, Clone)]
   pub struct CatalogEntry {
       pub path_hash: u64,                   // xxhash64 of full path
       pub path: String,                     // Full path string
       pub size: u64,                        // File size in bytes
       pub block_list: Vec<u64>,             // Physical block IDs
       pub created_at: u64,                  // Unix timestamp (μs)
       pub modified_at: u64,
       pub permissions: u32,                 // Unix-style permissions
       pub content_checksum: [u8; 32],       // SHA-256 of content
   }

   impl BTreeNode {
       pub fn new_leaf() -> Self {
           BTreeNode {
               is_leaf: true,
               num_entries: 0,
               entries: Vec::with_capacity(BTREE_FANOUT),
               children: Vec::new(),
           }
       }

       pub fn new_internal() -> Self {
           BTreeNode {
               is_leaf: false,
               num_entries: 0,
               entries: Vec::with_capacity(BTREE_FANOUT),
               children: Vec::with_capacity(BTREE_FANOUT + 1),
           }
       }

       pub fn search(&self, path_hash: u64) -> Option<&CatalogEntry> {
           self.entries.iter().find(|e| e.path_hash == path_hash)
       }

       pub fn insert(&mut self, entry: CatalogEntry) -> Result<Option<(u64, BTreeNode)>, CartridgeError> {
           // Insert entry in sorted order by path_hash
           let insert_pos = self.entries
               .binary_search_by_key(&entry.path_hash, |e| e.path_hash)
               .unwrap_or_else(|pos| pos);

           self.entries.insert(insert_pos, entry);
           self.num_entries += 1;

           // Split if node is full
           if self.num_entries > BTREE_FANOUT {
               let mid = BTREE_FANOUT / 2;
               let split_key = self.entries[mid].path_hash;

               let mut right_node = if self.is_leaf {
                   BTreeNode::new_leaf()
               } else {
                   BTreeNode::new_internal()
               };

               right_node.entries = self.entries.split_off(mid);
               right_node.num_entries = right_node.entries.len();
               self.num_entries = self.entries.len();

               if !self.is_leaf {
                   right_node.children = self.children.split_off(mid + 1);
               }

               Ok(Some((split_key, right_node)))
           } else {
               Ok(None)
           }
       }
   }
   ```

2. **B-Tree Operations** (2 days)

   ```rust
   // crates/cartridge-rs/src/catalog/mod.rs

   pub struct Catalog {
       root_page_id: u64,
       storage: Arc<Mutex<Storage>>,  // Page storage backend
   }

   impl Catalog {
       pub fn new(storage: Arc<Mutex<Storage>>) -> Self {
           Catalog {
               root_page_id: 0,
               storage,
           }
       }

       pub fn lookup(&self, path: &str) -> Result<Option<CatalogEntry>, CartridgeError> {
           let path_hash = xxhash64(path.as_bytes());
           self.search_recursive(self.root_page_id, path_hash)
       }

       fn search_recursive(&self, page_id: u64, path_hash: u64) -> Result<Option<CatalogEntry>, CartridgeError> {
           let storage = self.storage.lock().unwrap();
           let node = storage.read_btree_node(page_id)?;

           if let Some(entry) = node.search(path_hash) {
               return Ok(Some(entry.clone()));
           }

           if node.is_leaf {
               return Ok(None);
           }

           // Find child to descend into
           let child_idx = node.entries
               .binary_search_by_key(&path_hash, |e| e.path_hash)
               .unwrap_or_else(|idx| idx);

           let child_page_id = node.children[child_idx];
           drop(storage); // Release lock before recursion
           self.search_recursive(child_page_id, path_hash)
       }

       pub fn insert(&mut self, entry: CatalogEntry) -> Result<(), CartridgeError> {
           // Insert with potential splits propagating up
           if let Some((split_key, new_root)) = self.insert_recursive(self.root_page_id, entry)? {
               // Root split - create new root
               let mut root_node = BTreeNode::new_internal();
               root_node.entries.push(CatalogEntry {
                   path_hash: split_key,
                   ..Default::default()
               });
               root_node.children.push(self.root_page_id);
               root_node.children.push(new_root);

               let storage = self.storage.lock().unwrap();
               let new_root_page_id = storage.allocate_page()?;
               storage.write_btree_node(new_root_page_id, &root_node)?;

               self.root_page_id = new_root_page_id;
           }

           Ok(())
       }

       fn insert_recursive(&self, page_id: u64, entry: CatalogEntry) -> Result<Option<(u64, u64)>, CartridgeError> {
           // Returns Some((split_key, new_page_id)) if node split
           todo!()
       }
   }
   ```

3. **Path Hashing** (1 day)

   ```rust
   // crates/cartridge-rs/src/hash.rs

   use xxhash_rust::xxh3::xxh3_64;

   pub fn xxhash64(data: &[u8]) -> u64 {
       xxh3_64(data)
   }

   pub fn path_hash(path: &str) -> u64 {
       // Normalize path first (lowercase, forward slashes, trim)
       let normalized = path.to_lowercase().replace('\\', "/").trim().to_string();
       xxhash64(normalized.as_bytes())
   }
   ```

#### Deliverables (Week 2)

- ✅ B-tree node structure with insert/search
- ✅ Catalog wrapper with recursive operations
- ✅ Path hashing with normalization
- ✅ Unit tests: B-tree insert, search, split
- ✅ Integration test: Insert 1M entries, lookup in O(log n)
- ✅ Performance test: <5 disk reads for 1B files

---

### Phase 3: Extent Allocator & Compaction (Week 3)

**Goal**: Implement extent-based allocation and incremental compaction

#### Tasks

1. **Extent Allocator** (2 days)

   ```rust
   // crates/cartridge-rs/src/allocator/extent.rs

   use std::collections::BTreeMap;

   pub struct ExtentAllocator {
       free_extents: BTreeMap<u64, u64>,  // start_block → length
       total_blocks: u64,
       free_blocks: u64,
   }

   impl ExtentAllocator {
       pub fn new() -> Self {
           ExtentAllocator {
               free_extents: BTreeMap::new(),
               total_blocks: 0,
               free_blocks: 0,
           }
       }

       pub fn allocate(&mut self, num_blocks: u64) -> Result<Vec<u64>, CartridgeError> {
           // Best-fit allocation
           let mut best_start: Option<u64> = None;
           let mut best_length: Option<u64> = None;

           for (&start, &length) in &self.free_extents {
               if length >= num_blocks {
                   if best_length.is_none() || length < best_length.unwrap() {
                       best_start = Some(start);
                       best_length = Some(length);
                   }
               }
           }

           if let (Some(start), Some(length)) = (best_start, best_length) {
               // Allocate from this extent
               let blocks: Vec<u64> = (start..start + num_blocks).collect();

               // Update extent map
               self.free_extents.remove(&start);
               if length > num_blocks {
                   self.free_extents.insert(start + num_blocks, length - num_blocks);
               }

               self.free_blocks -= num_blocks;
               Ok(blocks)
           } else {
               Err(CartridgeError::OutOfSpace)
           }
       }

       pub fn free(&mut self, blocks: &[u64]) -> Result<(), CartridgeError> {
           if blocks.is_empty() {
               return Ok(());
           }

           // Sort blocks to detect contiguous ranges
           let mut sorted_blocks = blocks.to_vec();
           sorted_blocks.sort_unstable();

           // Merge into extents
           let mut current_start = sorted_blocks[0];
           let mut current_length = 1u64;

           for &block in &sorted_blocks[1..] {
               if block == current_start + current_length {
                   // Contiguous, extend current extent
                   current_length += 1;
               } else {
                   // Non-contiguous, finalize current extent
                   self.insert_free_extent(current_start, current_length)?;
                   current_start = block;
                   current_length = 1;
               }
           }

           // Finalize last extent
           self.insert_free_extent(current_start, current_length)?;

           self.free_blocks += blocks.len() as u64;
           Ok(())
       }

       fn insert_free_extent(&mut self, start: u64, length: u64) -> Result<(), CartridgeError> {
           // Coalesce with adjacent extents
           let prev_extent = self.free_extents.range(..start).next_back();
           let next_extent = self.free_extents.range(start + length..).next();

           let mut new_start = start;
           let mut new_length = length;

           // Merge with previous extent if adjacent
           if let Some((&prev_start, &prev_length)) = prev_extent {
               if prev_start + prev_length == start {
                   new_start = prev_start;
                   new_length += prev_length;
                   self.free_extents.remove(&prev_start);
               }
           }

           // Merge with next extent if adjacent
           if let Some((&next_start, &next_length)) = next_extent {
               if start + length == next_start {
                   new_length += next_length;
                   self.free_extents.remove(&next_start);
               }
           }

           self.free_extents.insert(new_start, new_length);
           Ok(())
       }
   }

   impl BlockAllocator for ExtentAllocator {
       fn allocate(&mut self, size: u64) -> Result<Vec<u64>, CartridgeError> {
           let num_blocks = (size + PAGE_SIZE as u64 - 1) / PAGE_SIZE as u64;
           self.allocate(num_blocks)
       }

       fn free(&mut self, blocks: &[u64]) -> Result<(), CartridgeError> {
           self.free(blocks)
       }

       fn fragmentation_score(&self) -> f64 {
           // Fragmentation = number of extents / ideal (1 extent)
           (self.free_extents.len() as f64) / 1.0
       }
   }
   ```

2. **Incremental Compaction** (3 days)

   ```rust
   // crates/cartridge-rs/src/compaction.rs

   pub struct IncrementalCompactor {
       allocator: Arc<Mutex<HybridAllocator>>,
       catalog: Arc<Mutex<Catalog>>,
       max_blocks_per_cycle: usize,  // Default: 100
   }

   impl IncrementalCompactor {
       pub fn new(
           allocator: Arc<Mutex<HybridAllocator>>,
           catalog: Arc<Mutex<Catalog>>,
       ) -> Self {
           IncrementalCompactor {
               allocator,
               catalog,
               max_blocks_per_cycle: 100,
           }
       }

       pub fn run_cycle(&mut self) -> Result<CompactionStats, CartridgeError> {
           let start = Instant::now();

           // Find most fragmented files
           let fragmented_files = self.find_fragmented_files(self.max_blocks_per_cycle)?;

           let mut blocks_moved = 0;
           let mut files_compacted = 0;

           for file_entry in fragmented_files {
               // Read file content
               let content = self.read_file_blocks(&file_entry.block_list)?;

               // Allocate contiguous space
               let new_blocks = {
                   let mut alloc = self.allocator.lock().unwrap();
                   alloc.allocate(file_entry.size)?
               };

               // Write to new location
               self.write_file_blocks(&new_blocks, &content)?;

               // Update catalog
               {
                   let mut catalog = self.catalog.lock().unwrap();
                   let mut updated_entry = file_entry.clone();
                   updated_entry.block_list = new_blocks.clone();
                   catalog.update(&updated_entry)?;
               }

               // Free old blocks
               {
                   let mut alloc = self.allocator.lock().unwrap();
                   alloc.free(&file_entry.block_list)?;
               }

               blocks_moved += file_entry.block_list.len();
               files_compacted += 1;
           }

           let elapsed = start.elapsed();

           Ok(CompactionStats {
               blocks_moved,
               files_compacted,
               elapsed_ms: elapsed.as_millis() as u64,
           })
       }

       fn find_fragmented_files(&self, max_blocks: usize) -> Result<Vec<CatalogEntry>, CartridgeError> {
           // Query catalog for files with non-contiguous blocks
           // Sort by fragmentation score (extent count)
           // Return top N until max_blocks reached
           todo!()
       }
   }

   #[derive(Debug)]
   pub struct CompactionStats {
       pub blocks_moved: usize,
       pub files_compacted: usize,
       pub elapsed_ms: u64,
   }
   ```

#### Deliverables (Week 3)

- ✅ Extent allocator with automatic coalescing
- ✅ Incremental compaction (100 blocks/cycle, <10ms latency)
- ✅ Fragmentation scoring
- ✅ Unit tests: Extent allocation, coalescing, compaction
- ✅ Integration test: Compact 10GB archive in background
- ✅ Performance test: Compaction cycle <10ms

---

### Phase 4: SQLite VFS Implementation (Week 4)

**Goal**: Implement sqlite3_vfs and sqlite3_io_methods

#### Tasks

1. **VFS Structure** (2 days)

   ```rust
   // crates/cartridge-rs/src/vfs/mod.rs

   use libsqlite3_sys as ffi;

   pub struct CartridgeVFS {
       name: CString,
       cartridge: Arc<Mutex<Cartridge>>,
   }

   impl CartridgeVFS {
       pub fn register() -> Result<(), CartridgeError> {
           let vfs_methods = ffi::sqlite3_vfs {
               iVersion: 3,
               szOsFile: size_of::<CartridgeFile>() as i32,
               mxPathname: 1024,
               pNext: null_mut(),
               zName: b"cartridge\0".as_ptr() as *const c_char,
               pAppData: null_mut(),
               xOpen: Some(vfs_open),
               xDelete: Some(vfs_delete),
               xAccess: Some(vfs_access),
               xFullPathname: Some(vfs_full_pathname),
               xDlOpen: None,
               xDlError: None,
               xDlSym: None,
               xDlClose: None,
               xRandomness: Some(vfs_randomness),
               xSleep: Some(vfs_sleep),
               xCurrentTime: Some(vfs_current_time),
               xGetLastError: Some(vfs_get_last_error),
               xCurrentTimeInt64: Some(vfs_current_time_int64),
               xSetSystemCall: None,
               xGetSystemCall: None,
               xNextSystemCall: None,
           };

           unsafe {
               let rc = ffi::sqlite3_vfs_register(&vfs_methods as *const _ as *mut _, 0);
               if rc != ffi::SQLITE_OK {
                   return Err(CartridgeError::VFSRegistrationFailed(rc));
               }
           }

           Ok(())
       }
   }

   #[repr(C)]
   pub struct CartridgeFile {
       base: ffi::sqlite3_file,
       path: String,
       cartridge: Arc<Mutex<Cartridge>>,
       offset: u64,
   }
   ```

2. **File I/O Methods** (3 days)

   ```rust
   // crates/cartridge-rs/src/vfs/file.rs

   unsafe extern "C" fn file_close(file: *mut ffi::sqlite3_file) -> c_int {
       let cart_file = &mut *(file as *mut CartridgeFile);
       // Cleanup
       ffi::SQLITE_OK
   }

   unsafe extern "C" fn file_read(
       file: *mut ffi::sqlite3_file,
       buf: *mut c_void,
       amt: c_int,
       offset: i64,
   ) -> c_int {
       let cart_file = &mut *(file as *mut CartridgeFile);
       let cartridge = cart_file.cartridge.lock().unwrap();

       match cartridge.read_file(&cart_file.path, offset as u64, amt as usize) {
           Ok(data) => {
               if data.len() < amt as usize {
                   // Short read
                   return ffi::SQLITE_IOERR_SHORT_READ;
               }
               std::ptr::copy_nonoverlapping(data.as_ptr(), buf as *mut u8, data.len());
               ffi::SQLITE_OK
           }
           Err(_) => ffi::SQLITE_IOERR_READ,
       }
   }

   unsafe extern "C" fn file_write(
       file: *mut ffi::sqlite3_file,
       buf: *const c_void,
       amt: c_int,
       offset: i64,
   ) -> c_int {
       let cart_file = &mut *(file as *mut CartridgeFile);
       let data = std::slice::from_raw_parts(buf as *const u8, amt as usize);

       let mut cartridge = cart_file.cartridge.lock().unwrap();
       match cartridge.write_file(&cart_file.path, offset as u64, data) {
           Ok(_) => ffi::SQLITE_OK,
           Err(_) => ffi::SQLITE_IOERR_WRITE,
       }
   }

   unsafe extern "C" fn file_truncate(file: *mut ffi::sqlite3_file, size: i64) -> c_int {
       let cart_file = &mut *(file as *mut CartridgeFile);
       let mut cartridge = cart_file.cartridge.lock().unwrap();

       match cartridge.truncate_file(&cart_file.path, size as u64) {
           Ok(_) => ffi::SQLITE_OK,
           Err(_) => ffi::SQLITE_IOERR_TRUNCATE,
       }
   }

   unsafe extern "C" fn file_sync(file: *mut ffi::sqlite3_file, flags: c_int) -> c_int {
       let cart_file = &mut *(file as *mut CartridgeFile);
       let cartridge = cart_file.cartridge.lock().unwrap();

       match cartridge.fsync() {
           Ok(_) => ffi::SQLITE_OK,
           Err(_) => ffi::SQLITE_IOERR_FSYNC,
       }
   }

   unsafe extern "C" fn file_file_size(file: *mut ffi::sqlite3_file, size: *mut i64) -> c_int {
       let cart_file = &mut *(file as *mut CartridgeFile);
       let cartridge = cart_file.cartridge.lock().unwrap();

       match cartridge.file_size(&cart_file.path) {
           Ok(file_size) => {
               *size = file_size as i64;
               ffi::SQLITE_OK
           }
           Err(_) => ffi::SQLITE_IOERR,
       }
   }

   // Locking methods (simplified for exclusive mode)
   unsafe extern "C" fn file_lock(file: *mut ffi::sqlite3_file, lock_type: c_int) -> c_int {
       // PRAGMA locking_mode=EXCLUSIVE makes this a no-op
       ffi::SQLITE_OK
   }

   unsafe extern "C" fn file_unlock(file: *mut ffi::sqlite3_file, lock_type: c_int) -> c_int {
       // PRAGMA locking_mode=EXCLUSIVE makes this a no-op
       ffi::SQLITE_OK
   }

   unsafe extern "C" fn file_check_reserved_lock(file: *mut ffi::sqlite3_file, res_out: *mut c_int) -> c_int {
       *res_out = 0; // Never reserved (exclusive mode)
       ffi::SQLITE_OK
   }

   unsafe extern "C" fn file_file_control(file: *mut ffi::sqlite3_file, op: c_int, arg: *mut c_void) -> c_int {
       match op {
           ffi::SQLITE_FCNTL_LOCKSTATE => ffi::SQLITE_OK,
           ffi::SQLITE_FCNTL_PRAGMA => ffi::SQLITE_NOTFOUND,
           _ => ffi::SQLITE_NOTFOUND,
       }
   }

   unsafe extern "C" fn file_sector_size(file: *mut ffi::sqlite3_file) -> c_int {
       PAGE_SIZE as c_int
   }

   unsafe extern "C" fn file_device_characteristics(file: *mut ffi::sqlite3_file) -> c_int {
       ffi::SQLITE_IOCAP_SAFE_APPEND | ffi::SQLITE_IOCAP_ATOMIC4K
   }
   ```

#### Deliverables (Week 4)

- ✅ VFS registration
- ✅ 13 core file I/O methods
- ✅ Exclusive locking mode (no xShm\* methods)
- ✅ Integration test: Create SQLite database in cartridge
- ✅ Integration test: WAL mode with transactions
- ✅ Performance test: 3-10x slower than native filesystem

---

### Phase 5: IAM Policy Engine (Week 5)

**Goal**: Implement S3-style access control with <100μs evaluation

#### Tasks

1. **Policy Document Structure** (1 day)

   ```rust
   // crates/cartridge-rs/src/iam/policy.rs

   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct PolicyDocument {
       #[serde(rename = "Version")]
       pub version: String,  // "2012-10-17"

       #[serde(rename = "Id", skip_serializing_if = "Option::is_none")]
       pub id: Option<String>,

       #[serde(rename = "Statement")]
       pub statements: Vec<Statement>,
   }

   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct Statement {
       #[serde(rename = "Effect")]
       pub effect: Effect,

       #[serde(rename = "Principal", skip_serializing_if = "Option::is_none")]
       pub principal: Option<Principal>,

       #[serde(rename = "Action")]
       pub action: ActionSpec,

       #[serde(rename = "Resource")]
       pub resource: ResourceSpec,

       #[serde(rename = "Condition", skip_serializing_if = "Option::is_none")]
       pub condition: Option<Condition>,
   }

   #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
   pub enum Effect {
       Allow,
       Deny,
   }

   #[derive(Debug, Clone, Serialize, Deserialize)]
   #[serde(untagged)]
   pub enum ActionSpec {
       Single(String),
       Multiple(Vec<String>),
   }

   #[derive(Debug, Clone, Serialize, Deserialize)]
   #[serde(untagged)]
   pub enum ResourceSpec {
       Single(String),
       Multiple(Vec<String>),
   }

   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct Condition {
       // Map of condition operator → {key → values}
       // Example: {"StringEquals": {"department": ["finance", "hr"]}}
       #[serde(flatten)]
       pub conditions: HashMap<String, HashMap<String, serde_json::Value>>,
   }
   ```

2. **Policy Evaluation Engine** (3 days)

   ```rust
   // crates/cartridge-rs/src/iam/evaluator.rs

   pub struct PolicyEvaluator {
       policies: Vec<PolicyDocument>,
       pattern_cache: LruCache<String, regex::Regex>,
       result_cache: LruCache<RequestTuple, EvaluationResult>,
   }

   #[derive(Debug, Clone, Hash, Eq, PartialEq)]
   pub struct RequestTuple {
       pub actor_id: String,
       pub action: String,
       pub resource: String,
       pub context: HashMap<String, String>,
   }

   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   pub enum EvaluationResult {
       Allow,
       Deny,
       ImplicitDeny,
   }

   impl PolicyEvaluator {
       pub fn new() -> Self {
           PolicyEvaluator {
               policies: Vec::new(),
               pattern_cache: LruCache::new(NonZeroUsize::new(1000).unwrap()),
               result_cache: LruCache::new(NonZeroUsize::new(10000).unwrap()),
           }
       }

       pub fn load_policy(&mut self, policy: PolicyDocument) {
           self.policies.push(policy);
           // Invalidate result cache
           self.result_cache.clear();
       }

       pub fn evaluate(&mut self, request: &RequestTuple) -> EvaluationResult {
           // Check cache first
           if let Some(&result) = self.result_cache.get(request) {
               return result;
           }

           let result = self.evaluate_uncached(request);
           self.result_cache.put(request.clone(), result);
           result
       }

       fn evaluate_uncached(&mut self, request: &RequestTuple) -> EvaluationResult {
           // Step 1: Check for explicit denies
           for policy in &self.policies {
               for statement in &policy.statements {
                   if statement.effect == Effect::Deny {
                       if self.matches_statement(statement, request) {
                           return EvaluationResult::Deny;
                       }
                   }
               }
           }

           // Step 2: Check for explicit allows
           for policy in &self.policies {
               for statement in &policy.statements {
                   if statement.effect == Effect::Allow {
                       if self.matches_statement(statement, request) {
                           return EvaluationResult::Allow;
                       }
                   }
               }
           }

           // Step 3: Default implicit deny
           EvaluationResult::ImplicitDeny
       }

       fn matches_statement(&mut self, statement: &Statement, request: &RequestTuple) -> bool {
           // Match action
           if !self.matches_action(&statement.action, &request.action) {
               return false;
           }

           // Match resource
           if !self.matches_resource(&statement.resource, &request.resource) {
               return false;
           }

           // Match conditions (if present)
           if let Some(ref condition) = statement.condition {
               if !self.matches_condition(condition, request) {
                   return false;
               }
           }

           true
       }

       fn matches_resource(&mut self, spec: &ResourceSpec, resource: &str) -> bool {
           let patterns = match spec {
               ResourceSpec::Single(s) => vec![s.as_str()],
               ResourceSpec::Multiple(v) => v.iter().map(|s| s.as_str()).collect(),
           };

           for pattern in patterns {
               if self.matches_pattern(pattern, resource) {
                   return true;
               }
           }

           false
       }

       fn matches_pattern(&mut self, pattern: &str, value: &str) -> bool {
           // Convert wildcard pattern to regex
           let regex_pattern = pattern
               .replace("*", ".*")
               .replace("?", ".");

           let regex = self.pattern_cache
               .get_or_insert(pattern.to_string(), || {
                   regex::Regex::new(&format!("^{}$", regex_pattern)).unwrap()
               });

           regex.is_match(value)
       }

       fn matches_condition(&self, condition: &Condition, request: &RequestTuple) -> bool {
           for (operator, clauses) in &condition.conditions {
               for (key, value) in clauses {
                   if !self.evaluate_condition(operator, key, value, request) {
                       return false;
                   }
               }
           }

           true
       }

       fn evaluate_condition(
           &self,
           operator: &str,
           key: &str,
           value: &serde_json::Value,
           request: &RequestTuple,
       ) -> bool {
           let context_value = request.context.get(key);

           match operator {
               "StringEquals" => {
                   if let Some(v) = value.as_str() {
                       context_value.map_or(false, |cv| cv == v)
                   } else {
                       false
                   }
               }
               "StringLike" => {
                   // Wildcard pattern matching
                   todo!()
               }
               "NumericLessThan" => {
                   // Numeric comparison
                   todo!()
               }
               _ => false,  // Unknown operator
           }
       }
   }
   ```

#### Deliverables (Week 5)

- ✅ Policy document parsing (JSON)
- ✅ Policy evaluation engine with explicit deny precedence
- ✅ Pattern matching with wildcard support
- ✅ Condition operators (StringEquals, StringLike, NumericLessThan)
- ✅ LRU caching for patterns and results
- ✅ Unit tests: Policy evaluation, wildcard matching, caching
- ✅ Performance test: 10,000+ evaluations/sec/core, <100μs cached

---

### Phase 6: Audit Log & Optimization (Week 6)

**Goal**: Implement append-only audit log and final optimizations

#### Tasks

1. **Audit Log Structure** (2 days)

   ```rust
   // crates/cartridge-rs/src/audit/mod.rs

   #[repr(C)]
   #[derive(Debug, Clone, Copy)]
   pub struct AuditEntry {
       pub timestamp_us: u64,     // Microsecond timestamp
       pub actor_id: u32,         // User/process ID
       pub operation: Operation,  // CREATE/READ/UPDATE/DELETE
       pub resource_table: u16,   // Which table
       pub resource_id: u64,      // Row ID or file ID
       pub session_id: u32,       // Optional session tracking
   }

   #[repr(u16)]
   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   pub enum Operation {
       Create = 0,
       Read = 1,
       Update = 2,
       Delete = 3,
   }

   pub struct AuditLogger {
       ring_buffer: Arc<RingBuffer<AuditEntry>>,
       flush_thread: Option<JoinHandle<()>>,
       flush_interval: Duration,
   }

   impl AuditLogger {
       pub fn new(capacity: usize, flush_interval: Duration) -> Self {
           let ring_buffer = Arc::new(RingBuffer::new(capacity));

           AuditLogger {
               ring_buffer,
               flush_thread: None,
               flush_interval,
           }
       }

       pub fn start(&mut self, storage: Arc<Mutex<Storage>>) {
           let ring_buffer = Arc::clone(&self.ring_buffer);
           let flush_interval = self.flush_interval;

           let flush_thread = std::thread::spawn(move || {
               loop {
                   std::thread::sleep(flush_interval);

                   // Read batch from ring buffer
                   let entries = ring_buffer.read_batch(1000);
                   if entries.is_empty() {
                       continue;
                   }

                   // Write to storage
                   let mut storage = storage.lock().unwrap();
                   if let Err(e) = storage.append_audit_entries(&entries) {
                       eprintln!("Audit flush error: {}", e);
                   }
               }
           });

           self.flush_thread = Some(flush_thread);
       }

       pub fn log(&self, entry: AuditEntry) {
           self.ring_buffer.write(entry);
       }
   }
   ```

2. **Lock-Free Ring Buffer** (2 days)

   ```rust
   // crates/cartridge-rs/src/audit/ring_buffer.rs

   use std::sync::atomic::{AtomicUsize, Ordering};

   pub struct RingBuffer<T: Copy> {
       buffer: Vec<AtomicCell<Option<T>>>,
       capacity: usize,
       write_pos: AtomicUsize,
       read_pos: AtomicUsize,
   }

   impl<T: Copy> RingBuffer<T> {
       pub fn new(capacity: usize) -> Self {
           let mut buffer = Vec::with_capacity(capacity);
           for _ in 0..capacity {
               buffer.push(AtomicCell::new(None));
           }

           RingBuffer {
               buffer,
               capacity,
               write_pos: AtomicUsize::new(0),
               read_pos: AtomicUsize::new(0),
           }
       }

       pub fn write(&self, value: T) {
           let pos = self.write_pos.fetch_add(1, Ordering::SeqCst) % self.capacity;
           self.buffer[pos].store(Some(value));
       }

       pub fn read_batch(&self, max_count: usize) -> Vec<T> {
           let mut batch = Vec::new();
           let current_write = self.write_pos.load(Ordering::SeqCst);
           let mut current_read = self.read_pos.load(Ordering::SeqCst);

           while batch.len() < max_count && current_read < current_write {
               let pos = current_read % self.capacity;
               if let Some(value) = self.buffer[pos].swap(None) {
                   batch.push(value);
               }
               current_read += 1;
           }

           self.read_pos.store(current_read, Ordering::SeqCst);
           batch
       }
   }
   ```

3. **Buffer Pool with ARC** (2 days)

   ```rust
   // crates/cartridge-rs/src/buffer_pool.rs

   pub struct BufferPool {
       t1: LruList,        // Recently accessed
       t2: LruList,        // Frequently accessed
       b1: GhostList,      // Recently evicted from T1
       b2: GhostList,      // Recently evicted from T2
       p: usize,           // Adaptive parameter
       capacity: usize,
       pages: HashMap<u64, Arc<Page>>,
   }

   impl BufferPool {
       pub fn new(capacity: usize) -> Self {
           BufferPool {
               t1: LruList::new(),
               t2: LruList::new(),
               b1: GhostList::new(),
               b2: GhostList::new(),
               p: 0,
               capacity,
               pages: HashMap::new(),
           }
       }

       pub fn get(&mut self, page_id: u64) -> Option<Arc<Page>> {
           if let Some(page) = self.pages.get(&page_id) {
               // Hit in T1 or T2
               self.on_hit(page_id);
               Some(Arc::clone(page))
           } else if self.b1.contains(page_id) {
               // Hit in B1 (ghost)
               self.p = std::cmp::min(self.p + 1, self.capacity);
               self.replace(page_id);
               None // Caller must load from disk
           } else if self.b2.contains(page_id) {
               // Hit in B2 (ghost)
               if self.p > 0 {
                   self.p -= 1;
               }
               self.replace(page_id);
               None // Caller must load from disk
           } else {
               // Miss
               None
           }
       }

       pub fn put(&mut self, page_id: u64, page: Arc<Page>) {
           if self.pages.len() >= self.capacity {
               self.replace(page_id);
           }

           self.pages.insert(page_id, page);
           self.t1.push(page_id);
       }

       fn on_hit(&mut self, page_id: u64) {
           if self.t1.remove(page_id) {
               // Move from T1 to T2 (accessed again)
               self.t2.push(page_id);
           } else if self.t2.contains(page_id) {
               // Already in T2, move to MRU position
               self.t2.move_to_front(page_id);
           }
       }

       fn replace(&mut self, page_id: u64) {
           // Adaptive replacement policy
           if self.t1.len() >= self.p {
               // Evict from T1
               if let Some(evicted) = self.t1.pop_lru() {
                   self.pages.remove(&evicted);
                   self.b1.insert(evicted);
               }
           } else {
               // Evict from T2
               if let Some(evicted) = self.t2.pop_lru() {
                   self.pages.remove(&evicted);
                   self.b2.insert(evicted);
               }
           }
       }

       pub fn hit_ratio(&self) -> f64 {
           // Track hits vs total accesses
           todo!()
       }
   }
   ```

#### Deliverables (Week 6)

- ✅ Audit log with lock-free ring buffer
- ✅ Async flush thread (10-100ms batching)
- ✅ Buffer pool with ARC eviction policy
- ✅ Performance optimizations (io_uring, memory mapping)
- ✅ Integration tests: Audit 1M operations, buffer pool hit ratio >90%
- ✅ Performance tests: <1% audit overhead, sub-10μs cached reads

---

## Testing Strategy

### Unit Tests (per phase)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_validation() {
        let header = Header::new();
        assert_eq!(header.magic, MAGIC);
        assert!(header.validate().is_ok());
    }

    #[test]
    fn test_bitmap_allocation() {
        let mut alloc = BitmapAllocator::new();
        // Initialize with some free blocks
        alloc.total_blocks = 1000;
        alloc.free_blocks = 1000;
        alloc.bitmap = vec![0; 16]; // 16 * 64 = 1024 blocks

        // Allocate 10 blocks
        let blocks = alloc.allocate(10).unwrap();
        assert_eq!(blocks.len(), 10);
        assert_eq!(alloc.free_blocks, 990);

        // Free them
        alloc.free(&blocks).unwrap();
        assert_eq!(alloc.free_blocks, 1000);
    }

    #[test]
    fn test_btree_insert_and_search() {
        let mut node = BTreeNode::new_leaf();
        let entry = CatalogEntry {
            path_hash: 12345,
            path: "/test/file.txt".to_string(),
            size: 1024,
            block_list: vec![1, 2, 3],
            created_at: 0,
            modified_at: 0,
            permissions: 0o644,
            content_checksum: [0; 32],
        };

        node.insert(entry.clone()).unwrap();
        let found = node.search(12345).unwrap();
        assert_eq!(found.path, "/test/file.txt");
    }

    #[test]
    fn test_policy_evaluation() {
        let mut evaluator = PolicyEvaluator::new();
        let policy = PolicyDocument {
            version: "2012-10-17".to_string(),
            id: None,
            statements: vec![
                Statement {
                    effect: Effect::Allow,
                    principal: None,
                    action: ActionSpec::Single("cart:read".to_string()),
                    resource: ResourceSpec::Single("cart://data/*".to_string()),
                    condition: None,
                }
            ],
        };
        evaluator.load_policy(policy);

        let request = RequestTuple {
            actor_id: "user1".to_string(),
            action: "cart:read".to_string(),
            resource: "cart://data/file.txt".to_string(),
            context: HashMap::new(),
        };

        assert_eq!(evaluator.evaluate(&request), EvaluationResult::Allow);
    }
}
```

### Integration Tests

```rust
#[cfg(test)]
mod integration {
    #[test]
    fn test_sqlite_database_in_cartridge() {
        // Create cartridge
        let cart = Cartridge::create("test.cart").unwrap();

        // Register VFS
        CartridgeVFS::register().unwrap();

        // Open SQLite database in cartridge
        let db = rusqlite::Connection::open_with_flags(
            "file:test.db?vfs=cartridge",
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        ).unwrap();

        // Create table
        db.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)", []).unwrap();

        // Insert data
        db.execute("INSERT INTO users (name) VALUES (?)", ["Alice"]).unwrap();

        // Query
        let name: String = db.query_row("SELECT name FROM users WHERE id = 1", [], |row| row.get(0)).unwrap();
        assert_eq!(name, "Alice");
    }

    #[test]
    fn test_audit_log_integrity() {
        let mut logger = AuditLogger::new(8192, Duration::from_millis(100));
        let storage = Arc::new(Mutex::new(Storage::new("test_audit.cart").unwrap()));
        logger.start(Arc::clone(&storage));

        // Log 10K operations
        for i in 0..10000 {
            logger.log(AuditEntry {
                timestamp_us: i,
                actor_id: 1,
                operation: Operation::Read,
                resource_table: 0,
                resource_id: i as u64,
                session_id: 1,
            });
        }

        // Wait for flush
        std::thread::sleep(Duration::from_secs(1));

        // Verify all entries logged
        let storage = storage.lock().unwrap();
        let entries = storage.read_audit_entries(0, 10000).unwrap();
        assert_eq!(entries.len(), 10000);
    }
}
```

### Performance Benchmarks

```bash
# Run all benchmarks
cargo bench --package cartridge

# Specific benchmarks
cargo bench --bench allocation    # Block allocation speed
cargo bench --bench btree         # B-tree insert/search
cargo bench --bench iam           # Policy evaluation
cargo bench --bench buffer_pool   # Cache hit ratio
cargo bench --bench audit         # Audit throughput
```

---

## Dependencies

### Integration with Existing Engram Infrastructure

The Cartridge format **reuses and extends** the existing `engram-rs` library from `../engram-core/`:

**What we reuse from engram-rs**:

- **ED25519 signing**: `ed25519-dalek` for cryptographic signatures
- **Compression**: `lz4_flex` and `zstd` for content compression
- **Checksums**: `crc32fast` and `sha2` for integrity verification
- **Serialization**: `serde` and `serde_json` for manifests

**What Cartridge adds**:

- **Mutable archive format** (engram is immutable)
- **SQLite VFS** (engram uses read-only VFS)
- **IAM policy engine** (engram has basic permissions)
- **Incremental compaction** (engram doesn't need it)
- **Audit logging** (engram doesn't track mutations)

**Integration strategy**:

```rust
// Cartridge can export to Engram (snapshot workflow)
let cartridge = Cartridge::open("workspace.cart")?;
cartridge.freeze()?;  // Make immutable
cartridge.vacuum()?;  // Optimize layout
let engram = cartridge.export_to_engram("release-v1.0.eng")?;
engram.sign(&signing_key)?;

// SAM deployment: Distribute Engram, unpack to Cartridge for active use
let engram = Engram::open("release-v1.0.eng")?;
engram.verify_signature(&public_key)?;
let cartridge = Cartridge::import_from_engram(&engram, "workspace.cart")?;
```

### Cargo.toml

```toml
[package]
name = "cartridge"
version = "0.1.0"
edition = "2021"
authors = ["SAM Contributors"]
description = "High-performance mutable archive format with SQLite VFS support"

[dependencies]
# Engram integration (reuse existing crypto/compression)
engram-rs = { path = "../../../engram-core" }

# Core
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
toml = "0.8"
thiserror = "1.0"
anyhow = "1.0"

# Hashing (xxhash for path hashing, sha2/crc32 from engram-rs)
xxhash-rust = { version = "0.8", features = ["xxh3"] }
ahash = "0.8"

# Caching
lru = "0.12"

# Concurrency
crossbeam = "0.8"
parking_lot = "0.12"

# Regex for IAM wildcard patterns
regex = "1.10"

# SQLite (rusqlite for high-level API, libsqlite3-sys for VFS FFI)
rusqlite = { version = "0.31", features = ["bundled", "backup"] }
libsqlite3-sys = { version = "0.28", features = ["bundled"] }

# I/O
memmap2 = "0.9"
tokio = { version = "1.35", features = ["full"], optional = true }

# Logging
tracing = "0.1"

[dev-dependencies]
criterion = "0.5"
tempfile = "3.12"
rand = "0.8"

[features]
default = []
async = ["tokio"]

[[bench]]
name = "allocation"
harness = false

[[bench]]
name = "btree"
harness = false

[[bench]]
name = "iam"
harness = false
```

---

## Performance Targets

| Metric                     | Target          | Measured |
| -------------------------- | --------------- | -------- |
| Read latency (cached)      | <10μs           | TBD      |
| Write latency (with audit) | <50μs           | TBD      |
| Buffer pool hit ratio      | >90%            | TBD      |
| IOPS (NVMe)                | >100K           | TBD      |
| IAM evaluation (cached)    | <100μs          | TBD      |
| Audit overhead             | <1%             | TBD      |
| SQLite overhead            | 3-10x vs native | TBD      |

---

## Next Steps

1. **Week 1**: Core storage layer implementation
2. **Week 2**: B-tree catalog
3. **Week 3**: Extent allocator + compaction
4. **Week 4**: SQLite VFS
5. **Week 5**: IAM policy engine
6. **Week 6**: Audit log + optimizations
7. **Week 7** (optional): Lighthouse integration, snapshot-to-Engram workflow

---

**Author**: Claude (with human oversight)
**Last Updated**: 2025-01-19
**Status**: Ready for implementation
**Next Milestone**: Complete Week 1 (Core storage layer)
