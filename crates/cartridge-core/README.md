# Cartridge

A high-performance, offline-first virtual filesystem designed for embedded systems (Raspberry Pi 5 and similar resource-constrained platforms).

## What is Cartridge?

Cartridge is a specialized archive format and storage system that provides a mutable, virtualized filesystem within a single file. Think of it as a hybrid between a traditional filesystem and a database, optimized for embedded systems with limited resources.

**Key Design Goals:**
- Run efficiently on Raspberry Pi 5 (4GB RAM, ARM64)
- Provide fast file operations with minimal memory overhead
- Support embedded databases (SQLite) as a virtual filesystem
- Enable compression and encryption without complexity
- Freeze into immutable archives (Engrams) for distribution

## Why Cartridge?

Traditional filesystems and archives have limitations for embedded systems:

| Problem | Traditional Approach | Cartridge Solution |
|---------|---------------------|-------------------|
| SQLite needs a real filesystem | Mount filesystem or use WAL mode | SQLite VFS integration (run SQLite inside cartridge) |
| Archives are immutable | Extract, modify, re-pack | Mutable workspace with freeze-on-demand |
| Memory constraints | Large buffers, full file loads | 4KB page-based I/O with ARC caching |
| File fragmentation | Filesystem-dependent | Hybrid allocator (bitmap + extent) |
| Data integrity | Filesystem journaling | Per-page SHA-256 checksums |
| Access control | OS-level permissions | IAM policies with capability-based auth |

## Key Features

### Core Storage

- **4KB Fixed Pages:** Optimal alignment with filesystem and memory pages
- **Hybrid Allocator:** Bitmap for small files (<256KB), extent-based for large files (≥256KB)
- **B-tree Catalog:** Efficient path lookups and directory listings
- **ARC Buffer Pool:** Adaptive Replacement Cache for hot data (better than LRU)
- **Copy-on-Write Snapshots:** Lightweight backups and versioning

### Performance

From benchmarks (see `performance.md`):
- **Read Performance:** Up to 18 GiB/s (64KB blocks)
- **Write Performance:** Up to 9 GiB/s (64KB blocks)
- **LZ4 Compression:** 9.77 GiB/s compression, 38.12 GiB/s decompression
- **Zstd Compression:** 5.15 GiB/s compression (better ratio)
- **Allocation:** 10.4 μs for large contiguous blocks

### Security & Integrity

- **SHA-256 Checksums:** Optional per-page verification
- **AES-256-GCM Encryption:** Authenticated encryption for sensitive data
- **IAM Policies:** Fine-grained access control with Allow/Deny rules
- **Audit Logging:** Tamper-evident operation tracking (<1% overhead)

### Integration

- **SQLite VFS:** Run SQLite databases directly inside cartridge files
- **Engram Export:** Freeze mutable cartridges into immutable, compressed archives
- **LZ4/Zstd Compression:** Transparent compression for storage efficiency
- **Platform Support:** Windows (x86_64), Linux (x86_64, ARM64, ARMv7), macOS (Intel, Apple Silicon)

## Quick Start

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
cartridge = { path = "path/to/cartridge" }
```

### Basic Usage

```rust
use cartridge::{Cartridge, Result};

fn main() -> Result<()> {
    // Create a new in-memory cartridge (1000 blocks = ~4MB)
    let mut cart = Cartridge::new(1000);

    // Create files
    cart.create_file("/readme.txt", b"Hello, Cartridge!")?;
    cart.create_file("/data.bin", &vec![42u8; 1024])?;

    // Read files
    let content = cart.read_file("/readme.txt")?;
    println!("Content: {}", String::from_utf8_lossy(&content));

    // Update files
    cart.write_file("/readme.txt", b"Updated content")?;

    // Create directories
    cart.create_dir("/docs")?;
    cart.create_file("/docs/guide.md", b"# Guide\nContent here")?;

    // List directory
    let files = cart.list_dir("/docs")?;
    for file in files {
        println!("Found: {}", file);
    }

    // Delete files
    cart.delete_file("/data.bin")?;

    // Get statistics
    let stats = cart.stats();
    println!("Used: {} / {} blocks", stats.used_blocks, stats.total_blocks);

    Ok(())
}
```

### Disk-Backed Cartridge

```rust
use cartridge::{Cartridge, Result};
use std::path::Path;

