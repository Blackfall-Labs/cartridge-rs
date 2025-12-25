# Cartridge

**v0.2.4** | **Production Ready** | **High-Performance Mutable Containers**

Offline-first storage system with auto-growth, SQLite VFS support, and cryptographic freezing.

[![Crates.io](https://img.shields.io/crates/v/cartridge-rs)](https://crates.io/crates/cartridge-rs)
[![Documentation](https://docs.rs/cartridge-rs/badge.svg)](https://docs.rs/cartridge-rs)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE-MIT)

---

## Overview

Cartridge is a production-ready storage system for **mutable containers** that start small (12KB) and grow automatically. Perfect for embedded systems, offline applications, and data distribution.

**Key Features:**
- **Auto-Growth:** Start at 12KB, expand as needed (no capacity planning)
- **High Performance:** 17.9 GiB/s reads, 9.4 GiB/s writes (verified benchmarks)
- **SQLite VFS:** Run databases directly inside containers
- **Immutable Freezing:** Convert to cryptographically signed Engram archives
- **Offline-First:** Works on Raspberry Pi 5 through enterprise servers

---

## Quick Start

### Installation

```toml
[dependencies]
cartridge-rs = "0.2.4"
```

### Basic Usage

```rust
use cartridge_rs::Cartridge;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create container - starts at 12KB, grows automatically!
    let mut cart = Cartridge::create("my-data", "My Data Container")?;

    // Write files (auto-creates directories)
    cart.write("documents/report.txt", b"Hello, World!")?;
    cart.write("config/settings.json", br#"{"version": "1.0"}"#)?;

    // Read files
    let content = cart.read("documents/report.txt")?;
    println!("{}", String::from_utf8_lossy(&content));

    // List directory
    let files = cart.list("documents")?;
    for file in files {
        println!("Found: {}", file);
    }

    // Access metadata
    println!("Container: {} ({})", cart.title()?, cart.slug()?);

    Ok(())
}
```

**Creates:** `my-data.cart` file

---

## Core Concepts

### Slug vs Title

- **Slug:** Kebab-case identifier for filenames (e.g., `us-constitution`)
- **Title:** Human-readable display name (e.g., `U.S. Constitution`)

```rust
let cart = Cartridge::create("us-constitution", "U.S. Constitution")?;
// Creates file: us-constitution.cart
// Display name: U.S. Constitution
```

### Auto-Growth

No capacity planning required. Containers start minimal and expand automatically:

```
12KB (3 blocks) → 24KB → 48KB → 96KB → 192KB → ... → ∞
```

**Example:**
```rust
// No capacity planning needed!
let mut cart = Cartridge::create("my-data", "My Data")?;

// Add 100KB file - container automatically grows
let large_data = vec![0u8; 100_000];
cart.write("large.bin", &large_data)?;

// Container grew from 12KB to 384KB automatically
```

### Container vs Archive

- **Container:** Mutable Cartridge (this crate)
- **Archive:** Immutable Engram (created via freezing)

```rust
// Mutable container
let mut cart = Cartridge::create("data", "My Data")?;
cart.write("file.txt", b"mutable")?;

// Freeze to immutable archive
cart.freeze_to_engram("data.eng")?;
```

---

## Features

### High-Performance I/O

**Throughput (64KB blocks):**
- Read: **17.91 GiB/s** (verified)
- Write: **9.41 GiB/s** (verified)

**Architecture:**
- Fixed 4KB pages (optimal for filesystems and databases)
- Hybrid allocator (Bitmap for small files, Extent for large files)
- ARC buffer pool (Adaptive Replacement Cache)

*Performance verified via docs/performance.md (2025-11-20)*

### Compression & Encryption

**Compression:**
```rust
// Transparent LZ4 compression (9.77 GiB/s)
cart.enable_compression()?;
```

**Encryption:**
```rust
// AES-256-GCM encryption
let key = [0u8; 32];  // Load from secure source
cart.enable_encryption(&key)?;
```

**Compression Performance (verified):**
- LZ4 Compression: 9.77 GiB/s
- LZ4 Decompression: 38.12 GiB/s (3.9x faster)
- Zstd Compression: 4.87 GiB/s
- Zstd Decompression: ~5.64 GiB/s

### Snapshots

```rust
// Create snapshot
let snapshot_id = cart.create_snapshot("backup-2025")?;

// Restore snapshot
cart.restore_snapshot(snapshot_id)?;

// List snapshots
let snapshots = cart.list_snapshots()?;
```

**Copy-on-Write:** Only modified pages are saved (90%+ space savings)

### IAM Policies

AWS-style access control:

```rust
use cartridge_rs::{Policy, Statement, Action, Effect};

let policy = Policy::new("read-only", vec![
    Statement {
        effect: Effect::Allow,
        actions: vec![Action::Read],
        resources: vec!["documents/**".to_string()],
    },
    Statement {
        effect: Effect::Deny,
        actions: vec![Action::Write, Action::Delete],
        resources: vec!["**".to_string()],
    },
]);

cart.set_policy(policy);
```

**Performance:** 1,000,000+ policy evaluations/sec (cached)

### SQLite VFS

Run SQLite databases **inside** containers:

```rust
use rusqlite::{Connection, OpenFlags};
use cartridge_rs::vfs::register_vfs;

// Register VFS
register_vfs()?;

// Open database inside container
let conn = Connection::open_with_flags(
    "file:mydb.db?vfs=cartridge&cartridge=my-data.cart",
    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
)?;

conn.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)", [])?;
conn.execute("INSERT INTO users (name) VALUES (?1)", ["Alice"])?;
```

**Performance:** ~90% of native filesystem speed

### Engram Freezing

Convert to immutable, signed archives:

```rust
use cartridge_rs::engram_integration::EngramFreezer;

let freezer = EngramFreezer::new_default(
    "my-data".to_string(),
    "1.0.0".to_string(),
    "Dataset v1".to_string(),
);

freezer.freeze(&cart, "my-data.eng")?;
```

**Result:** Immutable `.eng` archive with Ed25519 signatures

---

## Advanced Configuration

```rust
use cartridge_rs::CartridgeBuilder;

let cart = CartridgeBuilder::new()
    .slug("my-data")
    .title("My Data Container")
    .path("/custom/path/my-data")  // Optional: custom location
    .initial_blocks(3)             // Start small (12KB)
    .growth_threshold_percent(10)  // Grow at 10% free space
    .max_blocks(1_000_000)         // Cap at ~4GB
    .buffer_pool_size(10_000)      // 40 MB cache
    .with_audit_logging()          // Enable audit log
    .build()?;
```

---

## Examples

Run examples to see Cartridge in action:

```bash
# Basic usage
cargo run --example basic

# Auto-growth demonstration
cargo run --example auto_growth

# Manifest and metadata
cargo run --example manifest

# VFS trait for generic code
cargo run --example vfs_trait

# Compression analysis
cargo run --example compression_analysis

# SQLite VFS integration
cargo run --example sqlite_vfs
```

---

## Performance

### Verified Benchmarks

**File I/O (64KB blocks):**
- Read: 17.91 GiB/s (mean), 18.38 GiB/s (peak)
- Write: 9.41 GiB/s (mean), 9.59 GiB/s (peak)

**Compression (64KB blocks):**
- LZ4 Compression: 9.77 GiB/s
- LZ4 Decompression: 38.12 GiB/s
- Zstd Compression: 4.87 GiB/s
- Zstd Decompression: ~5.64 GiB/s

**Allocator:**
- Bitmap (small files): 24,096 blocks/ms
- Extent (large files): 173,611 blocks/ms (301x faster)

**Buffer Pool (ARC):**
- Cache Hit: 20-255 μs (pool size: 100-1000)
- Cache Miss: 3.26 ns
- Adaptation: 164 μs

*Source: docs/performance.md (generated 2025-11-20)*

**Note:** Auto-growth overhead claim (< 1ms per doubling) is **unverified**. See PERFORMANCE_VERIFICATION.md for details.

### Optimal Block Size

**64KB** provides maximum throughput for both reads and writes.

---

## Architecture

```
┌─────────────────────────────────────┐
│      High-Level API (lib.rs)       │
│  • Cartridge::create()              │
│  • Simple read/write/delete         │
│  • CartridgeBuilder                 │
└─────────────┬───────────────────────┘
              │
┌─────────────▼───────────────────────┐
│      Core Implementation            │
│  ┌──────────────────────────────┐  │
│  │ SQLite VFS (29 unsafe blocks)│  │
│  ├──────────────────────────────┤  │
│  │ IAM Policy Engine            │  │
│  ├──────────────────────────────┤  │
│  │ Snapshot Manager (CoW)       │  │
│  ├──────────────────────────────┤  │
│  │ B-tree Catalog               │  │
│  ├──────────────────────────────┤  │
│  │ Hybrid Allocator             │  │
│  │  • Bitmap (small files)      │  │
│  │  • Extent (large files)      │  │
│  ├──────────────────────────────┤  │
│  │ ARC Buffer Pool              │  │
│  ├──────────────────────────────┤  │
│  │ Compression (LZ4/Zstd)       │  │
│  ├──────────────────────────────┤  │
│  │ Encryption (AES-256-GCM)     │  │
│  ├──────────────────────────────┤  │
│  │ 4KB Page Layer               │  │
│  └──────────────────────────────┘  │
└─────────────────────────────────────┘
```

---

## Testing

### Run Tests

```bash
# All tests (242 passing)
cargo test

# With output
cargo test -- --nocapture

# Run benchmarks (8 suites)
cargo bench

# With logging
RUST_LOG=debug cargo test
```

### Test Coverage

- **242 tests** passing
- **8 benchmarks** (allocation, buffer pool, compression, IAM, pager, snapshots, VFS, mixed workload)
- **Comprehensive integration tests** for all features

**Testing Plans:**
- [TESTING_PLAN.md](TESTING_PLAN.md) - Comprehensive test plan (Phase 1-5)
- [PERFORMANCE_VERIFICATION.md](PERFORMANCE_VERIFICATION.md) - Performance claim verification

---

## Documentation

**Specifications:**
- [CARTRIDGE_SPECIFICATION.md](CARTRIDGE_SPECIFICATION.md) - Complete v0.2 format specification
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) - Implementation architecture
- [docs/performance.md](docs/performance.md) - Benchmark results

**Guides:**
- [LIBRARY_USAGE.md](LIBRARY_USAGE.md) - Comprehensive usage guide
- [DYNAMIC_PLAN.md](DYNAMIC_PLAN.md) - Auto-growth implementation
- [DYNAMIC_PLAN_STATUS.md](DYNAMIC_PLAN_STATUS.md) - Feature status

**API Documentation:**
- [docs.rs/cartridge-rs](https://docs.rs/cartridge-rs)

---

## Ecosystem

Cartridge is part of the Blackfall Labs technology stack:

- **[SAM](https://github.com/blackfall-labs/sam)** - Offline AI assistant for crisis centers
- **[CML](https://github.com/blackfall-labs/content-markup-language)** - Semantic content format
- **[Engram](https://github.com/blackfall-labs/engram)** - Signed archives with Git integration
- **[Cartridge-S3](../cartridge-s3-rs)** - S3-compatible HTTP API for Cartridge
- **[Byte Punch](https://github.com/blackfall-labs/byte-punch)** - Profile-aware compression
- **[Research Engine](../research-engine)** - Tauri desktop research application

---

## Status

**Production Ready** - v0.2.4

**Features:**
- ✅ Auto-growth containers
- ✅ Slug/title manifest system
- ✅ SQLite VFS integration
- ✅ Compression & encryption
- ✅ Snapshots & IAM policies
- ✅ Engram freezing
- ✅ Audit logging

**Limitations:**
- Single-page catalog (10,000-50,000 files max)
- Single-process only (no multi-process locking)
- No WAL (crash recovery)
- No defragmentation

**Future (v0.3 - Q2 2026):**
- Multi-page B-tree catalog (scale to millions of files)
- Binary serialization (replace JSON)
- WAL for crash recovery
- Defragmentation and compaction
- MVCC for concurrent access
- Multi-process support

---

## Use Cases

### Embedded Systems
```rust
// Raspberry Pi 5 with limited resources
let cart = Cartridge::create("sensor-data", "Sensor Data")?;
cart.write("temperature.csv", b"timestamp,temp\n...")?;
```

### Offline Applications
```rust
// Offline-first mobile app
let cart = Cartridge::create("offline-cache", "Offline Cache")?;
cart.write("articles/article1.html", html_content)?;
```

### Data Distribution
```rust
// Distribute datasets with verification
let cart = Cartridge::create("dataset", "ML Dataset")?;
cart.write("train/images/001.jpg", image_data)?;
cart.freeze_to_engram("dataset-v1.0.eng")?;  // Immutable, signed
```

### SQLite + Files Together
```rust
// Database + files in single container
register_vfs()?;
let conn = Connection::open("file:app.db?vfs=cartridge&cartridge=myapp.cart")?;
// Also store: /uploads/file.pdf, /config/settings.json
```

### Compliance Archives
```rust
// Tamper-proof audit logs
let cart = Cartridge::create("audit-2025", "Audit Logs 2025")?;
cart.enable_audit_logging();
cart.write("logs/january.log", log_data)?;
cart.freeze_to_engram("audit-2025.eng")?;  // Immutable, signed
```

---

## Security

**Cryptography:**
- AES-256-GCM for encryption
- Ed25519 for signatures (via Engram freezing)
- SHA-256 for checksums
- Hardware acceleration (AES-NI) on modern CPUs

**Access Control:**
- IAM policies (AWS-style)
- Wildcard patterns (`/data/**`)
- Deny precedence over Allow

**Best Practices:**
- ⚠️ **Never commit encryption keys to git**
- ⚠️ **Never hardcode keys in source**
- ✅ Use environment variables or hardware keys
- ✅ Rotate keys periodically
- ✅ Enable audit logging for compliance

---

## Contributing

Contributions are welcome! Please ensure:

1. All tests pass: `cargo test`
2. Code is formatted: `cargo fmt`
3. Clippy is happy: `cargo clippy -- -D warnings`
4. Documentation is updated
5. Add tests for new features

**Testing Requirements:**
- Minimum 80% code coverage
- All unsafe code must have tests
- Performance-critical code must have benchmarks

---

## License

Licensed under either of:

- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

---

## Acknowledgments

**Inspired by:**
- SQLite (testing methodology, corruption handling)
- LMDB (memory-mapped I/O, crash recovery)
- Git (content-addressable storage, cryptographic verification)
- RocksDB (LSM trees, compaction)
- ZFS/Btrfs (checksums, self-healing)

**Built with:**
- [engram-rs](https://github.com/blackfall-labs/engram) - Immutable archive format
- [rusqlite](https://github.com/rusqlite/rusqlite) - SQLite bindings
- [lz4_flex](https://github.com/PSeitz/lz4_flex) - LZ4 compression
- [zstd](https://github.com/gyscos/zstd-rs) - Zstandard compression
- [aes-gcm](https://github.com/RustCrypto/AEADs) - AES-256-GCM encryption

---

**Cartridge**: Mutable containers that grow with your data.

**Questions?** Open an issue: https://github.com/blackfall-labs/cartridge/issues
