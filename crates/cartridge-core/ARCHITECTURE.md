# Cartridge Architecture

**Version:** 0.1.0
**Last Updated:** 2025-11-20
**Status:** Production Ready (Phase 7 Complete)

---

## Table of Contents

1. [System Overview](#system-overview)
2. [Component Architecture](#component-architecture)
3. [Data Flow](#data-flow)
4. [Memory Layout](#memory-layout)
5. [Threading Model](#threading-model)
6. [Integration Points](#integration-points)
7. [Design Decisions](#design-decisions)
8. [Performance Characteristics](#performance-characteristics)

---

## System Overview

Cartridge is a high-performance, offline-first virtual filesystem optimized for embedded systems. It provides a mutable archive format within a single file, combining characteristics of both filesystems and databases.

### Design Philosophy

1. **Page-Based I/O:** Fixed 4KB pages align with filesystem and memory page sizes
2. **Hybrid Allocation:** Different strategies for small (<256KB) vs large (≥256KB) files
3. **Adaptive Caching:** ARC (Adaptive Replacement Cache) outperforms LRU
4. **Zero-Copy Where Possible:** Minimize memory copies and allocations
5. **Fail-Safe Design:** SHA-256 checksums, authenticated encryption, explicit error handling

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          Cartridge Public API                           │
│  - create_file(), read_file(), write_file(), delete_file()             │
│  - create_dir(), list_dir(), exists(), metadata()                      │
│  - flush(), close(), stats()                                           │
│  - create_snapshot(), restore_snapshot()                               │
│  - set_policy(), check_access()                                        │
└────────────────────────┬────────────────────────────────────────────────┘
                         │
        ┌────────────────┼────────────────┐
        │                │                │
┌───────▼────────┐  ┌───▼──────┐  ┌─────▼─────┐
│    Catalog     │  │ Allocator │  │ IAM Policy │
│   (B-tree)     │  │ (Hybrid)  │  │  Engine    │
└───────┬────────┘  └───┬──────┘  └─────┬──────┘
        │               │               │
        │   Maps paths  │  Allocates    │  Enforces
        │   to blocks   │  blocks       │  access rules
        │               │               │
        └───────┬───────┴───────┬───────┘
                │               │
          ┌─────▼───────────────▼─────┐
          │         Pager              │
          │  (4KB Page Management)     │
          └─────┬──────────────┬───────┘
                │              │
      ┌─────────▼─────┐  ┌────▼────────┐
      │  Buffer Pool  │  │   I/O Layer │
      │  (ARC Cache)  │  │  (File/Mem) │
      └───────────────┘  └─────────────┘
```

---

## Component Architecture

### 1. Cartridge Core

**File:** `cartridge.rs`
**Responsibilities:**
- High-level file operations (CRUD)
- Coordinate between catalog, allocator, and pager
- Session management and state tracking

**Key Types:**
```rust
pub struct Cartridge {
    header: Header,                    // Archive metadata (page 0)
    allocator: HybridAllocator,        // Block allocation
    catalog: Catalog,                  // Path → metadata mapping
    file: Option<CartridgeFile>,       // Disk backing (optional)
    pages: HashMap<u64, Vec<u8>>,      // In-memory page cache
    dirty_pages: HashSet<u64>,         // Pages needing flush
    audit_logger: Option<Arc<AuditLogger>>,  // Audit trail
    session_id: u32,                   // Current session
    policy: Option<Policy>,            // IAM policy
    policy_engine: Option<PolicyEngine>, // Policy evaluator
}
```

**Operations:**
- `create_file(path, content)` → Allocate blocks, write content, update catalog
- `read_file(path)` → Lookup blocks from catalog, read content
- `write_file(path, content)` → Free old blocks, allocate new, write, update
- `delete_file(path)` → Remove from catalog, free blocks
- `flush()` → Write dirty pages to disk
- `close()` → Flush and cleanup

### 2. Header

**File:** `header.rs`
**Responsibilities:**
- Store archive metadata in page 0
- Version information and format validation
- Pointers to critical structures (B-tree root)

**Structure (Page 0):**
```
Offset  Size  Field                 Description
------  ----  -------------------   ---------------------------
0       8     magic                 "CART\x00\x01\x00\x00"
8       2     version_major         1
10      2     version_minor         0
12      4     block_size            4096 (constant)
16      8     total_blocks          Archive capacity
24      8     free_blocks           Available blocks
32      8     btree_root_page       Catalog B-tree root
40      256   reserved              Future extensions
296     3800  padding               Pad to 4096 bytes
```

**Validation:**
- Magic number check
- Version compatibility
- Block size verification (must be 4096)
- Sanity checks (free_blocks ≤ total_blocks)

### 3. Page System

**File:** `page.rs`
**Responsibilities:**
- Fixed 4KB storage units
- Page type identification
- SHA-256 checksum computation/verification

**Page Structure:**
```
┌────────────────────────────────────────────┐
│  Page Header (64 bytes)                    │
├─────────────────────────────────────────────┤
│  page_type (u8)                            │
│  checksum (32 bytes SHA-256)               │
│  reserved (31 bytes)                       │
├─────────────────────────────────────────────┤
│  Page Data (4032 bytes)                    │
│                                            │
│  [Content varies by page type]             │
│                                            │
└────────────────────────────────────────────┘
Total: 4096 bytes (PAGE_SIZE)
```

**Page Types:**
- `Header (0)` - Archive header (always page 0)
- `CatalogBTree (1)` - B-tree node for catalog
- `ContentData (2)` - File content data
- `Freelist (3)` - Free block tracking
- `AuditLog (4)` - Audit log entries

**Checksum:**
- Optional SHA-256 of page data (4032 bytes)
- All zeros = skip verification (performance mode)
- Non-zero = verify on read (integrity mode)

### 4. Catalog (B-tree)

**Files:** `catalog/mod.rs`, `catalog/btree.rs`, `catalog/metadata.rs`
**Responsibilities:**
- Map file paths to metadata and block lists
- Efficient lookups, inserts, deletes
- Range queries for directory listings

**Metadata:**
```rust
pub struct FileMetadata {
    pub file_type: FileType,           // File or Directory
    pub size: u64,                     // Size in bytes
    pub blocks: Vec<u64>,              // Allocated block IDs
    pub created: SystemTime,           // Creation timestamp
    pub modified: SystemTime,          // Last modification
}
```

**B-tree Implementation:**
- In-memory B-tree (not yet paged to disk)
- Keys: File paths (String)
- Values: FileMetadata
- Operations: O(log n) search, insert, delete
- Range search for prefix matching (directory listings)

**Current Limitation:**
- B-tree nodes stored in single page (page 1)
- Serialized with `serde_json` (not optimal for large catalogs)
- Future: Multi-page B-tree with page splitting

### 5. Allocator (Hybrid)

**Files:** `allocator/hybrid.rs`, `allocator/bitmap.rs`, `allocator/extent.rs`
**Responsibilities:**
- Allocate and free blocks efficiently
- Minimize fragmentation
- Route to appropriate sub-allocator by size

**Strategy:**
```
File Size           Allocator      Rationale
---------           ---------      ---------
< 256KB             Bitmap         Fast, low overhead, fragmentation OK
≥ 256KB             Extent         Contiguous blocks, better I/O performance
```

#### Bitmap Allocator

**File:** `allocator/bitmap.rs`

```rust
pub struct BitmapAllocator {
    bitmap: Vec<u64>,          // Bitset (64 blocks per u64)
    total_blocks: usize,       // Total capacity
    allocated_count: usize,    // Blocks in use
    next_free_hint: usize,     // Optimization for allocation
}
```

**Algorithm:**
- Linear scan through bitmap to find free blocks
- Allocate first available (first-fit)
- O(n) allocation (acceptable for small files)
- O(1) free (clear bits)

**Performance:**
- Allocate 100K blocks: 4.15 ms
- Alloc/free cycle: 13.72 μs
- Fragmentation score: 4.67 μs

#### Extent Allocator

**File:** `allocator/extent.rs`

```rust
pub struct ExtentAllocator {
    free_extents: Vec<Extent>,     // List of free ranges
    total_blocks: usize,
}

pub struct Extent {
    start: u64,                    // Starting block ID
    length: u64,                   // Number of blocks
}
```

**Algorithm:**
- Maintain sorted list of free extents
- Best-fit allocation (smallest extent that fits)
- O(n) allocation where n = number of extents
- O(n) free with automatic coalescing

**Performance:**
- Allocate 100K blocks: 576 μs (301x faster than bitmap)
- Alloc/free cycle: 16.36 μs
- Fragmentation score: 535 ps (8,729x faster than bitmap)

#### Hybrid Allocator

**File:** `allocator/hybrid.rs`

**Routing Logic:**
```rust
const SMALL_FILE_THRESHOLD: u64 = 256 * 1024; // 256KB
const SMALL_FILE_BLOCKS: usize = 64;         // 64 blocks

fn allocate(&mut self, size: u64) -> Result<Vec<u64>> {
    if size < SMALL_FILE_THRESHOLD {
        self.bitmap.allocate(size)  // Small: use bitmap
    } else {
        self.extent.allocate(size)  // Large: use extent
    }
}

fn free(&mut self, blocks: &[u64]) -> Result<()> {
    if blocks.len() < SMALL_FILE_BLOCKS {
        self.bitmap.free(blocks)    // Small: free via bitmap
    } else {
        self.extent.free(blocks)    // Large: free via extent
    }
}
```

**Performance:**
- Hybrid small (100K blocks): 1.73 ms
- Hybrid large (100K blocks): 10.4 μs (16,700x faster!)
- Allocation by size: 4.99 μs (4KB) → 7.16 μs (1024KB)

### 6. Buffer Pool (ARC)

**File:** `buffer_pool.rs`
**Responsibilities:**
- Cache hot pages in memory
- Adapt to workload patterns (recency vs frequency)
- Provide fast cache hit/miss detection

**ARC Algorithm:**

ARC (Adaptive Replacement Cache) maintains four lists:
- **T1:** Recently accessed once (recency)
- **T2:** Frequently accessed (frequency)
- **B1:** Ghost entries evicted from T1
- **B2:** Ghost entries evicted from T2

```
┌───────────────────────────────────────────┐
│          ARC Cache Structure              │
├───────────────────────────────────────────┤
│  T1 (Recency)      │  T2 (Frequency)      │
│  [page_id, ...]    │  [page_id, ...]      │
│  Target: p pages   │  Target: c-p pages   │
├────────────────────┴──────────────────────┤
│  B1 (Ghost T1)     │  B2 (Ghost T2)       │
│  [page_id, ...]    │  [page_id, ...]      │
│  (metadata only)   │  (metadata only)     │
└───────────────────────────────────────────┘
```

**Adaptive Parameter `p`:**
- `0 ≤ p ≤ c` (capacity)
- Hit in B1 → increase p (favor recency)
- Hit in B2 → decrease p (favor frequency)
- Adaptation time: 164 μs

**Performance:**
- Cache hit (100 entries): 20.37 μs
- Cache hit (10,000 entries): 6.10 ms
- Cache miss: 3.26 ns (hash lookup only)
- Adaptation: 164 μs to shift workload

**Advantages over LRU:**
- Better hit rates on mixed workloads (80/20, scan-resistant)
- Self-tuning (no manual configuration)
- Ghost lists provide workload history

### 7. I/O Layer

**File:** `io.rs`
**Responsibilities:**
- Disk-backed file I/O
- Page-aligned reads/writes
- Sync and flush operations

**CartridgeFile:**
```rust
pub struct CartridgeFile {
    file: File,                    // OS file handle
    path: PathBuf,                 // File path
}
```

**Operations:**
- `create(path, header)` - Create new cartridge file
- `open(path)` - Open existing cartridge file
- `read_header()` - Read header from page 0
- `write_header(header)` - Write header to page 0
- `read_page_data(page_id)` - Read 4KB page
- `write_page_data(page_id, data)` - Write 4KB page
- `sync()` - Flush to disk (fsync)

**Disk Layout:**
```
Offset (bytes)    Page ID    Content
--------------    -------    -------
0                 0          Header
4096              1          Catalog B-tree
8192              2          Allocator state
12288             3+         Content data / freelist / audit
```

### 8. Compression

**File:** `compression.rs`
**Responsibilities:**
- Transparent LZ4/Zstd compression
- Compression-ratio-based decision making
- Fallback to uncompressed if not beneficial

**Supported Methods:**
- **LZ4:** Fast compression (9.77 GiB/s), fast decompression (38.12 GiB/s)
- **Zstd:** Better ratio (5.15 GiB/s), slower but higher compression

**Compression Format:**
```
[method: u8][compressed_data]
```

**Configuration:**
```rust
pub struct CompressionConfig {
    method: CompressionMethod,  // Lz4, Zstd, or None
    threshold: usize,           // Min size to compress (512 bytes)
    min_ratio: f32,             // Min compression ratio (0.9)
}
```

**Decision Logic:**
1. Skip if size < threshold
2. Compress with selected method
3. Check ratio: `compressed_len / original_len`
4. If ratio < min_ratio → use compressed
5. Otherwise → store uncompressed

**Performance:**
```
Algorithm  Size   Compress    Decompress   Ratio
---------  -----  ----------  -----------  -----
LZ4        64KB   9.52 GiB/s  37.13 GiB/s  ~2.0x
Zstd       64KB   4.87 GiB/s  N/A          ~4-5x
```

### 9. Encryption

**File:** `encryption.rs`
**Responsibilities:**
- AES-256-GCM authenticated encryption
- Unique nonce per page
- Tamper detection via authentication tag

**Format:**
```
[nonce: 12 bytes][ciphertext][auth_tag: 16 bytes]
```

**Security Properties:**
- **Confidentiality:** AES-256 (256-bit key)
- **Integrity:** GCM authentication tag (128-bit)
- **Nonce uniqueness:** Random 96-bit nonce per encryption
- **Key derivation:** Master key + page ID (future enhancement)

**Overhead:**
- 28 bytes per encrypted block (12 + 16)
- Minimal performance impact (hardware AES acceleration)

**Configuration:**
```rust
pub struct EncryptionConfig {
    master_key: [u8; 32],       // 256-bit master key
    enabled: bool,              // Enable/disable encryption
}
```

### 10. IAM Policies

**Files:** `iam/policy.rs`, `iam/engine.rs`, `iam/cache.rs`, `iam/pattern.rs`
**Responsibilities:**
- Fine-grained access control
- Wildcard pattern matching
- Cached policy evaluation (10,000+ evals/sec)

**Policy Structure:**
```rust
pub struct Policy {
    version: String,                // "2012-10-17" (AWS-compatible)
    statement: Vec<Statement>,      // List of rules
}

pub struct Statement {
    effect: Effect,                 // Allow or Deny
    action: Vec<Action>,            // Read, Write, Delete, etc.
    resource: Vec<String>,          // Path patterns ("/public/**")
    condition: Option<Condition>,   // Optional conditions
}
```

**Actions:**
- `Read` - Read file content
- `Write` - Modify file content
- `Create` - Create new files
- `Delete` - Delete files
- `List` - List directory contents
- `All` - All actions (wildcard)

**Pattern Matching:**
- `*` - Single path segment (`/docs/*.md`)
- `**` - Multiple segments (`/data/**`)
- Exact match (`/config.json`)

**Evaluation Logic:**
1. Check all Deny statements (explicit deny precedence)
2. If any Deny matches → access denied
3. Check all Allow statements
4. If any Allow matches → access granted
5. Otherwise → access denied (default deny)

**Caching:**
- LRU cache with 1000 entry default
- Cache key: `(action, resource)`
- Cache invalidation: Manual or on policy update
- Performance: 10,000+ evals/sec with cache

### 11. Audit Logging

**Files:** `audit/mod.rs`, `audit/ring_buffer.rs`
**Responsibilities:**
- Tamper-evident operation logging
- Low overhead (<1%) ring buffer
- File operation tracking

**Log Entry:**
```rust
pub struct AuditEntry {
    timestamp: u64,         // Microseconds since epoch
    operation: Operation,   // Create, Read, Update, Delete
    file_id: u64,           // File identifier (hash of path)
    session_id: u32,        // Session identifier
}
```

**Operations:**
- `Create` - File created
- `Read` - File read
- `Update` - File modified
- `Delete` - File deleted

**Ring Buffer:**
- Fixed-size circular buffer (default: 10,000 entries)
- Oldest entries overwritten when full
- Lock-free implementation for performance
- Batch flush to disk (async)

**Performance:**
- Write latency: <50 μs (P95)
- Overhead: <1% of operation time
- Throughput: 20,000+ logs/sec

### 12. Snapshots

**File:** `snapshot/mod.rs`
**Responsibilities:**
- Copy-on-write point-in-time snapshots
- Lightweight backup and versioning
- Snapshot metadata management

**Snapshot Metadata:**
```rust
pub struct SnapshotMetadata {
    id: u64,                        // Timestamp-based ID
    name: String,                   // Human-readable name
    description: String,            // Description
    created_at: u64,                // Unix timestamp (μs)
    parent_path: PathBuf,           // Parent cartridge path
    header: Header,                 // Cartridge header at snapshot time
    modified_pages: HashSet<u64>,   // Pages modified since snapshot
    size_bytes: u64,                // Snapshot size
}
```

**Snapshot Storage:**
```
snapshots/
  snapshot_<id>/
    metadata.json         # SnapshotMetadata
    pages.bin             # Serialized pages
```

**Operations:**
- `create_snapshot()` - Create new snapshot
- `restore_snapshot()` - Restore from snapshot
- `delete_snapshot()` - Delete snapshot
- `list_snapshots()` - List all snapshots
- `prune_old_snapshots(N)` - Keep only N most recent

**Performance:**
- Snapshot creation: ~3-5 ms for 100 pages
- Snapshot restoration: ~5-10 ms for 100 pages
- Storage overhead: Only modified pages stored

### 13. Engram Integration

**File:** `engram_integration.rs`
**Responsibilities:**
- Freeze mutable cartridges to immutable engrams
- Export IAM policies to engram manifests
- Compression and signing integration

**Freezing Process:**
```
Cartridge (mutable)
    ↓
1. Collect all files from catalog
2. Extract IAM policy (if present)
3. Build engram manifest (JSON)
4. Add files with compression
5. Add IAM policy file
6. Finalize and sign
    ↓
Engram (immutable, compressed, signed)
```

**Engram Manifest:**
```json
{
  "version": "1.0.0",
  "id": "cartridge-1234567890",
  "author": "Author Name",
  "description": "Frozen cartridge archive",
  "created": "2025-11-20T12:00:00Z",
  "immutable": true,
  "type": "cartridge",
  "capabilities": [
    "read:public/**",
    "write:data/**"
  ],
  "files": {
    "/readme.txt": {"size": 1024, "type": "file"},
    "/data.bin": {"size": 2048, "type": "file"}
  },
  "metadata": {
    "compression": "Zstd",
    "source": "cartridge"
  }
}
```

**Performance:**
- Freeze 100 files (1MB total): ~200 ms with Zstd
- Freeze 100 files (1MB total): ~100 ms with LZ4
- Compression ratios: 2-5x (text), 1.1-1.5x (binary)

### 14. SQLite VFS

**Files:** `vfs/vfs.rs`, `vfs/file.rs`
**Responsibilities:**
- Implement `sqlite3_vfs` interface
- Provide filesystem operations for SQLite
- Map SQLite files to cartridge paths

**VFS Callbacks:**
```rust
// Implemented in vfs.rs and file.rs
vfs_open()              // Open/create database file
vfs_delete()            // Delete database file
vfs_access()            // Check file existence
vfs_full_pathname()     // Resolve full path
vfs_randomness()        // Generate random bytes
vfs_sleep()             // Sleep microseconds
vfs_current_time()      // Get current Julian day
```

**Integration:**
```rust
// Register VFS with SQLite
let cart = Cartridge::create("db.cart", 10000)?;
register_vfs(Arc::new(Mutex::new(cart)))?;

// Use SQLite with cartridge VFS
let conn = Connection::open_with_flags(
    "mydb.db",
    OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_READ_WRITE,
)?;

// SQLite operations work transparently
conn.execute("CREATE TABLE ...", [])?;
```

**Performance:**
- SQLite read: ~18 GiB/s (cached)
- SQLite write: ~9 GiB/s
- Transaction overhead: <1% vs native filesystem

---

## Data Flow

### File Creation Flow

```
create_file("/data.txt", b"Hello")
    │
    ├─→ 1. Check IAM policy (check_access)
    │       └─→ PolicyEngine::evaluate()
    │
    ├─→ 2. Check if file exists (catalog.get)
    │       └─→ Return error if exists
    │
    ├─→ 3. Allocate blocks (allocator.allocate)
    │       ├─→ Size < 256KB → Bitmap allocator
    │       └─→ Size ≥ 256KB → Extent allocator
    │
    ├─→ 4. Write content to pages (write_content)
    │       ├─→ Split content into 4KB chunks
    │       ├─→ Store in pages HashMap
    │       └─→ Mark pages as dirty
    │
    ├─→ 5. Create metadata (FileMetadata::new)
    │       └─→ Set size, blocks, timestamps
    │
    ├─→ 6. Insert into catalog (catalog.insert)
    │       └─→ B-tree insert (path → metadata)
    │
    ├─→ 7. Update header (header.free_blocks)
    │
    └─→ 8. Audit log (audit_log)
            └─→ Log Operation::Create
```

### File Read Flow

```
read_file("/data.txt")
    │
    ├─→ 1. Check IAM policy (check_access)
    │       └─→ PolicyEngine::evaluate()
    │
    ├─→ 2. Audit log (audit_log)
    │       └─→ Log Operation::Read
    │
    ├─→ 3. Lookup metadata (catalog.get)
    │       └─→ B-tree search (O(log n))
    │
    ├─→ 4. Read content from blocks (read_content)
    │       ├─→ For each block:
    │       │   ├─→ Check in-memory cache (pages HashMap)
    │       │   ├─→ If not cached: load from disk (CartridgeFile::read_page_data)
    │       │   └─→ Cache the page
    │       └─→ Concatenate all blocks
    │
    └─→ 5. Return content
```

### Flush Flow

```
flush()
    │
    ├─→ 1. Write header (file.write_header)
    │       └─→ Serialize Header to page 0
    │
    ├─→ 2. Write catalog state (page 1)
    │       ├─→ Serialize B-tree to JSON
    │       └─→ Write to page 1
    │
    ├─→ 3. Write allocator state (page 2)
    │       ├─→ Serialize allocator to JSON
    │       └─→ Write to page 2
    │
    ├─→ 4. Write dirty pages
    │       ├─→ For each dirty page:
    │       │   └─→ file.write_page_data(page_id, data)
    │       └─→ Clear dirty_pages set
    │
    └─→ 5. Sync to disk (file.sync)
            └─→ fsync() system call
```

### Snapshot Creation Flow

```
create_snapshot("v1", "First version")
    │
    ├─→ 1. Create metadata (SnapshotMetadata::new)
    │       └─→ Generate timestamp-based ID
    │
    ├─→ 2. Calculate size
    │       └─→ Sum all page sizes
    │
    ├─→ 3. Write snapshot to disk
    │       ├─→ Create snapshot_<id>/ directory
    │       ├─→ Write metadata.json
    │       └─→ Write pages.bin (serialized pages)
    │
    └─→ 4. Register snapshot
            └─→ Add to SnapshotManager
```

---

## Memory Layout

### Cartridge In-Memory Structure

```
Cartridge (total: ~100-500 KB for typical usage)
├─ Header (296 bytes)
│  └─ Archive metadata
│
├─ Allocator (~10-50 KB)
│  ├─ BitmapAllocator
│  │  └─ bitmap: Vec<u64> (~1.25 KB per 10,000 blocks)
│  └─ ExtentAllocator
│     └─ free_extents: Vec<Extent> (~24 bytes per extent)
│
├─ Catalog (~5-20 KB)
│  └─ B-tree nodes (in-memory)
│     └─ HashMap<String, FileMetadata>
│
├─ Pages cache (~4 KB per cached page)
│  └─ HashMap<u64, Vec<u8>>
│     └─ Up to ARC cache capacity (default: 1000 pages = 4 MB)
│
├─ Dirty pages (~8 bytes per dirty page)
│  └─ HashSet<u64>
│
├─ Buffer Pool (~20-200 KB)
│  ├─ T1, T2 lists (recency/frequency)
│  ├─ B1, B2 ghost lists
│  └─ pages: HashMap<u64, Arc<Page>> (shared with Cartridge.pages)
│
├─ IAM Policy (~1-10 KB)
│  └─ PolicyEngine with cache (1000 entries = ~50 KB)
│
└─ Audit Logger (~1 MB)
   └─ Ring buffer (10,000 entries × 32 bytes = ~320 KB)
```

### Page Memory Layout

```
Page (4096 bytes)
├─ PageHeader (64 bytes)
│  ├─ page_type: u8 (1 byte)
│  ├─ checksum: [u8; 32] (32 bytes, SHA-256)
│  └─ reserved: [u8; 31] (31 bytes)
│
└─ Data (4032 bytes)
   └─ Content varies by page type
```

### File Memory Overhead

For a file stored in cartridge:

```
File: 10 KB
├─ Content pages: 3 × 4KB = 12 KB (with padding)
│  └─ Actual data: 10 KB + 96 bytes overhead (3 × 32 checksum)
│
├─ Catalog entry: ~150 bytes
│  ├─ Path string: ~20 bytes
│  └─ FileMetadata: ~130 bytes (blocks vector, timestamps)
│
└─ Allocator: 3 bits in bitmap or 1 extent entry
   └─ Negligible (~1 byte)

Total overhead: ~12.5% for 10KB file
```

---

## Threading Model

### Current Implementation (v0.1)

Cartridge is currently **single-threaded** with `Arc<Mutex<Cartridge>>` for shared access:

```rust
// Shared cartridge between threads
let cart = Arc::new(Mutex::new(Cartridge::new(1000)));

// Thread-safe access
let cart_clone = cart.clone();
thread::spawn(move || {
    let mut cart = cart_clone.lock().unwrap();
    cart.create_file("/thread1.txt", b"data").unwrap();
});
```

**Concurrency:**
- Multiple readers: **Blocked** (Mutex allows only one accessor)
- Read + Write: **Blocked**
- Write + Write: **Blocked**

**Thread Safety:**
- All public methods are `&mut self` (require exclusive access)
- `Arc<Mutex<>>` provides safe sharing but serializes all access
- BufferPool uses interior mutability (`parking_lot::Mutex`)

### Future: Multi-threaded (v0.2)

Planned improvements for concurrent access:

1. **Read-Write Lock (RwLock):**
   ```rust
   Arc<RwLock<Cartridge>>
   // Multiple readers, exclusive writer
   ```

2. **MVCC (Multi-Version Concurrency Control):**
   ```rust
   // Each read gets a snapshot view
   // Writes create new versions
   // No read blocking
   ```

3. **Async I/O:**
   ```rust
   async fn read_file(&self, path: &str) -> Result<Vec<u8>>;
   // Non-blocking I/O with tokio
   ```

4. **Lock-Free Structures:**
   - Lock-free catalog (concurrent B-tree)
   - Lock-free allocator (atomic operations)
   - Lock-free page cache (concurrent HashMap)

---

## Integration Points

### 1. SQLite VFS Integration

Cartridge implements the full `sqlite3_vfs` interface:

```
SQLite Engine
    │
    └─→ VFS Layer (cartridge VFS)
            │
            ├─→ xOpen()      → CartridgeFile::open()
            ├─→ xRead()      → Cartridge::read_file()
            ├─→ xWrite()     → Cartridge::write_file()
            ├─→ xDelete()    → Cartridge::delete_file()
            └─→ xAccess()    → Cartridge::exists()
```

**Usage:**
```rust
register_vfs(Arc::new(Mutex::new(cartridge)))?;
let conn = Connection::open_with_flags("db.sqlite", flags)?;
// SQLite now uses cartridge for all file I/O
```

### 2. Engram Integration

Cartridge can be frozen into immutable Engram archives:

```
Cartridge (mutable workspace)
    ↓ freeze()
Engram (immutable archive)
    ├─ Compressed with LZ4/Zstd
    ├─ Signed with Ed25519
    └─ Includes IAM policy in manifest
```

**Workflow:**
```rust
// Development: mutable cartridge
let mut cart = Cartridge::create("workspace.cart", 10000)?;
cart.create_file("/data.txt", data)?;

// Production: freeze to engram
let freezer = EngramFreezer::new_default(name, version, author);
freezer.freeze(&mut cart, Path::new("release.eng"))?;
```

### 3. ARC Buffer Pool

The buffer pool integrates with the page system:

```
Cartridge::read_file()
    │
    └─→ For each block:
        ├─→ BufferPool::get(page_id)
        │   ├─→ Cache hit → Return Arc<Page> (instant)
        │   └─→ Cache miss → Load from disk
        │
        ├─→ If miss: CartridgeFile::read_page_data()
        │
        └─→ BufferPool::put(page_id, page)
```

**Benefits:**
- Reduces disk I/O for hot data
- Adapts to workload (recency vs frequency)
- Shared Arc<Page> avoids copies

### 4. Compression/Encryption Pipeline

Transparent compression and encryption:

```
Cartridge::write_file(data)
    │
    ├─→ Optionally compress (if enabled)
    │   ├─→ compress_if_beneficial()
    │   └─→ Returns (data, was_compressed)
    │
    ├─→ Optionally encrypt (if enabled)
    │   ├─→ encrypt_if_enabled()
    │   └─→ Returns (data, was_encrypted)
    │
    └─→ Write to pages
```

**Read path:**
```
Cartridge::read_file()
    │
    ├─→ Read from pages
    │
    ├─→ Optionally decrypt (if was_encrypted)
    │   └─→ decrypt_if_encrypted()
    │
    └─→ Optionally decompress (if was_compressed)
        └─→ decompress()
```

---

## Design Decisions

### 1. Fixed 4KB Pages

**Rationale:**
- Aligns with OS filesystem block size (4KB on most systems)
- Aligns with memory page size (4KB on x86_64, ARM)
- Optimal for SSD writes (minimize write amplification)
- Simplifies allocation (no variable-sized blocks)

**Trade-offs:**
- Small files waste space (internal fragmentation)
- Large files need multiple pages (external fragmentation)
- Solution: Hybrid allocator mitigates both issues

### 2. Hybrid Allocator (Bitmap + Extent)

**Rationale:**
- Small files (<256KB): Bitmap allocator is fast enough, low overhead
- Large files (≥256KB): Extent allocator provides contiguous blocks for better I/O
- Threshold at 256KB (64 blocks) balances trade-offs

**Alternatives Considered:**
- Pure bitmap: Simple but poor performance for large files
- Pure extent: Complex for small files, high overhead
- Buddy allocator: More complex, not justified for our use case

**Performance:**
- Hybrid small: 1.73 ms for 100K blocks
- Hybrid large: 10.4 μs for 100K blocks (16,700x faster!)

### 3. ARC Buffer Pool (vs LRU)

**Rationale:**
- ARC adapts to workload (recency vs frequency)
- Better hit rates on mixed workloads (80/20, scan patterns)
- Self-tuning (no manual configuration)

**Alternatives Considered:**
- LRU: Simple but poor on sequential scans
- LFU: Good for frequency but misses recency
- 2Q: Better than LRU but not as adaptive as ARC

**Performance:**
- Hit rate: ~66% on random access (vs ~50% for LRU)
- Adaptation time: 164 μs

### 4. In-Memory B-tree (Not Yet Paged)

**Current:**
- B-tree nodes stored in memory
- Serialized to single page (page 1) on flush

**Rationale:**
- Simplifies implementation for v0.1
- Sufficient for small-to-medium catalogs (<10,000 files)
- Enables fast development and testing

**Future (v0.2):**
- Multi-page B-tree with page splitting
- Persist B-tree nodes to disk pages
- Support for millions of files

### 5. SHA-256 Checksums (Optional)

**Rationale:**
- Optional per-page verification
- Detect corruption and tampering
- Minimal overhead when disabled (all-zero checksum)

**Trade-offs:**
- Overhead: ~1-2 μs per page to compute
- Storage: 32 bytes per page (0.78% overhead)
- Benefit: Strong integrity guarantees

**Usage:**
- Enable for critical data (databases, configs)
- Disable for performance-critical paths (large files)

### 6. IAM Policies with Caching

**Rationale:**
- Fine-grained access control for multi-tenant systems
- Cache evaluations for high performance (10,000+ evals/sec)
- AWS-compatible policy format (familiar to developers)

**Trade-offs:**
- Memory overhead: ~50 KB for 1000-entry cache
- Complexity: Policy evaluation logic
- Benefit: Enterprise-grade access control

---

## Performance Characteristics

### Allocation Performance

From benchmarks (`performance.md`):

| Allocator | Operation | Latency | Throughput |
|-----------|-----------|---------|------------|
| Bitmap | Allocate 100K blocks | 4.15 ms | 24,096 blocks/ms |
| Extent | Allocate 100K blocks | 576 μs | 173,611 blocks/ms |
| Hybrid (small) | Allocate 100K blocks | 1.73 ms | 57,803 blocks/ms |
| Hybrid (large) | Allocate 100K blocks | 10.4 μs | 9,615,385 blocks/ms |

**Speedup:**
- Extent vs Bitmap: **301x faster**
- Hybrid (large) vs Hybrid (small): **16,700x faster**

### File I/O Performance

| Size | Write (P50) | Write Throughput | Read (P50) | Read Throughput |
|------|-------------|------------------|------------|-----------------|
| 1KB | 1.52 μs | 643.96 MiB/s | 274 ns | 3.47 GiB/s |
| 4KB | 1.25 μs | 3.05 GiB/s | 279 ns | 13.66 GiB/s |
| 16KB | 2.22 μs | 6.86 GiB/s | 880 ns | 17.34 GiB/s |
| **64KB** | **6.48 μs** | **9.41 GiB/s** | **3.41 μs** | **17.91 GiB/s** |
| 256KB | 28.85 μs | 8.46 GiB/s | 15.17 μs | 16.10 GiB/s |
| 1MB | 430.4 μs | 2.27 GiB/s | 399.8 μs | 2.44 GiB/s |

**Optimal Block Size:** 64KB (maximum throughput)

### Compression Performance

| Algorithm | Size | Compress | Decompress | Ratio |
|-----------|------|----------|------------|-------|
| LZ4 | 64KB | 9.52 GiB/s | 37.13 GiB/s | ~2x |
| Zstd | 64KB | 4.87 GiB/s | N/A | ~4-5x |

**LZ4 Decompression:** **3.9x faster** than compression

### ARC Cache Performance

| Pool Size | Get (hit) | Put | Miss |
|-----------|-----------|-----|------|
| 100 | 20.37 μs | 24.98 μs | 3.26 ns |
| 1,000 | 255.0 μs | 285.7 μs | 3.26 ns |
| 10,000 | 6.10 ms | 6.11 ms | 3.81 ns |

**Hit Rate:** 66% on random access, 90%+ on 80/20 workload

### IAM Policy Performance

| Operation | Latency | Throughput |
|-----------|---------|------------|
| Cached evaluation | <1 μs | 1,000,000+ evals/sec |
| Uncached evaluation | 10-20 μs | 50,000+ evals/sec |
| Cache lookup | ~100 ns | N/A |

### Snapshot Performance

| Operation | Latency (100 pages) |
|-----------|---------------------|
| Create snapshot | 3-5 ms |
| Restore snapshot | 5-10 ms |
| List snapshots | <1 ms |
| Delete snapshot | 1-2 ms |

---

## Scalability Analysis

### File Count Scaling

Current B-tree implementation (in-memory):

| Files | Catalog Size (JSON) | Flush Time | Limitation |
|-------|---------------------|------------|------------|
| 100 | ~20 KB | <1 ms | ✅ Excellent |
| 1,000 | ~200 KB | ~10 ms | ✅ Good |
| 10,000 | ~2 MB | ~100 ms | ⚠️ Acceptable |
| 100,000 | ~20 MB | ~1 sec | ❌ Too slow |

**Future:** Multi-page B-tree will scale to millions of files.

### Storage Size Scaling

| Total Blocks | Archive Size | Memory Overhead | Performance |
|--------------|--------------|-----------------|-------------|
| 1,000 | 4 MB | ~100 KB | ✅ Excellent |
| 10,000 | 40 MB | ~500 KB | ✅ Excellent |
| 100,000 | 400 MB | ~5 MB | ✅ Good |
| 1,000,000 | 4 GB | ~50 MB | ⚠️ Acceptable |
| 10,000,000 | 40 GB | ~500 MB | ❌ High memory |

**Limitation:** Bitmap allocator (Vec<u64>) scales linearly with block count.
**Future:** Paged bitmap or hierarchical bitmap for large archives.

### Concurrent Access Scaling

Current (single-threaded with Mutex):

| Threads | Throughput | Latency |
|---------|------------|---------|
| 1 | 100% (baseline) | 1x |
| 2 | ~50% | ~2x |
| 4 | ~25% | ~4x |
| 8 | ~12.5% | ~8x |

**Future (RwLock):**
- Multiple readers: Near-linear scaling
- Read-heavy workloads: 8x throughput on 8 cores

**Future (MVCC):**
- No read blocking: Linear scaling for reads
- Writes create new versions: Controlled overhead

---

## Conclusion

Cartridge provides a robust, high-performance storage system optimized for embedded systems. The hybrid allocator, ARC caching, and optional compression/encryption make it suitable for a wide range of applications from Raspberry Pi deployments to enterprise servers.

**Strengths:**
- Excellent I/O performance (18 GiB/s reads, 9 GiB/s writes)
- Adaptive caching with ARC (better than LRU)
- Hybrid allocation (optimal for all file sizes)
- SQLite VFS integration (database support)
- Snapshot support (backup and versioning)
- IAM policies (enterprise access control)

**Current Limitations:**
- Single-threaded (serialized access with Mutex)
- In-memory B-tree (limited to ~10,000 files efficiently)
- No compaction (fragmentation accumulates over time)

**Future Enhancements (v0.2):**
- Multi-threaded with RwLock or MVCC
- Multi-page B-tree (scale to millions of files)
- Compaction and defragmentation
- WAL for crash recovery
- Async I/O with tokio

For detailed performance metrics, see `performance.md`.
For file format specification, see `SPECIFICATION.md`.