fn main() -> Result<()> {
    // Create a disk-backed cartridge
    let path = Path::new("archive.cart");
    let mut cart = Cartridge::create(path, 10000)?;

    // Use it like an in-memory cartridge
    cart.create_file("/config.json", br#"{"key": "value"}"#)?;
    cart.create_file("/large.dat", &vec![0u8; 1024 * 1024])?;

    // Flush changes to disk
    cart.flush()?;

    // Close (automatically flushes)
    cart.close()?;

    // Reopen later
    let mut cart = Cartridge::open(path)?;
    let config = cart.read_file("/config.json")?;
    println!("Config: {}", String::from_utf8_lossy(&config));

    Ok(())
}
```

### Snapshots

```rust
use cartridge::{Cartridge, Result};
use std::path::Path;

fn main() -> Result<()> {
    let mut cart = Cartridge::new(1000);
    let snapshot_dir = Path::new("snapshots");

    // Create initial files
    cart.create_file("/data.txt", b"Version 1")?;

    // Create snapshot
    let snapshot_id = cart.create_snapshot(
        "v1".to_string(),
        "First version".to_string(),
        snapshot_dir,
    )?;

    // Modify data
    cart.write_file("/data.txt", b"Version 2")?;

    // Restore from snapshot
    cart.restore_snapshot(snapshot_id, snapshot_dir)?;

    // Data is back to "Version 1"
    let content = cart.read_file("/data.txt")?;
    assert_eq!(content, b"Version 1");

    Ok(())
}
```

### Freezing to Engram

```rust
use cartridge::{Cartridge, EngramFreezer, Result};
use engram_rs::CompressionMethod;
use std::path::Path;

fn main() -> Result<()> {
    let mut cart = Cartridge::new(1000);

    // Add content
    cart.create_file("/readme.txt", b"Hello, Engram!")?;
    cart.create_file("/data.json", br#"{"frozen": true}"#)?;

    // Create freezer
    let freezer = EngramFreezer::new(
        "my-cartridge".to_string(),
        "1.0.0".to_string(),
        "Author Name".to_string(),
        Some("Frozen cartridge archive".to_string()),
        CompressionMethod::Zstd,
    );

    // Freeze to immutable engram
    let engram_path = Path::new("archive.eng");
    freezer.freeze(&mut cart, engram_path)?;

    // Now you have an immutable, compressed, signed archive
    println!("Frozen to: {:?}", engram_path);

    Ok(())
}
```

### IAM Policies

```rust
use cartridge::{Cartridge, Result};
use cartridge::iam::{Policy, Statement, Effect, Action};

fn main() -> Result<()> {
    let mut cart = Cartridge::new(1000);

    // Create IAM policy
    let mut policy = Policy::new();

    // Allow read/write on /public/**
    policy.add_statement(Statement::new(
        Effect::Allow,
        vec![Action::Read, Action::Write, Action::Create],
        vec!["/public/**".to_string()],
    ));

    // Deny all access to /secret/**
    policy.add_statement(Statement::new(
        Effect::Deny,
        vec![Action::All],
        vec!["/secret/**".to_string()],
    ));

    // Apply policy to cartridge
    cart.set_policy(policy);

    // This works
    cart.create_file("/public/readme.txt", b"Public file")?;

    // This fails (access denied)
    let result = cart.create_file("/secret/key.pem", b"Secret");
    assert!(result.is_err());

    Ok(())
}
```

### SQLite VFS Integration

```rust
use cartridge::{Cartridge, Result};
use cartridge::vfs::register_vfs;
use std::sync::{Arc, Mutex};

fn main() -> Result<()> {
    // Create cartridge and wrap in Arc<Mutex<>>
    let cart = Cartridge::new(10000);
    let cart_arc = Arc::new(Mutex::new(cart));

    // Register VFS with SQLite
    register_vfs(cart_arc.clone())?;

    // Now you can use SQLite with the "cartridge" VFS
    let conn = rusqlite::Connection::open_with_flags(
        "mydb.db",
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
    )?;

    // Use SQLite normally - all data stored in cartridge
    conn.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)", [])?;
    conn.execute("INSERT INTO users (name) VALUES (?)", ["Alice"])?;

    // Query works as expected
    let name: String = conn.query_row("SELECT name FROM users WHERE id = 1", [], |row| row.get(0))?;
    println!("User: {}", name);

    Ok(())
}
```

## Performance Highlights

From comprehensive benchmarks (see `performance.md`):

### File I/O
- **Read (64KB):** 17.91 GiB/s (P50: 3.41μs)
- **Write (64KB):** 9.41 GiB/s (P50: 6.48μs)
- **Optimal Block Size:** 64KB (sweet spot for throughput)

### Compression
- **LZ4 Compression:** 9.77 GiB/s (64KB blocks)
- **LZ4 Decompression:** 38.12 GiB/s (64KB blocks, 3.9x faster than compression)
- **Zstd Compression:** 5.15 GiB/s (better ratio, 2x slower than LZ4)

### Allocation
- **Hybrid Small (bitmap):** 1.73 ms for 100K blocks
- **Hybrid Large (extent):** 10.4 μs for 100K blocks (16,700x faster)
- **Fragmentation Score:** 535 ps (extent), 4.67 μs (bitmap)

### ARC Cache
- **Cache Hit:** 20.37 μs (100 entries), 6.10 ms (10,000 entries)
- **Cache Miss:** 3.26 ns (constant time hash lookup)
- **Adaptation:** 164 μs to shift between recency/frequency workloads

## Use Cases

### Embedded Systems

Run complete applications with databases and file storage on Raspberry Pi:

```rust
// Single-file deployment with embedded SQLite
let cart = Cartridge::create("app.cart", 100000)?;
register_vfs(Arc::new(Mutex::new(cart)))?;

// SQLite database runs inside cartridge
let db = Connection::open_with_flags("app.db", OpenFlags::SQLITE_OPEN_CREATE)?;
// ... use database normally
```

### Offline-First Applications

Build apps that work without network access:

```rust
// Development: mutable workspace
let mut cart = Cartridge::create("workspace.cart", 50000)?;
cart.create_file("/data/config.json", config_bytes)?;
cart.create_file("/models/embeddings.bin", model_bytes)?;

// Production: freeze to immutable engram
let freezer = EngramFreezer::new_default("app-v1".into(), "1.0.0".into(), "Team".into());
freezer.freeze(&mut cart, Path::new("app-v1.eng"))?;
```

### Data Archival

Create compressed, verified archives with access control:

```rust
let mut cart = Cartridge::create("archive.cart", 1000000)?;

// Add policy for compliance
let mut policy = Policy::new();
policy.add_statement(Statement::new(
    Effect::Allow,
    vec![Action::Read],
    vec!["/**".to_string()],
));
cart.set_policy(policy);

// Add files with checksums
for file in &files {
    cart.create_file(&file.path, &file.content)?;
}

// Freeze with Zstd compression
let freezer = EngramFreezer::new(
    "archive-2025".into(),
    "1.0.0".into(),
    "Compliance Team".into(),
    Some("Annual archive".into()),
    CompressionMethod::Zstd,
);
freezer.freeze(&mut cart, Path::new("archive-2025.eng"))?;
```

### Testing & CI/CD

Fast, reproducible test environments:

```rust
#[test]
fn test_with_filesystem() {
    // Each test gets isolated cartridge
    let mut cart = Cartridge::new(1000);
    cart.create_file("/test/data.txt", b"test")?;

    // Run tests
    assert_eq!(cart.read_file("/test/data.txt")?, b"test");

    // No cleanup needed (in-memory)
}
```

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│               Cartridge Public API                      │
│  create_file, read_file, write_file, delete_file       │
└────────────────────┬────────────────────────────────────┘
                     │
    ┌────────────────┴────────────────┐
    │                                 │
┌───▼────────┐              ┌────────▼──────┐
│  Catalog   │              │   Allocator   │
│  (B-tree)  │              │   (Hybrid)    │
└───┬────────┘              └────────┬──────┘
    │                                │
    │   Path → Metadata              │   Block Management
    │   (size, blocks)               │
    │                                │
    └──────────┬─────────────────────┘
               │
         ┌─────▼─────┐
         │   Pager   │
         │ (4KB I/O) │
         └─────┬─────┘
               │
      ┌────────┴────────┐
      │                 │
┌─────▼──────┐    ┌────▼──────┐
│ ARC Cache  │    │    I/O    │
│ (Hot Data) │    │ (Disk/Mem)│
└────────────┘    └───────────┘
```

### Component Breakdown

- **Cartridge:** High-level API for file operations
- **Catalog:** B-tree mapping paths to metadata (size, blocks, timestamps)
- **Allocator:** Hybrid allocator (bitmap for small, extent for large)
- **Pager:** 4KB page-based I/O with checksums
- **Buffer Pool:** ARC cache for frequently accessed pages
- **I/O Layer:** Abstraction over disk files or in-memory storage

## Documentation

- **README.md** (this file) - Introduction and quick start
- **ARCHITECTURE.md** - Deep technical architecture details
- **SPECIFICATION.md** - Formal file format specification (v0.1)
- **CARTRIDGE_EXECUTIVE_SUMMARY.md** - Executive overview and status
- **performance.md** - Comprehensive benchmark results

## Testing

Cartridge has 192 passing tests covering all major subsystems:

```bash
# Run all tests
cd crates/cartridge
cargo test

# Run specific subsystem tests
cargo test --lib catalog
cargo test --lib allocator
cargo test --lib buffer_pool
cargo test --lib snapshot

# Run benchmarks
cargo bench
```

## Current Status

**Version:** 0.1.0 (Phase 7 Complete)
**Production Readiness:** Ready for embedded/offline use cases
**Test Coverage:** 192/193 tests passing (99.5%)

### Completed Features

- ✅ Core storage layer (pages, header, allocator)
- ✅ Hybrid allocator (bitmap + extent)
- ✅ B-tree catalog
- ✅ ARC buffer pool
- ✅ Disk I/O with flush/sync
- ✅ LZ4/Zstd compression
- ✅ AES-256-GCM encryption
- ✅ IAM policies with caching
- ✅ Audit logging
- ✅ Copy-on-write snapshots
- ✅ SQLite VFS integration
- ✅ Engram freezing

### Roadmap (v0.2)

- [ ] Multi-threaded I/O with async/await
- [ ] Compaction/defragmentation
- [ ] Incremental snapshots (delta compression)
- [ ] B-tree page splitting (currently in-memory only)
- [ ] WAL (Write-Ahead Logging) for crash recovery
- [ ] Concurrent read/write with MVCC

## Contributing

Cartridge is part of the SAM (Societal Advisory Module) project. See the main SAM repository for contribution guidelines.

## License

Copyright (c) 2025 Manifest Humanity. All rights reserved.

This is proprietary software for internal use by crisis call centers. Not for public distribution.

## Contact

For questions or issues, please refer to the main SAM project documentation.
