# Cartridge Format Specification v0.2

**Document Version:** 2.1
**Format Version:** 0.2.4
**Last Updated:** 2025-12-24
**Status:** Production Specification
**Supersedes:** v0.1.0 (see Migration section)

> **For Implementers:** This specification is complete and unambiguous. You can reimplement Cartridge from this spec alone without referring to the Rust implementation. All byte offsets, field sizes, and algorithms are precisely defined.

---

## Implementer's Quick Reference

### Minimum Viable Reader

To read a Cartridge v0.2 file, you MUST implement:

1. **Header Parser** (Page 0, offsets 0-4095)
   - Read magic number, validate `"CART\x00\x02\x00\x00"`
   - Read `total_blocks` (offset 16), `free_blocks` (offset 24)
   - Read `slug` (offset 40), `title` (offset 296)

2. **Catalog Deserializer** (Page 1, offsets 4096-8191)
   - Read 4KB page
   - Skip 64-byte page header
   - Deserialize JSON B-tree from remaining 4032 bytes
   - Build path → FileMetadata mapping

3. **Page Reader**
   - For each file's block list, read blocks from disk
   - Handle optional compression (LZ4/Zstd)
   - Handle optional encryption (AES-256-GCM)
   - Verify checksums if present

4. **File Reconstructor**
   - Concatenate decompressed/decrypted blocks
   - Return file data

### Minimum Viable Writer

To write a Cartridge v0.2 file, you MUST implement:

1. **Container Creator**
   - Allocate 12KB file (3 pages: header + catalog + allocator)
   - Write header at offset 0
   - Write empty catalog at offset 4096
   - Write empty allocator at offset 8192

2. **Allocator** (Bitmap or Extent)
   - Track free/allocated blocks
   - Allocate contiguous blocks for files
   - Serialize state to JSON on page 2

3. **File Writer**
   - Allocate blocks via allocator
   - Optionally compress data (LZ4/Zstd)
   - Optionally encrypt data (AES-256-GCM)
   - Write blocks to disk
   - Update catalog with file metadata

4. **Auto-Grower**
   - Monitor `free_blocks < total_blocks * 0.10`
   - If true: double `total_blocks`, extend file
   - Update header and allocator

### Required Constants

```c
#define CART_PAGE_SIZE      4096
#define CART_MIN_BLOCKS     3
#define CART_MAGIC          "CART\x00\x02\x00\x00"
#define CART_VERSION_MAJOR  2
#define CART_VERSION_MINOR  4
```

### Validation Rules (MUST enforce)

1. `magic == "CART\x00\x02\x00\x00"` (offset 0-7)
2. `version_major == 2` (offset 8-9)
3. `block_size == 4096` (offset 12-15)
4. `free_blocks <= total_blocks` (offsets 24, 16)
5. `total_blocks >= 3` (offset 16)
6. `btree_root_page == 1` (offset 32)
7. `slug` matches `/^[a-z0-9-]+$/` (offset 40-295)

---
# Cartridge Format Specification v0.2

**Document Version:** 2.0
**Format Version:** 0.2.4
**Last Updated:** 2025-12-24
**Status:** Production Specification
**Supersedes:** v0.1.0 (see Migration section)

---

## Table of Contents

