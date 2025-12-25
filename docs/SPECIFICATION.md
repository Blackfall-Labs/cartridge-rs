# Cartridge Format Specification v0.1

**Document Version:** 1.0
**Format Version:** 0.1.0
**Last Updated:** 2025-11-20
**Status:** Production Specification

---

## Table of Contents

1. [Introduction](#introduction)
2. [File Format Overview](#file-format-overview)
3. [Header Structure](#header-structure)
4. [Page Format](#page-format)
5. [Catalog Structure](#catalog-structure)
6. [Allocation Metadata](#allocation-metadata)
7. [Compression Format](#compression-format)
8. [Encryption Format](#encryption-format)
9. [IAM Policy Format](#iam-policy-format)
10. [Snapshot Format](#snapshot-format)
11. [Version History](#version-history)
12. [Compatibility](#compatibility)

---

## Introduction

### Purpose

This document defines the binary format for Cartridge archives (`.cart` files). The format is designed for:

- Embedded systems with limited resources (Raspberry Pi 5)
- Mutable archive workspaces with freeze-to-immutable capability
- High-performance file I/O with optional compression and encryption
- SQLite VFS integration

### Design Goals

1. **Fixed-size pages:** All I/O in 4KB units for filesystem alignment
2. **Platform independence:** Little-endian byte order, explicit sizes
3. **Forward compatibility:** Reserved fields for future extensions
4. **Verifiability:** Optional SHA-256 checksums for integrity
5. **Efficiency:** Minimal overhead, fast lookups, cache-friendly

### Conventions

- All multi-byte integers are **little-endian**
- All offsets are in **bytes** unless otherwise specified
- Page IDs start at **0** (header page)
- Block IDs and page IDs are synonymous
- Sizes are in **bytes** unless otherwise specified

---

## File Format Overview

### High-Level Structure

```
┌─────────────────────────────────────────────┐
│  Page 0: Header (4096 bytes)                │
│  - Magic number, version, block counts      │
│  - Pointers to catalog and allocator        │
├─────────────────────────────────────────────┤
│  Page 1: Catalog B-tree (4096 bytes)        │
│  - Serialized B-tree (JSON, temporary)      │
│  - Path → metadata mappings                 │
├─────────────────────────────────────────────┤
│  Page 2: Allocator State (4096 bytes)       │
│  - Serialized allocator (JSON, temporary)   │
│  - Bitmap and extent data                   │
├─────────────────────────────────────────────┤
│  Page 3+: Content Data                      │
│  - File contents (4032 bytes per page)      │
│  - Freelist pages                           │
│  - Audit log pages                          │
└─────────────────────────────────────────────┘
```

### Reserved Pages

| Page ID | Purpose                         | Fixed     |
| ------- | ------------------------------- | --------- |
| 0       | Header                          | ✅ Always |
| 1       | Catalog B-tree root             | ✅ Always |
| 2       | Allocator state                 | ✅ Always |
| 3+      | Content data / freelist / audit | Dynamic   |

---

## Header Structure

### Page 0: Cartridge Header

**Size:** 4096 bytes (PAGE_SIZE)

#### Byte Layout

| Offset | Size | Type    | Field           | Description                               |
| ------ | ---- | ------- | --------------- | ----------------------------------------- |
| 0      | 8    | u8[8]   | magic           | Magic number: `"CART\x00\x01\x00\x00"`    |
| 8      | 2    | u16     | version_major   | Major version (1 for v0.1)                |
| 10     | 2    | u16     | version_minor   | Minor version (0 for v0.1)                |
| 12     | 4    | u32     | block_size      | Block size in bytes (4096, constant)      |
| 16     | 8    | u64     | total_blocks    | Total number of blocks in archive         |
| 24     | 8    | u64     | free_blocks     | Number of free blocks available           |
| 32     | 8    | u64     | btree_root_page | Page ID of B-tree catalog root (always 1) |
| 40     | 256  | u8[256] | reserved        | Reserved for future use (all zeros)       |
| 296    | 3800 | -       | padding         | Padding to 4096 bytes (all zeros)         |

#### C Structure (for reference)

```c
struct CartridgeHeader {
    uint8_t  magic[8];          // "CART\x00\x01\x00\x00"
    uint16_t version_major;     // 1
    uint16_t version_minor;     // 0
    uint32_t block_size;        // 4096
    uint64_t total_blocks;      // Archive capacity
    uint64_t free_blocks;       // Available blocks
    uint64_t btree_root_page;   // Catalog root (page 1)
    uint8_t  reserved[256];     // Future extensions
    uint8_t  padding[3800];     // Pad to 4096 bytes
};
```

#### Magic Number

```
Byte:  0     1     2     3     4     5     6     7
Value: 0x43  0x41  0x52  0x54  0x00  0x01  0x00  0x00
ASCII: 'C'   'A'   'R'   'T'   NUL   SOH   NUL   NUL
```

- **"CART"** = Cartridge identifier
- **0x00 0x01** = Format version marker (v0.1)
- **0x00 0x00** = Reserved for future use

#### Version Semantics

- **version_major:** Breaking changes (incompatible format)
- **version_minor:** Compatible additions (new fields in reserved space)

Current version: **1.0** (v0.1 development phase)

#### Reserved Space Usage

The 256-byte reserved space (offset 40-295) can be used for future extensions:

**Proposed Allocations (not yet implemented):**

| Offset | Size | Purpose                                     |
| ------ | ---- | ------------------------------------------- |
| 40     | 1    | compression_method (0=None, 1=LZ4, 2=Zstd)  |
| 41     | 1    | encryption_enabled (0=No, 1=Yes)            |
| 42     | 32   | encryption_key_hash (SHA-256 of master key) |
| 74     | 8    | snapshot_count (number of snapshots)        |
| 82     | 8    | audit_log_start (first audit log page)      |
| 90     | 8    | freelist_start (first freelist page)        |
| 98     | 158  | reserved_future (zeros)                     |

#### Validation Rules

1. **Magic number** must be exactly `"CART\x00\x01\x00\x00"`
2. **version_major** must be 1 (current implementation)
3. **version_minor** must be 0 (current implementation)
4. **block_size** must be 4096
5. **free_blocks** ≤ **total_blocks**
6. **btree_root_page** must be 1 (fixed for v0.1)

---

## Page Format

### Page Structure

All pages (except page 0) share a common structure:

**Size:** 4096 bytes

#### Byte Layout

| Offset | Size | Type     | Field     | Description                         |
| ------ | ---- | -------- | --------- | ----------------------------------- |
| 0      | 1    | u8       | page_type | Page type identifier (0-4)          |
| 1      | 32   | u8[32]   | checksum  | SHA-256 checksum of data (optional) |
| 33     | 31   | u8[31]   | reserved  | Reserved for future use             |
| 64     | 4032 | u8[4032] | data      | Page data (varies by type)          |

#### Page Types

| Value | Name         | Description                  |
| ----- | ------------ | ---------------------------- |
| 0     | Header       | Archive header (page 0 only) |
| 1     | CatalogBTree | B-tree catalog node          |
| 2     | ContentData  | File content data            |
| 3     | Freelist     | Free block tracking          |
| 4     | AuditLog     | Audit log entries            |

#### Page Header (64 bytes)

```c
struct PageHeader {
    uint8_t page_type;      // Page type (0-4)
    uint8_t checksum[32];   // SHA-256 of data (optional)
    uint8_t reserved[31];   // Future extensions
};
```

#### Checksum Computation

- **Algorithm:** SHA-256
- **Input:** Page data (4032 bytes, offset 64-4095)
- **Output:** 32-byte hash stored at offset 1-32

**Optional Checksum:**

- If checksum is all zeros (32 × 0x00): **Skip verification**
- If checksum is non-zero: **Verify on read**

**Verification:**

```rust
fn verify_checksum(page: &Page) -> bool {
    if page.header.checksum == [0u8; 32] {
        return true; // Skip verification
    }
    let computed = sha256(&page.data);
    computed == page.header.checksum
}
```

---

## Catalog Structure

### Page 1: B-tree Catalog

**Current Implementation (v0.1):** In-memory B-tree serialized to single page

**Future (v0.2):** Multi-page B-tree with proper paging

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
      "created": 1700000000,
      "modified": 1700000000
    },
    "/data.bin": {
      "file_type": "File",
      "size": 8192,
      "blocks": [5, 6],
      "created": 1700000000,
      "modified": 1700000000
    }
  }
}
```

**Limitations:**

- Maximum catalog size: 4032 bytes (single page)
- Approximate capacity: 10-20 files (depends on path lengths)

#### FileMetadata Structure

```rust
struct FileMetadata {
    file_type: FileType,        // "File" or "Directory"
    size: u64,                  // Size in bytes
    blocks: Vec<u64>,           // Allocated block IDs
    created: u64,               // Unix timestamp (microseconds)
    modified: u64,              // Unix timestamp (microseconds)
}
```

#### FileType Values

| Value | Name      | Description                     |
| ----- | --------- | ------------------------------- |
| 0     | File      | Regular file                    |
| 1     | Directory | Directory (no blocks allocated) |

#### Future Multi-Page B-tree (v0.2)

**Proposed Format:**

```
B-tree Node Page (Page Type: CatalogBTree)
├─ Node Header (64 bytes)
│  ├─ is_leaf: u8 (1 = leaf, 0 = internal)
│  ├─ key_count: u16 (number of keys in node)
│  ├─ parent_page: u64 (parent node page ID, 0 if root)
│  └─ reserved: [u8; 47]
│
└─ Node Data (4032 bytes)
   ├─ Keys: [String] (variable length, null-terminated)
   ├─ Values (leaf): [FileMetadata] (serialized)
   └─ Pointers (internal): [u64] (child page IDs)
```

---

## Allocation Metadata

### Page 2: Allocator State

**Current Implementation (v0.1):** Serialized with JSON (temporary)

**Future (v0.2):** Binary format for efficiency

#### Current Format (JSON Serialization)

```json
{
  "bitmap": {
    "bitmap": [0, 0, 7, ...],  // Vec<u64>, 64 blocks per entry
    "total_blocks": 10000,
    "allocated_count": 150
  },
  "extent": {
    "free_extents": [
      {"start": 500, "length": 1000},
      {"start": 2000, "length": 5000}
    ],
    "total_blocks": 10000
  }
}
```

#### Bitmap Format

**Structure:**

- Array of u64 values
- Each bit represents one block (0 = free, 1 = allocated)
- 64 blocks per u64 entry
- Little-endian bit order

**Example:**

```
bitmap[0] = 0b0000_0000_0000_0111 (binary)
          = 7 (decimal)

Blocks 0, 1, 2 are allocated (bits set)
Blocks 3-63 are free (bits clear)
```

#### Extent Format

**Structure:**

```rust
struct Extent {
    start: u64,     // Starting block ID
    length: u64,    // Number of contiguous blocks
}
```

**Free Extents List:**

- Sorted by start block ID
- Merged automatically on free (coalescing)

---

## Compression Format

### Compressed Page Data

Compression is applied to page data (4032 bytes), not the entire page.

#### Compression Methods

| Value | Name | Description                                  |
| ----- | ---- | -------------------------------------------- |
| 0     | None | No compression                               |
| 1     | LZ4  | LZ4 compression (fast, moderate ratio)       |
| 2     | Zstd | Zstandard compression (slower, better ratio) |

#### Compressed Data Format

**For LZ4:**

```
[size_prefix: 4 bytes][compressed_data]
```

- LZ4 uses `compress_prepend_size()` which adds a 4-byte size prefix
- Total size: 4 + compressed_length

**For Zstd:**

```
[compressed_data]
```

- Zstd includes size information in the frame
- Decompress with `decompress(data, max_size)`

#### Compression Decision

Compression is applied if:

1. Data size ≥ threshold (512 bytes for LZ4, 1024 for Zstd)
2. Compression ratio < min_ratio (0.9 for LZ4, 0.85 for Zstd)

Otherwise, data is stored uncompressed.

#### Compression Metadata (Future)

In v0.2, page header reserved space may include:

| Offset | Size | Field              | Values                    |
| ------ | ---- | ------------------ | ------------------------- |
| 33     | 1    | compression_method | 0=None, 1=LZ4, 2=Zstd     |
| 34     | 4    | original_size      | Uncompressed size (bytes) |
| 38     | 4    | compressed_size    | Compressed size (bytes)   |

---

## Encryption Format

### Encrypted Page Data

Encryption is applied to page data (4032 bytes) after compression (if enabled).

#### Encryption Algorithm

**AES-256-GCM (Galois/Counter Mode)**

- Key size: 256 bits (32 bytes)
- Nonce size: 96 bits (12 bytes)
- Tag size: 128 bits (16 bytes)

#### Encrypted Data Format

```
[nonce: 12 bytes][ciphertext][auth_tag: 16 bytes]
```

**Total overhead:** 28 bytes (12 + 16)

#### Nonce Generation

- Random 96-bit nonce per encryption operation
- Generated with cryptographically secure RNG (OsRng)
- Stored at the beginning of encrypted data

**Future:** Nonce may be derived from `page_id + counter` for space efficiency.

#### Master Key

**Storage:**

- 32-byte master key (not stored in cartridge file)
- Provided at runtime (environment variable, config file, hardware key)

**Future:** Key derivation function (KDF) for page-specific keys:

```
page_key = HKDF-SHA256(master_key, page_id, "cartridge-page-key")
```

#### Authentication Tag

- 128-bit GCM authentication tag
- Verifies ciphertext integrity
- Prevents tampering and forgery

**Decryption failure indicates:**

- Wrong key
- Corrupted ciphertext
- Tampered data

---

## IAM Policy Format

### IAM Policy Structure

IAM policies are stored as JSON (not in cartridge file, but in engram manifest).

#### Policy JSON Schema

```json
{
  "version": "2012-10-17",
  "statement": [
    {
      "effect": "Allow",
      "action": ["Read", "Write", "Create"],
      "resource": ["/public/**"],
      "condition": null
    },
    {
      "effect": "Deny",
      "action": ["All"],
      "resource": ["/secret/**"],
      "condition": null
    }
  ]
}
```

#### Effect Values

| Value | Name  | Description                         |
| ----- | ----- | ----------------------------------- |
| Allow | Allow | Grant access                        |
| Deny  | Deny  | Deny access (precedence over Allow) |

#### Action Values

| Value  | Description             |
| ------ | ----------------------- |
| Read   | Read file content       |
| Write  | Modify file content     |
| Create | Create new files        |
| Delete | Delete files            |
| List   | List directory contents |
| All    | All actions (wildcard)  |

#### Resource Patterns

**Wildcards:**

- `*` - Match single path segment (e.g., `/docs/*.md`)
- `**` - Match multiple segments (e.g., `/data/**`)

**Examples:**

- `/config.json` - Exact match
- `/docs/*.md` - All markdown files in /docs
- `/data/**` - All files under /data (recursive)
- `/**` - All files (global)

#### Condition Structure (Optional)

```json
{
  "operator": "StringEquals",
  "key": "path",
  "value": "/admin/config.json"
}
```

**Operators:**

- `StringEquals`, `StringNotEquals`
- `NumericEquals`, `NumericLessThan`, `NumericGreaterThan`
- `DateBefore`, `DateAfter`

---

## Snapshot Format

### Snapshot Directory Structure

Snapshots are stored in a separate directory, not inside the cartridge file.

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
  "description": "First version",
  "created_at": 1700000000000000,
  "parent_path": "/path/to/cartridge-rs.cart",
  "header": {
    "magic": [67, 65, 82, 84, 0, 1, 0, 0],
    "version_major": 1,
    "version_minor": 0,
    "block_size": 4096,
    "total_blocks": 10000,
    "free_blocks": 9850,
    "btree_root_page": 1,
    "reserved": [0, 0, ...]
  },
  "modified_pages": [],
  "size_bytes": 163840
}
```

### Snapshot Pages (pages.bin)

Binary format:

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

**Example:**

```
Page count:  2 (8 bytes, little-endian)

Page Entry 1:
  page_id:   3 (8 bytes)
  page_size: 4096 (8 bytes)
  page_data: [4096 bytes of page 3 data]

Page Entry 2:
  page_id:   4 (8 bytes)
  page_size: 4096 (8 bytes)
  page_data: [4096 bytes of page 4 data]
```

---

## Version History

### v0.1.0 (Current)

**Release Date:** 2025-11-20
**Status:** Production

**Features:**

- Fixed 4KB page format
- Magic number and version in header
- Reserved space for future extensions
- B-tree catalog (in-memory, JSON serialization)
- Bitmap + extent hybrid allocator
- SHA-256 checksums (optional)
- LZ4/Zstd compression
- AES-256-GCM encryption
- IAM policy support
- Snapshot support

**Limitations:**

- Single-page catalog (limited to ~10,000 files)
- JSON serialization (inefficient)
- No WAL (crash recovery)
- No compaction

### v0.2.0 (Planned)

**Target:** Q1 2026

**Planned Changes:**

- Multi-page B-tree catalog (scale to millions of files)
- Binary serialization (replace JSON)
- WAL for crash recovery
- Compaction and defragmentation
- Incremental snapshots (delta compression)
- MVCC for concurrent access

**Breaking Changes:**

- Catalog format (backward compatible reader)
- Allocator format (binary instead of JSON)

---

## Compatibility

### Forward Compatibility

Implementations must:

1. **Ignore unknown reserved bytes** (treat as zeros)
2. **Check version_major** for compatibility
3. **Support older version_minor** within same major version

**Example:**

```rust
fn is_compatible(header: &Header) -> bool {
    header.version_major == 1 && header.version_minor <= 0
}
```

### Backward Compatibility

v0.2 readers must:

- Support v0.1 files (read-only if necessary)
- Convert v0.1 catalog to v0.2 on write
- Preserve unknown fields in reserved space

### Migration Path

**v0.1 → v0.2:**

1. Read v0.1 file
2. Parse JSON catalog and allocator
3. Convert to binary format
4. Write v0.2 file
5. Optionally: compact and defragment

**Tool:**

```bash
cartridge-migrate --input v0.1.cart --output v0.2.cart
```

---

## Appendix A: Binary Format Examples

### Example 1: Minimal Cartridge

**Header (Page 0):**

```
00000000: 43 41 52 54 00 01 00 00  01 00 00 00 00 10 00 00  CART............
00000010: E8 03 00 00 00 00 00 00  E5 03 00 00 00 00 00 00  ................
00000020: 01 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  ................
...
```

**Decoded:**

- Magic: "CART\x00\x01\x00\x00"
- Version: 1.0
- Block size: 4096
- Total blocks: 1000
- Free blocks: 997
- B-tree root: page 1

### Example 2: Content Page

**Page 3 (Content Data):**

```
00000000: 02 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  ................
00000010: 00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  ................
00000020: 00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  ................
00000030: 00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  ................
00000040: 48 65 6C 6C 6F 2C 20 57  6F 72 6C 64 21 00 00 00  Hello, World!...
...
```

**Decoded:**

- Page type: 2 (ContentData)
- Checksum: all zeros (disabled)
- Data: "Hello, World!" + padding

---

## Appendix B: File Format Checklist

### Implementation Checklist

For implementing a Cartridge reader/writer:

**Header:**

- [ ] Verify magic number
- [ ] Check version compatibility
- [ ] Validate block_size == 4096
- [ ] Validate free_blocks ≤ total_blocks
- [ ] Read btree_root_page

**Pages:**

- [ ] Read/write 4KB pages
- [ ] Parse page headers (type, checksum)
- [ ] Compute/verify SHA-256 checksums

**Catalog:**

- [ ] Deserialize B-tree from page 1
- [ ] Lookup files by path
- [ ] Insert/update/delete entries

**Allocator:**

- [ ] Deserialize allocator from page 2
- [ ] Allocate/free blocks
- [ ] Track free blocks

**Optional Features:**

- [ ] Compression (LZ4/Zstd)
- [ ] Encryption (AES-256-GCM)
- [ ] IAM policy evaluation
- [ ] Snapshots

---

**End of Specification**