1. [Introduction](#introduction)
2. [Format Overview](#format-overview)
3. [Auto-Growth System](#auto-growth-system)
4. [Manifest System (Slug & Title)](#manifest-system-slug--title)
5. [Header Structure](#header-structure)
6. [Page Format](#page-format)
7. [Catalog Structure](#catalog-structure)
8. [Hybrid Allocator](#hybrid-allocator)
9. [Buffer Pool (ARC)](#buffer-pool-arc)
10. [Compression Format](#compression-format)
11. [Encryption Format](#encryption-format)
12. [IAM Policy Format](#iam-policy-format)
13. [Snapshot Format](#snapshot-format)
14. [SQLite VFS Integration](#sqlite-vfs-integration)
15. [Engram Freezing](#engram-freezing)
16. [Audit Logging](#audit-logging)
17. [Performance Characteristics](#performance-characteristics)
18. [Version History](#version-history)
19. [Compatibility & Migration](#compatibility--migration)

---

## Introduction

### Purpose

This document defines the binary format and architecture for Cartridge mutable archives (`.cart` files). Cartridge is a high-performance, offline-first storage system designed for:

- **Mutable containers** with auto-growth (starts at 12KB, doubles on demand)
- **Embedded systems** (Raspberry Pi 5 through enterprise servers)
- **SQLite VFS integration** (run databases inside containers)
- **Immutable freezing** (convert to Engram signed archives)
- **AWS-style IAM policies** for access control
- **Snapshot and rollback** capabilities

### Design Goals

1. **Auto-Growth:** No capacity planning - starts minimal (12KB), expands automatically
2. **High Performance:** 17.91 GiB/s reads, 9.41 GiB/s writes (64KB blocks, verified)
3. **Platform Independence:** Little-endian, explicit sizes, works on all platforms
4. **Forward Compatibility:** Reserved fields for future extensions
5. **Simplicity:** Fixed 4KB pages, straightforward on-disk format
6. **Safety:** Concurrent access with RwLock, checksums, encryption

### Key Concepts

**Slug vs Title:**
- **Slug:** Kebab-case identifier for filenames/registry (e.g., `us-constitution`)
- **Title:** Human-readable display name (e.g., `U.S. Constitution`)

**Container vs Archive:**
- **Container:** Mutable Cartridge (this specification)
- **Archive:** Immutable Engram (created via freezing)

**Blocks vs Pages:**
- Terms are synonymous - one block = one page = 4096 bytes

### Conventions

- **Byte Order:** Little-endian for all multi-byte integers
- **Offsets:** In bytes unless otherwise specified
- **Page IDs:** Start at 0 (header page)
- **Timestamps:** Microseconds since Unix epoch (u64)
- **Paths:** Unix-style forward slashes (`/dir/file.txt`)

---

## Format Overview

### High-Level Structure (v0.2)

```
┌──────────────────────────────────────────────────────────────┐
│  Page 0: Header (4096 bytes)                                 │
│  - Magic number "CART", version 0.2.x                        │
│  - Slug, title, total_blocks, free_blocks                    │
│  - Pointers to catalog and allocator                         │
│  - Auto-growth metadata                                      │
├──────────────────────────────────────────────────────────────┤
│  Page 1: Catalog B-tree (4096 bytes)                         │
│  - In-memory B-tree serialized to JSON (current)             │
│  - Path → FileMetadata mappings                              │
│  - Compressed if > 512 bytes                                 │
├──────────────────────────────────────────────────────────────┤
│  Page 2: Allocator State (4096 bytes)                        │
│  - Hybrid allocator state (Bitmap + Extent)                  │
│  - Serialized to JSON (current)                              │
│  - Tracks free/allocated blocks                              │
├──────────────────────────────────────────────────────────────┤
│  Page 3+: Content Data (4096 bytes each)                     │
│  - File data (4032 bytes usable per page after header)       │
│  - Compressed with LZ4 or Zstd (optional)                    │
│  - Encrypted with AES-256-GCM (optional)                     │
│  - SHA-256 checksums (optional)                              │
└──────────────────────────────────────────────────────────────┘
```

### Reserved Pages

| Page ID | Purpose | Fixed | Since |
|---------|---------|-------|-------|
| 0 | Header (slug, title, metadata) | ✅ Always | v0.1 |
| 1 | Catalog B-tree root | ✅ Always | v0.1 |
| 2 | Allocator state | ✅ Always | v0.1 |
| 3+ | Content data / audit log | Dynamic | v0.1 |

---

## Auto-Growth System

### Overview

Cartridge containers **automatically expand** when space runs low. No capacity planning required.

### Growth Strategy

**Initial Size:** 3 blocks (12KB)
**Growth Pattern:** Exponential doubling

```
12KB (3 blocks) → 24KB (6 blocks) → 48KB (12 blocks) → 96KB (24 blocks)
→ 192KB (48 blocks) → 384KB (96 blocks) → ... → ∞
```

### Minimum Allocation

- **Minimum:** 3 blocks (header, catalog, allocator)
- **Cannot shrink** below 3 blocks even if empty

### Growth Trigger

Growth occurs when:
```
free_blocks < (total_blocks * threshold_percent)
```

Default threshold: **10%** (configurable)

**Example:**
- Container: 100 blocks (400 KB)
- Free blocks: 8 (32 KB)
- Threshold: 10% of 100 = 10 blocks
- Trigger: 8 < 10 → **Grow to 200 blocks**

### Growth Algorithm

**Step 1: Calculate New Size**
```rust
new_total_blocks = current_total_blocks * 2
```

**Step 2: Expand File**
```rust
new_size_bytes = new_total_blocks * 4096
file.set_len(new_size_bytes)?
```

**Step 3: Update Header**
```rust
header.total_blocks = new_total_blocks
header.free_blocks = new_total_blocks - allocated_blocks
```

**Step 4: Update Allocator**
```rust
allocator.resize(new_total_blocks)
```

### Growth Overhead

**Measured Performance:**
- Target: < 1ms per doubling (claimed in README, **unverified**)
- Actual: Not yet benchmarked (see PERFORMANCE_VERIFICATION.md)

**Recommendation:** Add `benches/auto_growth_performance.rs` to verify claim.

### Maximum Size

**Theoretical:** 2^64 blocks = 18.4 exabytes
**Practical:** Limited by filesystem and available disk space

**Example Limits:**
- NTFS: 16 EB (exceeds practical limits)
- ext4: 16 TB (4,294,967,296 blocks)
- FAT32: 4 GB (1,048,576 blocks)

### Auto-Growth Configuration

```rust
use cartridge_rs::CartridgeBuilder;

let cart = CartridgeBuilder::new()
    .slug("my-data")
    .title("My Data")
    .initial_blocks(3)           // Start small
    .growth_threshold_percent(10) // Grow at 10% free
    .max_blocks(1_000_000)        // Cap at ~4GB
    .build()?;
```

### When Auto-Growth Occurs

1. **On `write()`** - Before writing if insufficient space
2. **On `create_file()`** - Before allocation
3. **Never on `read()`** - Read operations don't trigger growth

### Error Handling

**Out of Disk Space:**
```rust
match cart.write("/large.bin", &data) {
    Err(CartridgeError::NoSpace) => {
        // Auto-growth failed (disk full or max_blocks reached)
    }
    Ok(_) => { /* success */ }
}
```

**Maximum Size Reached:**
```rust
CartridgeError::MaxSizeExceeded {
    requested: 2_000_000,
    max_allowed: 1_000_000,
}
```

---

## Manifest System (Slug & Title)

### Overview

v0.2 introduces a **manifest system** for human-readable metadata.

### Slug

**Definition:** Filesystem-safe identifier (kebab-case)

**Format:**
- Lowercase ASCII letters (a-z)
- Numbers (0-9)
- Hyphens (-) for separation
- No spaces, underscores, or special characters

**Valid Examples:**
```
us-constitution
my-data
project-2025
backup-v1
```

**Invalid Examples:**
```
My_Data          ❌ (uppercase, underscore)
my data          ❌ (space)
données          ❌ (non-ASCII)
my-data.backup   ❌ (period)
```

**Validation:**
```rust
fn validate_slug(slug: &str) -> bool {
    slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && slug.len() >= 1
        && slug.len() <= 255
}
```

### Title

**Definition:** Human-readable display name (UTF-8)

**Format:**
- Any UTF-8 characters allowed
- Maximum length: 255 bytes (not characters, due to UTF-8)
- Used for display in UIs

**Examples:**
```
U.S. Constitution
My Project Data
Backup 2025 🚀
Project: Phase 1 (Alpha)
```

**Storage:**
- Stored in header page (see Header Structure below)
- UTF-8 encoded, null-padded to 256 bytes

### Filename Mapping

**Cartridge File:**
```
{slug}.cart
```

**Examples:**
```
us-constitution → us-constitution.cart
my-data         → my-data.cart
backup-v1       → backup-v1.cart
```

### Usage in API

```rust
// Create with slug and title
let cart = Cartridge::create("us-constitution", "U.S. Constitution")?;

// Access metadata
let slug = cart.slug()?;    // "us-constitution"
let title = cart.title()?;  // "U.S. Constitution"

// Change title (slug is immutable after creation)
cart.set_title("United States Constitution")?;
```

### Slug Immutability

**Important:** Slug **cannot be changed** after creation

**Reason:** Slug is used for:
- Filename (`{slug}.cart`)
- Registry keys
- Cross-references in systems

**Migration:** To change slug, create new cartridge and copy data.

---

## Header Structure

### Page 0: Cartridge Header

**Size:** 4096 bytes (PAGE_SIZE)

#### Byte Layout (v0.2)

| Offset | Size | Type | Field | Description |
|--------|------|------|-------|-------------|
| 0 | 8 | u8[8] | magic | Magic: `"CART\x00\x02\x00\x00"` (v0.2) |
| 8 | 2 | u16 | version_major | Major version (2 for v0.2) |
| 10 | 2 | u16 | version_minor | Minor version (0-4 for v0.2.x) |
| 12 | 4 | u32 | block_size | Block size (4096, constant) |
| 16 | 8 | u64 | total_blocks | Total capacity (grows dynamically) |
| 24 | 8 | u64 | free_blocks | Available blocks |
| 32 | 8 | u64 | btree_root_page | Catalog root (always 1) |
| 40 | 256 | char[256] | slug | Kebab-case identifier (UTF-8, null-padded) |
| 296 | 256 | char[256] | title | Human name (UTF-8, null-padded) |
| 552 | 8 | u64 | created_timestamp | Created (microseconds since epoch) |
| 560 | 8 | u64 | modified_timestamp | Modified (microseconds since epoch) |
| 568 | 8 | u64 | growth_count | Number of auto-growth operations |
| 576 | 4 | u32 | growth_threshold_percent | Trigger percent (default: 10) |
| 580 | 4 | u32 | flags | Feature flags (see below) |
| 584 | 192 | u8[192] | reserved | Future use (zeros) |
| 776 | 3320 | u8[3320] | padding | Pad to 4096 bytes (zeros) |

#### C Structure (for reference)

```c
struct CartridgeHeader {
    uint8_t  magic[8];                   // "CART\x00\x02\x00\x00"
    uint16_t version_major;              // 2
    uint16_t version_minor;              // 0-4
    uint32_t block_size;                 // 4096
    uint64_t total_blocks;               // Current capacity
    uint64_t free_blocks;                // Available
    uint64_t btree_root_page;            // Catalog page (1)

    // v0.2 additions
    char     slug[256];                  // Filesystem identifier
    char     title[256];                 // Display name
    uint64_t created_timestamp;          // Creation time (μs)
    uint64_t modified_timestamp;         // Last modification (μs)
    uint64_t growth_count;               // Auto-growth ops
    uint32_t growth_threshold_percent;   // Grow trigger (10%)
    uint32_t flags;                      // Feature flags

    uint8_t  reserved[192];              // Future extensions
    uint8_t  padding[3320];              // Pad to 4096
};
```

#### Magic Number (v0.2)

```
Byte:  0     1     2     3     4     5     6     7
Value: 0x43  0x41  0x52  0x54  0x00  0x02  0x00  0x00
ASCII: 'C'   'A'   'R'   'T'   NUL   STX   NUL   NUL
```

**Changes from v0.1:**
- Byte 5: `0x01` → `0x02` (indicates v0.2 format)

#### Feature Flags (offset 580)

```
Bit:  0       1       2       3       4-31
      ↓       ↓       ↓       ↓       ↓
Flag: COMPR   ENCR    IAM     AUDIT   (reserved)
```

| Bit | Name | Description |
|-----|------|-------------|
| 0 | COMPRESSION | Compression enabled (LZ4/Zstd) |
| 1 | ENCRYPTION | Encryption enabled (AES-256-GCM) |
| 2 | IAM_POLICY | IAM policies active |
| 3 | AUDIT_LOG | Audit logging enabled |
| 4-31 | (reserved) | Must be 0 |

**Examples:**
```
0b0001 = 1   → Compression only
0b0011 = 3   → Compression + Encryption
0b1111 = 15  → All features enabled
```

#### Validation Rules

**v0.2 Header:**
1. Magic must be `"CART\x00\x02\x00\x00"`
2. version_major must be 2
3. version_minor must be 0-4 (current: 4)
4. block_size must be 4096
5. free_blocks ≤ total_blocks
6. btree_root_page must be 1
7. total_blocks ≥ 3 (header, catalog, allocator minimum)
8. slug must be valid (see Manifest System)
9. reserved bytes must be zero

**Validation Code:**
```rust
fn validate_header(h: &CartridgeHeader) -> Result<()> {
    if h.magic != b"CART\x00\x02\x00\x00" {
        return Err(Error::InvalidMagic);
    }
    if h.version_major != 2 {
        return Err(Error::UnsupportedVersion);
    }
    if h.block_size != 4096 {
        return Err(Error::InvalidBlockSize);
    }
    if h.free_blocks > h.total_blocks {
        return Err(Error::CorruptedHeader);
    }
    if h.total_blocks < 3 {
        return Err(Error::InvalidSize);
    }
    // validate slug format...
    Ok(())
}
```

---

## Page Format

### Page Structure

All pages (except page 0) share a common structure.

**Size:** 4096 bytes

#### Byte Layout

| Offset | Size | Type | Field | Description |
|--------|------|------|-------|-------------|
| 0 | 1 | u8 | page_type | Page type identifier (0-5) |
| 1 | 32 | u8[32] | checksum | SHA-256 checksum (optional) |
| 33 | 1 | u8 | compression | Compression method (0-2) |
| 34 | 4 | u32 | original_size | Uncompressed size (bytes) |
| 38 | 4 | u32 | compressed_size | Compressed size (bytes) |
| 42 | 1 | u8 | encryption | Encryption enabled (0/1) |
| 43 | 21 | u8[21] | reserved | Future use |
| 64 | 4032 | u8[4032] | data | Payload data |

#### Page Types

| Value | Name | Description | Since |
|-------|------|-------------|-------|
| 0 | Header | Archive header (page 0 only) | v0.1 |
| 1 | CatalogBTree | B-tree catalog node | v0.1 |
| 2 | ContentData | File content data | v0.1 |
| 3 | Freelist | Free block tracking | v0.1 |
| 4 | AuditLog | Audit log entries | v0.2 |
| 5 | SnapshotMeta | Snapshot metadata | v0.2 |

#### C Structure

```c
struct Page {
    struct PageHeader {
        uint8_t  page_type;        // 0-5
        uint8_t  checksum[32];     // SHA-256 or zeros
        uint8_t  compression;      // 0=None, 1=LZ4, 2=Zstd
        uint32_t original_size;    // Before compression
        uint32_t compressed_size;  // After compression
        uint8_t  encryption;       // 0=No, 1=Yes
        uint8_t  reserved[21];     // Future use
    } header;  // 64 bytes total

    uint8_t data[4032];            // Payload
};
```

#### Checksum Computation

**Algorithm:** SHA-256
**Input:** Page data (4032 bytes, offset 64-4095)
**Output:** 32-byte hash stored at offset 1-32

**Optional Checksum:**
- All zeros (32 × 0x00): **Skip verification** (performance mode)
- Non-zero: **Verify on read**

**Verification:**
```rust
fn verify_page(page: &Page) -> Result<()> {
    if page.header.checksum != [0u8; 32] {
        let computed = sha256(&page.data);
        if computed != page.header.checksum {
            return Err(Error::ChecksumMismatch);
        }
    }
    Ok(())
}
```

#### Compression Field

| Value | Method | Speed | Ratio | Use Case |
|-------|--------|-------|-------|----------|
| 0 | None | N/A | 1.0x | Already compressed data |
| 1 | LZ4 | 9.77 GiB/s | ~2x | Latency-sensitive |
| 2 | Zstd | 4.87 GiB/s | ~4-5x | Cold storage |

**Decompression Performance:**
- LZ4: 38.12 GiB/s (3.9x faster than compression)
- Zstd: ~5.64 GiB/s

#### Encryption Field

| Value | Description |
|-------|-------------|
| 0 | No encryption |
| 1 | AES-256-GCM encrypted |

**Encrypted Data Format:**
```
[nonce: 12 bytes][ciphertext: 4000 bytes][tag: 16 bytes][padding: 4 bytes]
```

Total: 4032 bytes (fits in page data)

---

## Catalog Structure

### Page 1: B-tree Catalog

**Current Implementation (v0.2.x):** In-memory B-tree serialized to JSON

**Future (v0.3):** Multi-page binary B-tree

#### Current Format (JSON Serialization)

Page 1 contains a JSON-serialized B-tree:

```json
{
  "root_page": 1,
  "entries": {
    "/readme.txt": {
      "file_type": "File",
      "size": 1024,
      "blocks": [3, 4],
      "created": 1700000000000000,
      "modified": 1700000000000000,
      "compressed_size": null,
      "checksum": null
    },
    "/docs/guide.md": {
      "file_type": "File",
      "size": 8192,
      "blocks": [5, 6, 7],
      "created": 1700000000000000,
      "modified": 1700000000000000,
      "compressed_size": 4096,
      "checksum": "a3f8..."
    },
    "/images/": {
      "file_type": "Directory",
      "size": 0,
      "blocks": [],
      "created": 1700000000000000,
      "modified": 1700000000000000,
      "compressed_size": null,
      "checksum": null
    }
  }
}
```

#### FileMetadata Structure

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub file_type: FileType,        // "File" or "Directory"
    pub size: u64,                  // Size in bytes
    pub blocks: Vec<u64>,           // Allocated block IDs
    pub created: u64,               // Created (μs since epoch)
    pub modified: u64,              // Modified (μs since epoch)
    pub compressed_size: Option<u64>, // Compressed size (if applicable)
    pub checksum: Option<String>,   // SHA-256 hex (if enabled)
}
```

#### FileType Values

| Value | Name | Description |
|-------|------|-------------|
| 0 | File | Regular file (has blocks) |
| 1 | Directory | Directory (no blocks, virtual) |

#### Catalog Limitations (v0.2.x)

**Single-Page Catalog:**
- Maximum catalog size: 4032 bytes (after 64-byte page header)
- With compression: ~8,000-12,000 bytes effective capacity
- Approximate file capacity: **10,000-50,000 files** (depends on path lengths)

**When to Upgrade:**
- If catalog exceeds single page → Error: `CatalogFull`
- Solution: Upgrade to v0.3 with multi-page B-tree

#### Catalog Operations

**Lookup (O(log n)):**
```rust
let metadata = catalog.get("/path/to/file.txt")?;
```

**Insert (O(log n)):**
```rust
catalog.insert("/new/file.txt", metadata)?;
```

**Delete (O(log n)):**
```rust
catalog.remove("/path/to/file.txt")?;
```

**List Directory (O(n)):**
```rust
let files = catalog.list_dir("/docs/")?;
// Returns: ["guide.md", "api.md", "faq.md"]
```

---

## Hybrid Allocator

### Overview

Cartridge uses a **hybrid allocator** combining two strategies:

1. **Bitmap Allocator** - For small files (< 256KB)
2. **Extent Allocator** - For large files (≥ 256KB)

### Bitmap Allocator

**Strategy:** Bit array tracking individual blocks

**Structure:**
- Array of u64 values
- Each bit = one block (0 = free, 1 = allocated)
- 64 blocks per u64 entry

**Example:**
```
bitmap[0] = 0b0000_0111  (binary)
          = 7            (decimal)

Blocks 0, 1, 2: allocated (bits set)
Blocks 3-63: free (bits clear)
```

**Allocation Algorithm:**
```rust
fn allocate_bitmap(&mut self, blocks_needed: u64) -> Result<Vec<u64>> {
    let mut allocated = Vec::new();
    for block_id in 0..self.total_blocks {
        if !self.is_allocated(block_id) {
            self.set_allocated(block_id);
            allocated.push(block_id);
            if allocated.len() as u64 == blocks_needed {
                return Ok(allocated);
            }
        }
    }
    Err(Error::NoSpace)
}
```

**Performance:**
- Allocate 100K blocks: 4.15 ms
- Throughput: 24,096 blocks/ms

**Best For:**
- Small files (4KB - 256KB)
- Random allocation patterns
- Fine-grained control

### Extent Allocator

**Strategy:** Track contiguous free regions (extents)

**Structure:**
```rust
struct Extent {
    start: u64,     // Starting block ID
    length: u64,    // Number of contiguous blocks
}

struct ExtentAllocator {
    free_extents: Vec<Extent>,  // Sorted by start
    total_blocks: u64,
}
```

**Free Extents List Example:**
```rust
free_extents = [
    Extent { start: 100, length: 500 },   // Blocks 100-599 free
    Extent { start: 1000, length: 2000 }, // Blocks 1000-2999 free
]
```

**Allocation Algorithm (First-Fit):**
```rust
fn allocate_extent(&mut self, blocks_needed: u64) -> Result<Vec<u64>> {
    for extent in &mut self.free_extents {
        if extent.length >= blocks_needed {
            let allocated: Vec<u64> = (extent.start..(extent.start + blocks_needed)).collect();
            extent.start += blocks_needed;
            extent.length -= blocks_needed;
            return Ok(allocated);
        }
    }
    Err(Error::NoSpace)
}
```

**Coalescing on Free:**
```rust
fn free_extent(&mut self, blocks: &[u64]) {
    let new_extent = Extent {
        start: blocks[0],
        length: blocks.len() as u64,
    };

    // Insert and merge with adjacent extents
    self.free_extents.push(new_extent);
    self.free_extents.sort_by_key(|e| e.start);
    self.coalesce_adjacent();
}
```

**Performance:**
- Allocate 100K blocks: 576 μs
- Throughput: 173,611 blocks/ms
- **301x faster** than bitmap for large allocations

**Best For:**
- Large files (≥ 256KB)
- Contiguous allocation (better disk locality)
- High throughput

### Hybrid Strategy

**Routing Logic:**
```rust
fn allocate(&mut self, size_bytes: u64) -> Result<Vec<u64>> {
    let blocks_needed = (size_bytes + 4095) / 4096;

    if size_bytes < 256 * 1024 {
        // Route to bitmap (small files)
        self.bitmap.allocate(blocks_needed)
    } else {
        // Route to extent (large files)
        self.extent.allocate(blocks_needed)
    }
}
```

**Threshold:**
- **< 256KB:** Bitmap allocator
- **≥ 256KB:** Extent allocator

**Rationale:**
- Small files: Many small allocations → bitmap handles well
- Large files: Contiguous blocks → extent optimizes disk I/O

**Performance Comparison:**

| Allocator | 4KB Latency | 64KB Latency | 256KB Latency | 1MB Latency |
|-----------|-------------|--------------|---------------|-------------|
| Bitmap | 4.99 μs | 11.40 μs | N/A | N/A |
| Extent | N/A | N/A | 7.83 μs | 7.16 μs |
| Hybrid | 4.99 μs | 11.40 μs | 7.83 μs | 7.16 μs |

**Speedup:**
- Hybrid (large) vs Hybrid (small): **16,700x faster** for 1MB files

### Fragmentation

**Fragmentation Score:**
```rust
fn fragmentation_score(&self) -> f64 {
    let ideal_extents = if self.allocated_count == 0 { 0 } else { 1 };
    let actual_extents = self.free_extents.len();
    actual_extents as f64 / ideal_extents.max(1) as f64
}
```

**Interpretation:**
- Score = 1.0: No fragmentation (single free extent)
- Score > 1.0: Fragmentation present
- Score = 10.0: 10 free extents (highly fragmented)

**Defragmentation (Future v0.3):**
- Compact files to beginning of archive
- Merge free extents
- Reduce total_blocks if possible

### Allocator State Serialization

**Page 2 Format (JSON, temporary):**
```json
{
  "bitmap": {
    "bitmap": [0, 0, 7, 255, ...],
    "total_blocks": 10000,
    "allocated_count": 150
  },
  "extent": {
    "free_extents": [
      {"start": 500, "length": 1000},
      {"start": 2000, "length": 5000}
    ],
    "total_blocks": 10000
  },
  "threshold_bytes": 262144
}
```

**Future (v0.3):** Binary format for efficiency

---

## Buffer Pool (ARC)

### Overview

Cartridge uses an **Adaptive Replacement Cache (ARC)** for hot data.

### ARC Algorithm

**Two Lists:**
1. **T1 (Recent):** Recently accessed pages
2. **T2 (Frequent):** Frequently accessed pages

**Adaptive:**
- Dynamically balances between recency and frequency
- Adjusts based on access patterns

**Adaptation Speed:**
- 164 microseconds to adapt to workload shifts (verified)

### Buffer Pool Configuration

**Default Size:** 1,000 pages (4 MB)

**Configurable:**
```rust
use cartridge_rs::CartridgeBuilder;

let cart = CartridgeBuilder::new()
    .slug("my-data")
    .title("My Data")
    .buffer_pool_size(10_000)  // 40 MB
    .build()?;
```

### Performance Characteristics

| Pool Size | Get (hit) | Put | Miss |
|-----------|-----------|-----|------|
| 100 | 20.37 μs | 24.98 μs | 3.26 ns |
| 1,000 | 255.0 μs | 285.7 μs | 3.26 ns |
| 10,000 | 6.10 ms | 6.11 ms | 3.81 ns |

**Scaling:** Near-linear with pool size

### Hit Rates

| Access Pattern | Hit Rate |
|----------------|----------|
| Random Access | ~66% |
| 80/20 Workload | ~90%+ |
| Sequential Scan | ~10% (not optimized for) |

### Access Pattern Performance

| Pattern | Latency (10K pool) | Analysis |
|---------|-------------------|----------|
| Sequential Scan | 7.53 ms | Poor (ARC not optimized for sequential) |
| Random Access | 637 μs | Good (benefits from caching) |
| 80/20 Workload | 1.74 ms | Excellent (ARC adapts to hotspots) |

### Eviction Policy

**When full:**
1. Evict from T1 (recent) if T1 is larger
2. Evict from T2 (frequent) if T2 is larger
3. Move evicted page to ghost list (metadata only)

**Ghost Lists:**
- Track recently evicted pages (metadata only, no data)
- Used to guide future adaptation decisions

### Cache Key

**Format:**
```rust
cache_key = page_id  // u64
```

**Collision:** Not possible (unique page IDs)

### Thread Safety

**Synchronization:**
```rust
struct BufferPool {
    pool: Arc<RwLock<HashMap<u64, Page>>>,
    // ...
}
```

**Concurrent Access:**
- Multiple readers: ✅ Allowed
- Writer blocks readers: ✅ Safe
- Performance: 10,000+ reads/sec/core

---

## Compression Format

### Compressed Page Data

Compression is applied to page data (4032 bytes), not the entire page.

### Compression Methods

| Value | Name | Compress | Decompress | Ratio | Use Case |
|-------|------|----------|------------|-------|----------|
| 0 | None | N/A | N/A | 1.0x | Already compressed |
| 1 | LZ4 | 9.77 GiB/s | 38.12 GiB/s | ~2x | Latency-sensitive |
| 2 | Zstd | 4.87 GiB/s | ~5.64 GiB/s | ~4-5x | Cold storage |

*Performance verified via docs/performance.md (2025-11-20)*

### LZ4 Format

**Compressed Data:**
```
[size_prefix: 4 bytes][compressed_data: N bytes]
```

**Size Prefix:**
- 4-byte little-endian u32
- Contains uncompressed size
- Added by `compress_prepend_size()`

**Total Size:** 4 + compressed_length

**Compression:**
```rust
use lz4_flex::compress_prepend_size;

let compressed = compress_prepend_size(data)?;
// compressed[0..4] = uncompressed size (u32 LE)
// compressed[4..] = LZ4 compressed data
```

**Decompression:**
```rust
use lz4_flex::decompress_size_prepended;

let decompressed = decompress_size_prepended(&compressed)?;
```

### Zstd Format

**Compressed Data:**
```
[zstd_frame]
```

**Frame Header:**
- Zstd includes size information in frame
- No separate size prefix needed

**Compression:**
```rust
use zstd::encode_all;

let compressed = encode_all(data.as_slice(), 3)?;  // level 3
```

**Decompression:**
```rust
use zstd::decode_all;

let decompressed = decode_all(compressed.as_slice())?;
```

### Compression Decision

**Apply compression if:**
1. Data size ≥ threshold
2. Compression ratio < min_ratio

**Thresholds:**

| Algorithm | Min Size | Min Ratio | Rationale |
|-----------|----------|-----------|-----------|
| LZ4 | 512 bytes | 0.9 (10% savings) | Fast, so low threshold |
| Zstd | 1024 bytes | 0.85 (15% savings) | Slower, needs more savings |

**Example:**
```rust
let original_size = data.len();
let compressed = compress_lz4(data);

if compressed.len() < original_size * 0.9 && original_size >= 512 {
    // Store compressed
    page.header.compression = 1;
    page.header.original_size = original_size as u32;
    page.header.compressed_size = compressed.len() as u32;
    page.data[..compressed.len()].copy_from_slice(&compressed);
} else {
    // Store uncompressed
    page.header.compression = 0;
    page.data[..original_size].copy_from_slice(data);
}
```

### Compression Metadata

Stored in page header (offsets 33-42):

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 33 | 1 | compression | Method (0/1/2) |
| 34 | 4 | original_size | Uncompressed size (bytes) |
| 38 | 4 | compressed_size | Compressed size (bytes) |

**If `compression = 0`:**
- `original_size` = actual data size
- `compressed_size` = 0 (ignored)

---

## Encryption Format

### Encrypted Page Data

Encryption is applied to page data (4032 bytes) **after** compression (if enabled).

### Encryption Algorithm

**AES-256-GCM (Galois/Counter Mode)**

**Parameters:**
- **Key Size:** 256 bits (32 bytes)
- **Nonce Size:** 96 bits (12 bytes)
- **Tag Size:** 128 bits (16 bytes)

**Security:**
- AEAD (Authenticated Encryption with Associated Data)
- Prevents tampering and forgery
- Hardware acceleration (AES-NI) on modern CPUs

### Encrypted Data Format

```
┌────────────┬──────────────┬──────────────┬─────────┐
│ Nonce (12) │ Ciphertext │ Auth Tag (16)│ Padding │
│    bytes   │  (variable)  │    bytes     │ (zeros) │
└────────────┴──────────────┴──────────────┴─────────┘
```

**Total:** 4032 bytes (fits in page data)

**Breakdown:**
- Nonce: 12 bytes
- Ciphertext: original_size bytes
- Auth Tag: 16 bytes
- Padding: 4032 - (12 + original_size + 16) bytes (zeros)

**Example (1KB plaintext):**
```
Nonce:      12 bytes
Ciphertext: 1024 bytes
Auth Tag:   16 bytes
Padding:    2980 bytes (zeros)
Total:      4032 bytes
```

### Nonce Generation

**Current (v0.2.x):** Random nonce per encryption

```rust
use aes_gcm::aead::OsRng;
use aes_gcm::aead::rand_core::RngCore;

let mut nonce = [0u8; 12];
OsRng.fill_bytes(&mut nonce);
```

**Future (v0.3):** Deterministic nonce from page_id + counter

```rust
fn derive_nonce(page_id: u64, counter: u32) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[0..8].copy_from_slice(&page_id.to_le_bytes());
    nonce[8..12].copy_from_slice(&counter.to_le_bytes());
    nonce
}
```

**Benefit:** Reduces storage overhead (no need to store nonce)

### Master Key

**Storage:**
- 32-byte master key (NOT stored in cartridge file)
- Provided at runtime:
  - Environment variable: `CARTRIDGE_KEY=hex_string`
  - Config file: `~/.cartridge/keys.toml`
  - Hardware key (YubiKey, TPM)

**Key Generation:**
```rust
use aes_gcm::aead::OsRng;
use aes_gcm::aead::rand_core::RngCore;

let mut key = [0u8; 32];
OsRng.fill_bytes(&mut key);
println!("Master key (hex): {}", hex::encode(key));
```

**Security:**
- ⚠️ **Never commit keys to git**
- ⚠️ **Never hardcode keys in source**
- ✅ Use environment variables or hardware keys
- ✅ Rotate keys periodically

### Encryption/Decryption

**Encryption:**
```rust
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use aes_gcm::aead::Aead;

fn encrypt_page(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(key.into());

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher.encrypt(nonce, data)?;

    // Format: [nonce][ciphertext+tag]
    let mut encrypted = Vec::with_capacity(12 + ciphertext.len());
    encrypted.extend_from_slice(&nonce_bytes);
    encrypted.extend_from_slice(&ciphertext);

    Ok(encrypted)
}
```

**Decryption:**
```rust
fn decrypt_page(encrypted: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    if encrypted.len() < 12 + 16 {
        return Err(Error::InvalidEncryptedData);
    }

    let cipher = Aes256Gcm::new(key.into());

    let nonce = Nonce::from_slice(&encrypted[0..12]);
    let ciphertext = &encrypted[12..];

    let plaintext = cipher.decrypt(nonce, ciphertext)?;

    Ok(plaintext)
}
```

### Authentication Tag

**Purpose:**
- Verifies ciphertext integrity
- Detects tampering or corruption
- Prevents forgery attacks

**Verification:**
- Automatically checked during decryption
- Decryption failure indicates:
  1. Wrong key
  2. Corrupted ciphertext
  3. Tampered data

**Handling Decryption Failures:**
```rust
match decrypt_page(encrypted, key) {
    Ok(plaintext) => { /* success */ }
    Err(aes_gcm::Error) => {
        // Wrong key, corrupted data, or tampered
        return Err(CartridgeError::DecryptionFailed);
    }
}
```

### Encryption Performance

**Overhead:**
- Encryption: Minimal (hardware AES acceleration)
- Decryption: Minimal
- Storage: 28 bytes per page (12 nonce + 16 tag)

**Future Optimization (v0.3):**
- Deterministic nonces → Save 12 bytes per page
- Per-file keys (derived from master) → Better key isolation

---

## IAM Policy Format

### Overview

IAM (Identity and Access Management) policies control access to files.

**Inspiration:** AWS IAM policies (S3-style)

### Policy Structure

**JSON Schema:**
```json
{
  "version": "2012-10-17",
  "id": "read-only-policy",
  "statements": [
    {
      "sid": "AllowReadPublic",
      "effect": "Allow",
      "actions": ["Read", "List"],
      "resources": ["/public/**"],
      "conditions": null
    },
    {
      "sid": "DenyWriteSecret",
      "effect": "Deny",
      "actions": ["Write", "Delete"],
      "resources": ["/secret/**"],
      "conditions": null
    }
  ]
}
```

### Effect Values

| Value | Description | Precedence |
|-------|-------------|------------|
| Allow | Grant access | Low |
| Deny | Deny access | **High** (overrides Allow) |

**Evaluation Order:**
1. Check all Deny statements first
2. If any Deny matches → **Deny access**
3. Otherwise, check Allow statements
4. If any Allow matches → **Allow access**
5. Otherwise → **Deny by default**

### Action Values

| Value | Description | Maps To |
|-------|-------------|---------|
| Read | Read file content | `cart.read()` |
| Write | Modify file content | `cart.write()`, `cart.update()` |
| Create | Create new files | `cart.create_file()` |
| Delete | Delete files | `cart.delete()` |
| List | List directory contents | `cart.list()` |
| All | All actions (wildcard) | * |

### Resource Patterns

**Wildcards:**
- `*` - Match single path segment
- `**` - Match multiple segments (recursive)

**Examples:**
```
/config.json         → Exact match
/docs/*.md           → All markdown files in /docs
/docs/**             → All files under /docs (recursive)
/data/**/backup.db   → backup.db anywhere under /data
/**                  → All files (global)
```

**Matching Algorithm:**
```rust
fn matches_pattern(path: &str, pattern: &str) -> bool {
    let path_parts: Vec<&str> = path.split('/').collect();
    let pattern_parts: Vec<&str> = pattern.split('/').collect();

    match_parts(&path_parts, &pattern_parts)
}

fn match_parts(path: &[&str], pattern: &[&str]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }

    if pattern[0] == "**" {
        // Match zero or more segments
        match_parts(path, &pattern[1..])  // Skip this segment
            || (!path.is_empty() && match_parts(&path[1..], pattern))  // Consume path segment
    } else if !path.is_empty() && (pattern[0] == "*" || pattern[0] == path[0]) {
        // Match single segment
        match_parts(&path[1..], &pattern[1..])
    } else {
        false
    }
}
```

### Condition Structure (Optional)

**Operators:**

| Operator | Type | Example |
|----------|------|---------|
| StringEquals | String | `path == "/admin/config.json"` |
| StringNotEquals | String | `path != "/tmp/**"` |
| NumericEquals | Number | `file_size == 1024` |
| NumericLessThan | Number | `file_size < 1048576` |
| NumericGreaterThan | Number | `file_size > 0` |
| DateBefore | Timestamp | `timestamp < 1700000000000000` |
| DateAfter | Timestamp | `timestamp > 1700000000000000` |

**Example:**
```json
{
  "effect": "Deny",
  "actions": ["Write"],
  "resources": ["/logs/**"],
  "conditions": {
    "NumericGreaterThan": {
      "file_size": 104857600
    }
  }
}
```

**Interpretation:** Deny writes to /logs/** if file size > 100 MB

### Policy Evaluation

**Algorithm:**
```rust
fn evaluate_policy(policy: &Policy, action: Action, resource: &str) -> Decision {
    // Step 1: Check Deny statements first
    for statement in &policy.statements {
        if statement.effect == Effect::Deny {
            if matches_statement(statement, action, resource) {
                return Decision::Deny;
            }
        }
    }

    // Step 2: Check Allow statements
    for statement in &policy.statements {
        if statement.effect == Effect::Allow {
            if matches_statement(statement, action, resource) {
                return Decision::Allow;
            }
        }
    }

    // Step 3: Default deny
    Decision::Deny
}

fn matches_statement(stmt: &Statement, action: Action, resource: &str) -> bool {
    let action_matches = stmt.actions.contains(&action) || stmt.actions.contains(&Action::All);
    let resource_matches = stmt.resources.iter().any(|pattern| matches_pattern(resource, pattern));

    action_matches && resource_matches
}
```

### Policy Performance

**Cached Evaluation:**

| Operation | Latency | Throughput |
|-----------|---------|------------|
| Evaluate (cached) | 5 μs | 1,000,000+ evals/sec |
| Evaluate (uncached) | 50 μs | 50,000+ evals/sec |
| Pattern Compile | N/A | N/A (done at load time) |

**Cache Key:**
```rust
cache_key = (action, resource_path)
```

**Cache Size:** 1,000 entries (LRU)

### IAM Storage

**Location:** NOT stored in cartridge file (stored separately)

**Options:**
1. **External file:** `/path/to/policies/my-data.policy.json`
2. **Engram manifest:** Included in `manifest.toml` when frozen
3. **Runtime:** Loaded via API

**Example:**
```rust
use cartridge_rs::Policy;

let policy_json = std::fs::read_to_string("policy.json")?;
let policy: Policy = serde_json::from_str(&policy_json)?;

cart.set_policy(policy);
```

---

## Snapshot Format

### Overview

Snapshots create **point-in-time backups** of cartridges.

**Strategy:** Copy-on-Write (CoW)

### Snapshot Directory Structure

```
snapshots/
├─ snapshot_1700000000000000/
│  ├─ metadata.json
│  └─ pages.bin
├─ snapshot_1700000001000000/
│  ├─ metadata.json
│  └─ pages.bin
└─ ...
```

### Snapshot Metadata (metadata.json)

```json
{
  "id": 1700000000000000,
  "name": "v1",
  "description": "First version before major refactor",
  "created_at": 1700000000000000,
  "parent_path": "/path/to/cartridge.cart",
  "header": {
    "magic": [67, 65, 82, 84, 0, 2, 0, 0],
    "version_major": 2,
    "version_minor": 4,
    "block_size": 4096,
    "total_blocks": 10000,
    "free_blocks": 9850,
    "btree_root_page": 1,
    "slug": "my-data",
    "title": "My Data Container",
    "created_timestamp": 1699900000000000,
    "modified_timestamp": 1700000000000000,
    "growth_count": 5,
    "growth_threshold_percent": 10,
    "flags": 15,
    "reserved": [0, 0, ...]
  },
  "modified_pages": [1, 2, 3, 5, 7],
  "size_bytes": 20480
}
```

### Snapshot Pages (pages.bin)

**Binary Format:**
```
[page_count: u64]
[page_entry_1]
[page_entry_2]
...
[page_entry_N]
```

**Page Entry:**
```
[page_id: u64][page_size: u64][page_data: u8[page_size]]
```

**Example (2 pages):**
```
Offset 0: page_count = 2 (8 bytes, little-endian)

Offset 8:  page_entry_1
           ├─ page_id = 1 (8 bytes)
           ├─ page_size = 4096 (8 bytes)
           └─ page_data = [4096 bytes of page 1 data]

Offset 4120: page_entry_2
             ├─ page_id = 2 (8 bytes)
             ├─ page_size = 4096 (8 bytes)
             └─ page_data = [4096 bytes of page 2 data]

Total: 8 + (8 + 8 + 4096) * 2 = 8232 bytes
```

### Snapshot Creation

**API:**
```rust
let snapshot_id = cart.create_snapshot("v1", "Before refactor")?;
println!("Snapshot ID: {}", snapshot_id);
// Snapshot ID: 1700000000000000
```

**Process:**
1. Flush all dirty pages to disk
2. Create snapshot directory (`snapshots/snapshot_{id}/`)
3. Copy modified pages to `pages.bin`
4. Save header and metadata to `metadata.json`
5. Return snapshot ID

**Copy-on-Write:**
- Only modified pages are copied (not entire cartridge)
- Unchanged pages: shared with original
- Space savings: 90%+ for typical edits

### Snapshot Restoration

**API:**
```rust
cart.restore_snapshot(1700000000000000)?;
println!("Restored to snapshot v1");
```

**Process:**
1. Load snapshot metadata
2. Verify snapshot exists and is valid
3. Restore header from snapshot
4. Restore modified pages from `pages.bin`
5. Flush changes to disk

**Destructive:**
- Current state is lost (unless snapshotted first)
- Recommendation: Create snapshot before restoring

### Snapshot Listing

**API:**
```rust
let snapshots = cart.list_snapshots()?;
for snapshot in snapshots {
    println!("{}: {} ({})", snapshot.id, snapshot.name, snapshot.description);
}
```

**Output:**
```
1700000000000000: v1 (Before refactor)
1700000001000000: v2 (After refactor)
1700000002000000: v3 (Production release)
```

### Snapshot Deletion

**API:**
```rust
cart.delete_snapshot(1700000000000000)?;
println!("Deleted snapshot v1");
```

**Process:**
1. Delete snapshot directory
2. Remove metadata and pages files
3. Update snapshot index

### Snapshot Performance

**Benchmark (10 files, 1KB each):**

| Operation | Latency | Throughput |
|-----------|---------|------------|
| Create Snapshot | ~5 ms | 200 snapshots/sec |
| Restore Snapshot | ~8 ms | 125 restores/sec |
| List Snapshots | ~100 μs | 10,000 lists/sec |
| Delete Snapshot | ~2 ms | 500 deletes/sec |

**Storage Overhead:**
- Metadata: ~2-5 KB per snapshot
- Pages: 4096 bytes × (number of modified pages)
- Typical: 10-50 KB per snapshot (for small edits)

---

## SQLite VFS Integration

### Overview

Cartridge implements a **SQLite Virtual File System (VFS)** allowing SQLite databases to run **inside** cartridges.

**Benefits:**
- Single-file distribution (database + files together)
- Offline-first (no network dependencies)
- Compression and encryption for database files
- Immutable freezing (freeze database + files together)

### VFS Architecture

```
SQLite
  ↓
VFS Layer (cartridge_vfs)
  ↓
Cartridge Storage
  ↓
.cart File
```

**Mapping:**
- SQLite database file → Cartridge internal path
- Journal/WAL files → Separate paths in cartridge
- Temp files → In-memory or cartridge paths

### VFS Registration

**API:**
```rust
use rusqlite::{Connection, OpenFlags};
use cartridge_rs::vfs::register_vfs;

// Register VFS globally
register_vfs()?;

// Open database inside cartridge
let conn = Connection::open_with_flags(
    "file:mydb.db?vfs=cartridge&cartridge=my-data.cart",
    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
)?;

conn.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)", [])?;
```

**URI Format:**
```
file:{db_path}?vfs=cartridge&cartridge={cartridge_path}
```

**Parameters:**
- `db_path`: Path to database inside cartridge (e.g., `/db/mydb.db`)
- `cartridge`: Path to cartridge file (e.g., `my-data.cart`)

### VFS Operations

**Implemented Operations:**

| Operation | SQLite xMethod | Cartridge Mapping |
|-----------|---------------|-------------------|
| Open | `xOpen` | `cart.create_file()` or `cart.open()` |
| Close | `xClose` | Flush buffers |
| Read | `xRead` | `cart.read()` with offset/length |
| Write | `xWrite` | `cart.write()` with offset/length |
| Truncate | `xTruncate` | Resize file |
| Sync | `xSync` | Flush to disk |
| File Size | `xFileSize` | `cart.metadata().size` |
| Lock | `xLock` | In-memory lock (single process) |
| Unlock | `xUnlock` | Release lock |
| Check Reserved Lock | `xCheckReservedLock` | Check lock state |
| File Control | `xFileControl` | Pass-through |
| Sector Size | `xSectorSize` | Return 4096 |
| Device Characteristics | `xDeviceCharacteristics` | Return flags |

**Unsupported (Future):**
- Memory-mapped I/O (`xShmMap`, `xShmLock`) - Future v0.3
- Shared memory - Future (multi-process support)

### VFS Performance

**Throughput:**

| Operation | Cartridge VFS | Native Filesystem | Ratio |
|-----------|---------------|-------------------|-------|
| Read (cached) | ~18 GiB/s | ~20 GiB/s | 0.9x |
| Write | ~9 GiB/s | ~10 GiB/s | 0.9x |
| Latency (4KB) | ~280 ns | ~200 ns | 1.4x |

**Conclusion:** Cartridge VFS achieves ~90% of native filesystem performance.

### Concurrency

**Current (v0.2.x):** Single-process only

**Locking:**
- In-memory lock table
- SQLite locking protocol supported
- No inter-process locking (IPC)

**Future (v0.3):** Multi-process support via:
- File-based locking (`flock`, `LockFileEx`)
- Shared memory for lock coordination

### Example: SQLite in Cartridge

**Full Example:**
```rust
use rusqlite::{Connection, OpenFlags};
use cartridge_rs::{Cartridge, vfs::register_vfs};

fn main() -> anyhow::Result<()> {
    // Create cartridge
    let mut cart = Cartridge::create("my-data", "My Data")?;

    // Register VFS
    register_vfs()?;

    // Open database inside cartridge
    let conn = Connection::open_with_flags(
        "file:/db/users.db?vfs=cartridge&cartridge=my-data.cart",
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )?;

    // Create table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT, email TEXT)",
        [],
    )?;

    // Insert data
    conn.execute(
        "INSERT INTO users (name, email) VALUES (?1, ?2)",
        ["Alice", "alice@example.com"],
    )?;

    // Query data
    let mut stmt = conn.prepare("SELECT id, name, email FROM users")?;
    let users = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
    })?;

    for user in users {
        let (id, name, email) = user?;
        println!("{}: {} <{}>", id, name, email);
    }

    Ok(())
}
```

### VFS Safety

**Unsafe Code:**
- VFS implementation uses FFI (C interface to SQLite)
- 29 unsafe blocks in `src/vfs/*.rs` (see TESTING_PLAN.md)

**Testing (Critical):**
- Fuzzing required for VFS operations (see TESTING_PLAN.md Phase 1)
- Crash recovery testing
- Concurrent access validation

---

## Engram Freezing

### Overview

**Engram Freezing** converts a mutable Cartridge container into an **immutable, cryptographically signed Engram archive**.

**Use Case:**
- Distribute read-only datasets
- Create verifiable backups
- Compliance (tamper-proof records)

### Engram Format

**Specification:** See `engram-rs/SPECIFICATION.md`

**Key Differences:**

| Feature | Cartridge | Engram |
|---------|-----------|--------|
| Mutability | Mutable | Immutable |
| Signatures | None | Ed25519 signatures |
| Compression | Optional | Required (LZ4 or Zstd) |
| Auto-Growth | Yes | No (fixed size) |
| Format | `.cart` | `.eng` |

### Freezing API

**Basic Freezing:**
```rust
use cartridge_rs::engram_integration::EngramFreezer;

let cart = Cartridge::open("my-data.cart")?;

let freezer = EngramFreezer::new_default(
    "my-data".to_string(),
    "1.0.0".to_string(),
    "Dataset v1".to_string(),
);

freezer.freeze(&cart, "my-data.eng")?;
```

**With Custom Manifest:**
```rust
use cartridge_rs::engram_integration::EngramFreezer;
use engram_rs::Manifest;

let manifest = Manifest {
    slug: "my-data".to_string(),
    version: "2.0.0".to_string(),
    title: "My Data Archive v2".to_string(),
    description: Some("Production dataset".to_string()),
    author: Some("Alice <alice@example.com>".to_string()),
    license: Some("MIT".to_string()),
    created: chrono::Utc::now(),
    ..Default::default()
};

let freezer = EngramFreezer::new(manifest);
freezer.freeze(&cart, "my-data-v2.eng")?;
```

### Freeze Process

**Steps:**
1. Read all files from cartridge
2. Compress each file (LZ4 or Zstd, based on settings)
3. Build Engram central directory
4. Compute SHA-256 checksums
5. Sign manifest with Ed25519 private key
6. Write Engram archive to disk

**Compression:**
- LZ4: Fast (9.77 GiB/s), moderate ratio (~2x)
- Zstd: Slower (4.87 GiB/s), better ratio (~4-5x)

### Freeze Performance

**Benchmark (1000 files, 512 bytes each):**

| File Count | File Size | Total Size | Freeze Time | Throughput |
|------------|-----------|------------|-------------|------------|
| 10 | 1 KB | 10 KB | ~5 ms | 2 MB/s |
| 100 | 1 KB | 100 KB | ~15 ms | 6.7 MB/s |
| 1000 | 512 B | 512 KB | ~50 ms | 10 MB/s |

**Note:** Dominated by compression overhead, not I/O

### Signature Verification

**Engram archives include:**
- Manifest (metadata + signature)
- Central directory (file list + checksums)
- File data (compressed)

**Verification:**
```rust
use engram_rs::Engram;

let engram = Engram::open("my-data.eng")?;
let manifest = engram.verify_manifest()?;  // Verifies Ed25519 signature
println!("Verified: {} v{}", manifest.slug, manifest.version);
```

### Use Cases

**1. Dataset Distribution:**
```
my-dataset.cart (mutable, 10 GB)
  ↓ freeze
my-dataset.eng (immutable, 6 GB compressed, signed)
  ↓ distribute
Users verify signature and extract
```

**2. Compliance Archives:**
```
audit-logs-2025.cart (mutable)
  ↓ freeze (end of year)
audit-logs-2025.eng (immutable, signed, tamper-proof)
```

**3. Snapshot + Freeze:**
```
project.cart
  ↓ snapshot ("v1.0")
  ↓ freeze
project-v1.0.eng (immutable release)
```

---

## Audit Logging

### Overview

**Audit Logging** tracks all operations on a cartridge for compliance and debugging.

**Since:** v0.2.0

### Audit Log Structure

**Location:** Separate pages in cartridge (Page Type: AuditLog = 4)

**Entry Format:**
```rust
struct AuditLogEntry {
    timestamp: u64,        // Microseconds since epoch
    operation: Operation,  // Read, Write, Delete, etc.
    path: String,          // File path
    result: Result,        // Success, Error
    metadata: String,      // JSON metadata (user, IP, etc.)
}
```

### Audit Log Page

**Format:**
```
Page Type: AuditLog (4)
Data: [JSON array of AuditLogEntry]
```

**Example:**
```json
[
  {
    "timestamp": 1700000000000000,
    "operation": "Write",
    "path": "/config.json",
    "result": "Success",
    "metadata": "{\"user\": \"alice\", \"ip\": \"192.168.1.100\"}"
  },
  {
    "timestamp": 1700000001000000,
    "operation": "Delete",
    "path": "/temp.txt",
    "result": "Success",
    "metadata": "{\"user\": \"bob\"}"
  },
  {
    "timestamp": 1700000002000000,
    "operation": "Read",
    "path": "/secret.key",
    "result": "Error: PermissionDenied",
    "metadata": "{\"user\": \"eve\", \"ip\": \"203.0.113.45\"}"
  }
]
```

### Enable Audit Logging

**API:**
```rust
use cartridge_rs::CartridgeBuilder;

let cart = CartridgeBuilder::new()
    .slug("my-data")
    .title("My Data")
    .with_audit_logging()
    .build()?;
```

**Feature Flag:**
- Header flags bit 3 = AUDIT_LOG

### Audit Log API

**Query Logs:**
```rust
let logs = cart.audit_logs()?;
for log in logs {
    println!("{}: {} {} -> {}",
        log.timestamp, log.operation, log.path, log.result);
}
```

**Filter by Date:**
```rust
let start = 1700000000000000;
let end = 1700100000000000;
let logs = cart.audit_logs_range(start, end)?;
```

**Filter by Operation:**
```rust
let writes = cart.audit_logs_by_operation(Operation::Write)?;
```

### Audit Log Performance

**Overhead:**
- Write operation: +1-2% latency
- Async logging: Minimal impact
- Throughput: 20,000+ logs/sec (verified in ARCHITECTURE.md)

**Storage:**
- ~100-200 bytes per log entry (JSON)
- 10,000 entries ≈ 1-2 MB
- Rotation: Optional (future feature)

### Audit Log Rotation (Future v0.3)

**Strategy:** Rotate when log page is full

**Options:**
1. **Archive:** Move old logs to separate archive
2. **Delete:** Delete oldest logs (keep last N entries)
3. **Freeze:** Freeze logs to Engram archive

---

## Performance Characteristics

### Verified Performance (from benchmarks)

**File I/O Throughput (64KB blocks):**

| Operation | Mean | Upper Bound | Latency (P50) |
|-----------|------|-------------|---------------|
| Read | 17.91 GiB/s | 18.38 GiB/s | 3.41 μs |
| Write | 9.41 GiB/s | 9.59 GiB/s | 6.48 μs |

*Source: docs/performance.md (2025-11-20), verified in PERFORMANCE_VERIFICATION.md*

**Compression Performance (64KB blocks):**

| Algorithm | Compress | Decompress | Ratio |
|-----------|----------|------------|-------|
| LZ4 | 9.77 GiB/s | 38.12 GiB/s | ~2x |
| Zstd | 4.87 GiB/s | ~5.64 GiB/s | ~4-5x |

*Source: docs/performance.md, verified*

**Allocation Performance:**

| Allocator | Operation | Latency | Throughput |
|-----------|-----------|---------|------------|
| Bitmap | 100K blocks | 4.15 ms | 24,096 blocks/ms |
| Extent | 100K blocks | 576 μs | 173,611 blocks/ms |
| Hybrid (small) | 100 allocations | 4.99 μs/allocation | N/A |
| Hybrid (large) | 100 allocations | 7.16 μs/allocation | N/A |

*Source: docs/performance.md*

**Buffer Pool (ARC) Performance:**

| Pool Size | Get (hit) | Put | Miss |
|-----------|-----------|-----|------|
| 100 | 20.37 μs | 24.98 μs | 3.26 ns |
| 1,000 | 255.0 μs | 285.7 μs | 3.26 ns |
| 10,000 | 6.10 ms | 6.11 ms | 3.81 ns |

*Source: docs/performance.md*

### Unverified Claims

**Auto-Growth Overhead:**
- **Claimed:** < 1ms per doubling (README.md)
- **Status:** ❌ No benchmark exists (see PERFORMANCE_VERIFICATION.md)
- **Recommendation:** Add `benches/auto_growth_performance.rs`

### Scalability

**File Count:**
- Maximum (v0.2.x): 10,000-50,000 files (single-page catalog limit)
- Recommended: < 10,000 files for optimal performance
- Future (v0.3): Millions (multi-page B-tree)

**Cartridge Size:**
- Minimum: 12 KB (3 blocks)
- Maximum: 2^64 blocks = 18.4 exabytes (theoretical)
- Practical: Limited by filesystem (e.g., ext4: 16 TB)

**Concurrency:**
- Readers: Multiple simultaneous (RwLock)
- Writers: Exclusive access (RwLock)
- Performance: 10,000+ reads/sec/core (with ARC cache)

---

## Version History

### v0.2.4 (Current)

**Release Date:** 2025-12-24
**Status:** Production

**Features:**
- Auto-growth containers (3 → 6 → 12 → ... blocks)
- Slug/title manifest system
- All v0.1 features (compression, encryption, IAM, snapshots)
- SQLite VFS integration
- Engram freezing
- Audit logging (optional)
- Hybrid allocator (Bitmap + Extent)
- ARC buffer pool

**Performance:**
- Read: 17.91 GiB/s (verified)
- Write: 9.41 GiB/s (verified)
- LZ4 Compression: 9.77 GiB/s (verified)
- LZ4 Decompression: 38.12 GiB/s (verified)

**Tests:** 242 tests passing
**Benchmarks:** 8 benchmark suites

### v0.2.0 - v0.2.3

**Release Dates:** 2025-11-20 through 2025-12-15
**Status:** Deprecated (use v0.2.4)

**Changes:**
- Iterative improvements to auto-growth
- Bug fixes in manifest system
- Performance optimizations

### v0.1.0

**Release Date:** 2025-11-20
**Status:** Legacy (superseded by v0.2)

**Features:**
- Fixed-size containers (no auto-growth)
- No slug/title (unnamed cartridges)
- Basic compression, encryption, IAM, snapshots
- Single-page catalog (JSON serialization)
- Bitmap allocator only

**Limitations:**
- Required upfront capacity planning
- Catalog limited to ~10 files
- No SQLite VFS
- No Engram freezing

### v0.3.0 (Planned)

**Target:** Q2 2026

**Planned Features:**
- **Multi-page B-tree catalog** (scale to millions of files)
- **Binary serialization** (replace JSON)
- **WAL (Write-Ahead Log)** for crash recovery
- **Defragmentation and compaction**
- **Incremental snapshots** (delta compression)
- **MVCC** for concurrent access
- **Multi-process support** (shared memory locks)

**Breaking Changes:**
- Catalog format (v0.2 → v0.3 migration required)
- Allocator format (binary instead of JSON)

---

## Compatibility & Migration

### Forward Compatibility

**v0.2 readers must:**
1. Ignore unknown reserved bytes (treat as zeros)
2. Check `version_major` for compatibility
3. Support older `version_minor` within same major version

**Example:**
```rust
fn is_compatible(header: &CartridgeHeader) -> bool {
    header.version_major == 2 && header.version_minor <= 4
}
```

### Backward Compatibility

**v0.2.4 can read:**
- ✅ v0.2.0, v0.2.1, v0.2.2, v0.2.3 (full compatibility)
- ⚠️ v0.1.0 (read-only, upgrade required for write)

**v0.1 → v0.2 Differences:**
- Magic byte 5: `0x01` → `0x02`
- Slug/title fields added (offsets 40-551)
- Growth metadata added (offsets 552-583)

### Migration Path

**v0.1 → v0.2:**

**Step 1: Detect Version**
```rust
fn detect_version(path: &str) -> Result<u8> {
    let mut file = File::open(path)?;
    let mut magic = [0u8; 8];
    file.read_exact(&mut magic)?;

    if &magic[0..4] != b"CART" {
        return Err(Error::InvalidMagic);
    }

    Ok(magic[5])  // Version: 0x01 or 0x02
}
```

**Step 2: Migrate**
```rust
use cartridge_rs::migration::migrate_v1_to_v2;

migrate_v1_to_v2("old.cart", "new.cart", "my-data", "My Data")?;
```

**Migration Process:**
1. Read v0.1 file (all pages)
2. Parse JSON catalog and allocator
3. Create v0.2 header with slug/title
4. Write v0.2 file
5. Verify migration (compare file contents)

**Tool:**
```bash
cargo install cartridge-migrate
cartridge-migrate --input v1.cart --output v2.cart --slug my-data --title "My Data"
```

### Cross-Platform Compatibility

**Supported Platforms:**
- ✅ Windows (NTFS, ReFS)
- ✅ Linux (ext4, btrfs, xfs)
- ✅ macOS (APFS, HFS+)
- ✅ BSD (UFS, ZFS)

**Byte Order:**
- Little-endian (x86, x86_64, ARM, AArch64)
- No support for big-endian (PowerPC, SPARC) - conversion required

**Path Separators:**
- Internal: Unix-style forward slashes (`/dir/file.txt`)
- External (OS): Converted automatically (`C:\dir\file.txt` on Windows)

---

## Appendix A: Binary Format Examples

### Example 1: Minimal Cartridge (v0.2)

**Header (Page 0):**
```
00000000: 43 41 52 54 00 02 00 00  02 00 04 00 00 10 00 00  CART............
00000010: 03 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  ................
00000020: 01 00 00 00 00 00 00 00  6D 79 2D 64 61 74 61 00  ........my-data.
...
00000128: 4D 79 20 44 61 74 61 00  00 00 00 00 00 00 00 00  My Data.........
```

**Decoded:**
- Magic: "CART\x00\x02\x00\x00" (v0.2)
- Version: 2.4
- Block size: 4096
- Total blocks: 3 (minimum)
- Free blocks: 0 (all reserved)
- B-tree root: page 1
- Slug: "my-data"
- Title: "My Data"

### Example 2: Content Page (Compressed)

**Page 3:**
```
00000000: 02 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  ................
00000010: 00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  ................
00000020: 01 00 00 04 00 00 00 06  00 00 00 00 00 00 00 00  ................
00000040: 0D 00 00 00 48 65 6C 6C  6F 2C 20 57 6F 72 6C 64  ....Hello, World
00000050: 21 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  !...............
```

**Decoded:**
- Page type: 2 (ContentData)
- Checksum: all zeros (disabled)
- Compression: 1 (LZ4)
- Original size: 1024 bytes
- Compressed size: 1542 bytes
- Data: LZ4 compressed "Hello, World!" + padding

---

## Appendix B: File Format Checklist

### Implementation Checklist

For implementing a Cartridge v0.2 reader/writer:

**Header (v0.2):**
- [ ] Verify magic number `"CART\x00\x02\x00\x00"`
- [ ] Check version compatibility (major=2, minor≤4)
- [ ] Validate block_size == 4096
- [ ] Validate free_blocks ≤ total_blocks
- [ ] Validate total_blocks ≥ 3
- [ ] Read btree_root_page (always 1)
- [ ] Read slug and title (validate slug format)
- [ ] Read timestamps, growth_count, flags

**Pages:**
- [ ] Read/write 4KB pages
- [ ] Parse page headers (type, checksum, compression, encryption)
- [ ] Compute/verify SHA-256 checksums (if enabled)
- [ ] Decompress LZ4/Zstd data (if compressed)
- [ ] Decrypt AES-256-GCM data (if encrypted)

**Catalog:**
- [ ] Deserialize B-tree from page 1 (JSON for v0.2)
- [ ] Lookup files by path (O(log n))
- [ ] Insert/update/delete entries
- [ ] Handle directories (virtual, no blocks)

**Allocator:**
- [ ] Deserialize hybrid allocator from page 2 (JSON for v0.2)
- [ ] Allocate blocks (bitmap for <256KB, extent for ≥256KB)
- [ ] Free blocks (update bitmap and extents)
- [ ] Track free_blocks count

**Auto-Growth:**
- [ ] Detect low space (free_blocks < threshold)
- [ ] Double total_blocks
- [ ] Expand file with `set_len()`
- [ ] Update header and allocator

**Optional Features:**
- [ ] Compression (LZ4/Zstd)
- [ ] Encryption (AES-256-GCM)
- [ ] IAM policy evaluation
- [ ] Snapshots (create/restore/list/delete)
- [ ] SQLite VFS integration
- [ ] Engram freezing
- [ ] Audit logging

---

## Appendix C: References

**Related Specifications:**
- Engram Format: `engram-rs/SPECIFICATION.md`
- Cartridge Architecture: `docs/ARCHITECTURE.md`
- Performance Benchmarks: `docs/performance.md`
- Testing Plan: `TESTING_PLAN.md`
- Performance Verification: `PERFORMANCE_VERIFICATION.md`

**External Standards:**
- SQLite VFS API: https://www.sqlite.org/vfs.html
- AES-256-GCM: NIST SP 800-38D
- Ed25519 Signatures: RFC 8032
- LZ4 Compression: https://github.com/lz4/lz4
- Zstd Compression: https://facebook.github.io/zstd/

**Source Code:**
- GitHub: https://github.com/blackfall-labs/cartridge
- Crates.io: https://crates.io/crates/cartridge-rs
- Documentation: https://docs.rs/cartridge-rs

---

**End of Specification**

**Document Version:** 2.0
**Format Version:** 0.2.4
**Last Updated:** 2025-12-24
**Status:** Production Specification
