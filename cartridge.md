# Cartridge

**Mutable containers that grow with your data** 📦

[![Crates.io](https://img.shields.io/crates/v/cartridge-rs)](https://crates.io/crates/cartridge-rs)
[![Documentation](https://docs.rs/cartridge-rs/badge.svg)](https://docs.rs/cartridge-rs)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE-MIT)
[![Tests](https://img.shields.io/badge/tests-234%20passing-brightgreen)]()

> **Production Ready** · v0.2.4 · 17.9 GiB/s · Offline-First · SQLite VFS · Crypto Verified

---

## What is Cartridge?

Cartridge is a **high-performance storage system** for applications that need:

- 📦 **Auto-growing containers** - Start at 12KB, expand automatically
- 🌐 **Offline-first design** - Zero network dependencies
- 🗄️ **SQLite databases inside** - Run databases within containers
- 📸 **Immutable snapshots** - Point-in-time backups
- 🔐 **Cryptographic freezing** - Convert to signed archives
- ⚡ **High Performance** - 17.9 GiB/s read, 9.4 GiB/s write
- 🛡️ **Battle-Tested** - 234 tests covering security, performance, and reliability

Perfect for **embedded systems, offline apps, data distribution, and compliance**.

---

## Quick Start

### Installation

```toml
[dependencies]
cartridge-rs = "0.2.4"
anyhow = "1.0.100" 
```

Anyhow is a required dependency of cratridge-rs

### Hello World

```rust
use cartridge_rs::Cartridge;

fn main() -> anyhow::Result<()> {
    // Create a container (creates "notes.cart" file)
    let mut cart = Cartridge::create("notes", "My Notes")?;

    // Write some files
    cart.write("/readme.txt", b"Hello, Cartridge!")?;
    cart.write("/docs/guide.md", b"# Getting Started\n...")?;

    // Read them back
    let content = cart.read("/readme.txt")?;
    println!("{}", String::from_utf8_lossy(&content));

    // List everything
    for file in cart.list("/")? {
        println!("📄 {}", file);
    }

    Ok(())
}
```

**That's it!** The container automatically grows from 12KB as you add data.

---

## Key Features

### 🚀 High Performance

Verified benchmarks on real hardware:

| Operation          | Throughput     | Notes                      |
| ------------------ | -------------- | -------------------------- |
| **Read**           | 17.91 GiB/s    | Faster than most SSDs      |
| **Write**          | 9.41 GiB/s     | Sustained performance      |
| **LZ4 Decompress** | 38.12 GiB/s    | 4x faster than compression |
| **Allocation**     | 173k blocks/ms | Extent allocator           |

### 📦 Auto-Growth

**No capacity planning needed.**
Containers start tiny (12KB) and double when needed:

```
12KB → 24KB → 48KB → 96KB → 192KB → 384KB → ... → ∞
```

```rust
// Just write data - container grows automatically!
let mut cart = Cartridge::create("data", "My Data")?;

for i in 0..1000 {
    let big_file = vec![0u8; 100_000];  // 100KB each
    cart.write(&format!("/file{}.bin", i), &big_file)?;
}
// Container grew from 12KB to 400MB automatically ✨
```

### 🗄️ SQLite Inside Containers

Run **entire databases** inside a single `.cart` file:

```rust
use rusqlite::{Connection, OpenFlags};
use cartridge_rs::vfs::register_vfs;

// Register the VFS (one-time setup)
register_vfs()?;

// Open SQLite database INSIDE the container
let conn = Connection::open_with_flags(
    "file:/mydb.db?vfs=cartridge&cartridge=myapp.cart",
    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
)?;

// Use SQLite normally
conn.execute("CREATE TABLE users (id INTEGER, name TEXT)", [])?;
conn.execute("INSERT INTO users VALUES (1, 'Alice')", [])?;

// Query it
let name: String = conn.query_row(
    "SELECT name FROM users WHERE id = ?1",
    [1],
    |row| row.get(0)
)?;

println!("User: {}", name);  // Output: User: Alice
```

**Why?** Single-file distribution of database + files together.

### 📸 Snapshots

Time-travel for your data:

```rust
// Save state
let snapshot1 = cart.create_snapshot("before-migration")?;

// Make changes
cart.write("/config.json", new_config)?;
cart.delete("/old-files/")?;

// Oops! Roll back
cart.restore_snapshot(snapshot1)?;

// Everything is back to before-migration state ✨
```

**Copy-on-Write** - Only changed pages are saved (90%+ space savings).

### 🔐 Security Features

**Compression:**

```rust
cart.enable_compression()?;  // LZ4 (9.77 GiB/s) or Zstd (4.87 GiB/s)
```

**Encryption:**

```rust
let key = load_key_from_secure_source()?;
cart.enable_encryption(&key)?;  // AES-256-GCM
```

**Access Control (IAM):**

```rust
use cartridge_rs::{Policy, Statement, Action, Effect};

let policy = Policy::new("read-only", vec![
    Statement {
        effect: Effect::Allow,
        actions: vec![Action::Read],
        resources: vec!["/public/**".to_string()],
    },
    Statement {
        effect: Effect::Deny,
        actions: vec![Action::Write, Action::Delete],
        resources: vec!["/**".to_string()],
    },
]);

cart.set_policy(policy)?;

// Now writes are blocked, only reads allowed
cart.write("/test.txt", b"data")?;  // ❌ Error: Access denied
```

**Immutable Archives:**

```rust
// Freeze container to immutable, cryptographically signed archive
cart.freeze_to_engram("archive-v1.0.eng")?;
```

---

## Use Cases

### 📱 Offline-First Mobile Apps

```rust
// Perfect for apps that work without internet
let mut cache = Cartridge::create("app-cache", "My App Cache")?;

// Store articles for offline reading
cache.write("/articles/article1.html", html)?;
cache.write("/articles/article2.html", html)?;

// Store images
cache.write("/images/hero.jpg", jpeg_bytes)?;

// Works completely offline! 🌐❌
```

### 🎛️ Embedded Systems (Raspberry Pi, IoT)

```rust
// Logs from temperature sensors
let mut logs = Cartridge::create("sensor-logs", "Temperature Logs")?;

loop {
    let temp = read_temperature_sensor()?;
    let timestamp = SystemTime::now();

    logs.write(
        &format!("/logs/{}.json", timestamp),
        serde_json::to_vec(&temp)?.as_slice()
    )?;

    sleep(Duration::from_secs(60));
}
// Container grows automatically as logs accumulate
```

### 📊 Dataset Distribution

```rust
// Package datasets with verification
let mut dataset = Cartridge::create("ml-dataset", "Image Dataset v2")?;

// Add training data
for (i, image) in training_images.iter().enumerate() {
    dataset.write(&format!("/train/img{}.jpg", i), image)?;
}

// Add metadata
dataset.write("/metadata.json", metadata_json)?;

// Freeze to immutable, signed archive
dataset.freeze_to_engram("ml-dataset-v2.0.eng")?;

// Recipients can verify the Ed25519 signature 🔐
```

### 🏢 Compliance & Audit Logs

```rust
// Tamper-proof audit logs
let mut audit = Cartridge::create("audit-2025", "Audit Logs 2025")?;
audit.enable_audit_logging()?;

// Log operations (automatically timestamped)
audit.write("/logs/january.log", log_data)?;
audit.write("/logs/february.log", log_data)?;

// At end of year, freeze to immutable archive
audit.freeze_to_engram("audit-2025-final.eng")?;

// Eng file is cryptographically signed - any tampering is detectable ✅
```

### 🗃️ Database + Files in One Container

```rust
use rusqlite::{Connection, OpenFlags};

register_vfs()?;

// SQLite database inside container
let conn = Connection::open_with_flags(
    "file:/app.db?vfs=cartridge&cartridge=myapp.cart",
    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
)?;

conn.execute("CREATE TABLE documents (id INTEGER, path TEXT)", [])?;
conn.execute("INSERT INTO documents VALUES (1, '/uploads/doc1.pdf')", [])?;

// Also store the actual files in the same container
let mut cart = Cartridge::open("myapp.cart")?;
cart.write("/uploads/doc1.pdf", pdf_bytes)?;
cart.write("/config/settings.json", settings_json)?;

// Ship a SINGLE myapp.cart file with database + files! 📦
```

---

## Advanced Usage

### Builder Pattern

Full control over container creation:

```rust
use cartridge_rs::CartridgeBuilder;

let cart = CartridgeBuilder::new()
    .slug("my-data")                   // Filename: my-data.cart
    .title("My Application Data")      // Display name
    .path("/custom/location/my-data")  // Custom location (optional)
    .initial_blocks(10)                // Start with 40KB instead of 12KB
    .growth_threshold_percent(5)       // Grow when only 5% free (default: 10%)
    .max_blocks(1_000_000)             // Cap at ~4GB (prevents runaway growth)
    .buffer_pool_size(10_000)          // 40MB cache (default: 4MB)
    .with_audit_logging()              // Enable audit trail
    .build()?;
```

### Metadata & Inspection

```rust
let cart = Cartridge::open("myapp.cart")?;

// Basic info
println!("Slug: {}", cart.slug()?);      // "myapp"
println!("Title: {}", cart.title()?);    // "My Application"

// Storage info
let info = cart.info()?;
println!("Size: {} MB", info.size_bytes / 1_000_000);
println!("Files: {}", info.file_count);
println!("Free: {}%", info.free_percentage);

// File metadata
let meta = cart.metadata("/myfile.txt")?;
println!("Size: {} bytes", meta.size);
println!("Created: {}", meta.created);
println!("Modified: {}", meta.modified);
```

### Snapshot Management

```rust
// Create multiple snapshots
let snap1 = cart.create_snapshot("v1.0")?;
let snap2 = cart.create_snapshot("v1.1")?;
let snap3 = cart.create_snapshot("v2.0-beta")?;

// List all snapshots
for snapshot in cart.list_snapshots()? {
    println!("{}: {} ({})",
        snapshot.id,
        snapshot.name,
        snapshot.created
    );
}

// Restore any version
cart.restore_snapshot(snap1)?;  // Back to v1.0

// Delete old snapshots
cart.delete_snapshot(snap2)?;
```

---

## Architecture

Cartridge is built with a clean, layered architecture:

```
┌─────────────────────────────────────────────────────────────┐
│                    Public API (lib.rs)                       │
│  Cartridge::create() · read() · write() · delete() · ...    │
└────────────────────────┬────────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────────┐
│                   Core Implementation                        │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ SQLite VFS Layer (vfs/)                                │ │
│  │ • 29 unsafe FFI blocks                                 │ │
│  │ • 19 tests (100 concurrent connections tested)         │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ IAM Policy Engine (iam/)                               │ │
│  │ • AWS-style policies                                   │ │
│  │ • 1M+ evaluations/sec (cached)                         │ │
│  │ • Path normalization (fixes security bugs)             │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ Snapshot Manager (snapshot/)                           │ │
│  │ • Copy-on-Write (CoW)                                  │ │
│  │ • Point-in-time backups                                │ │
│  │ • 90%+ space savings                                   │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ B-Tree Catalog (catalog/)                              │ │
│  │ • Path → FileMetadata                                  │ │
│  │ • O(log n) lookups                                     │ │
│  │ • JSON serialized (v0.2), binary (v0.3)                │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ Hybrid Allocator (allocator/)                          │ │
│  │ • Bitmap: <256KB files (24k blocks/ms)                 │ │
│  │ • Extent: ≥256KB files (173k blocks/ms, 301x faster)   │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ ARC Buffer Pool (buffer_pool/)                         │ │
│  │ • Adaptive Replacement Cache                           │ │
│  │ • 66% hit rate (random), 90%+ (80/20 workload)         │ │
│  │ • 164μs adaptation time                                │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ Compression Layer (compression/)                       │ │
│  │ • LZ4: 9.77 GiB/s compress, 38.12 GiB/s decompress     │ │
│  │ • Zstd: 4.87 GiB/s compress, ~5.64 GiB/s decompress    │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ Encryption Layer (encryption/)                         │ │
│  │ • AES-256-GCM                                          │ │
│  │ • Hardware acceleration (AES-NI)                       │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ 4KB Page Layer (pager/)                                │ │
│  │ • Fixed-size pages (optimal for FS and DBs)            │ │
│  │ • SHA-256 checksums (optional)                         │ │
│  └────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────┘
```

---

## Testing

### Test Suite

**234 tests passing (100%)** across 6 test phases:

```bash
# Run all tests
cargo test

# Run specific test suites
cargo test --test corruption_detection    # Phase 1: Data integrity
cargo test --test concurrent_stress        # Phase 2: Concurrency
cargo test --test performance_benchmarks   # Phase 3: Performance
cargo test --test snapshot_advanced        # Phase 4: Advanced features
cargo test --test security_iam_bypass      # Phase 5: Security
cargo test --test vfs_ffi_integration      # Phase 6: VFS FFI

# With output
cargo test -- --nocapture
```

### Test Coverage

| Phase       | Tests | What It Tests                                                  |
| ----------- | ----- | -------------------------------------------------------------- |
| **Phase 1** | 26    | Data integrity, corruption detection, crash recovery           |
| **Phase 2** | 26    | Concurrency (12 threads), VFS multi-conn, snapshot consistency |
| **Phase 3** | 8     | Performance, auto-growth, 100GB scale, fragmentation           |
| **Phase 4** | 17    | Snapshots, audit logging, engram freezing                      |
| **Phase 5** | 24    | IAM security (2 CVEs fixed!), memory safety, encryption tests  |
| **Phase 6** | 19    | VFS FFI (29 unsafe blocks), 100 concurrent SQLite connections  |
| **Engram**  | 114   | Integration tests, freeze validation, VFS tests                |

**Critical Security Fixes:**

- ✅ **CVE-001**: IAM path traversal (`/public/../private/secret.txt` blocked)
- ✅ **CVE-002**: IAM glob patterns (`*.txt` now works correctly)

### Benchmarks

```bash
# Run performance benchmarks
cargo bench

# Specific benchmarks
cargo bench --bench allocation_performance
cargo bench --bench buffer_pool_performance
cargo bench --bench compression_analysis
```

---

## Performance

### Real-World Benchmarks

Tested on **AMD Ryzen 9 7950X** with NVMe SSD:

| Metric                 | Value          | Notes                           |
| ---------------------- | -------------- | ------------------------------- |
| **Read Throughput**    | 17.91 GiB/s    | Mean, 64KB blocks               |
| **Write Throughput**   | 9.41 GiB/s     | Mean, 64KB blocks               |
| **LZ4 Compress**       | 9.77 GiB/s     | Real-time compression           |
| **LZ4 Decompress**     | 38.12 GiB/s    | 4x faster than compress         |
| **Zstd Compress**      | 4.87 GiB/s     | Better ratios (~4-5x)           |
| **Allocation (small)** | 24k blocks/ms  | Bitmap allocator                |
| **Allocation (large)** | 173k blocks/ms | Extent allocator (301x faster!) |
| **Cache Hit**          | 20-255 μs      | ARC buffer pool                 |
| **Policy Eval**        | 5 μs           | IAM (cached), 1M+ evals/sec     |

_Verified: See `docs/performance.md` and `TESTING_STATUS.md`_

### Scalability

| Dimension       | v0.2.4 (Current)                | v0.3.0 (Planned)           |
| --------------- | ------------------------------- | -------------------------- |
| **Max Files**   | 10k-50k                         | Millions                   |
| **Max Size**    | 18.4 EB (filesystem limit)      | Same                       |
| **Min Size**    | 12 KB (3 pages)                 | Same                       |
| **Concurrency** | Multi-threaded (single process) | Multi-process              |
| **Catalog**     | Single 4KB page (JSON)          | Multi-page B-tree (binary) |

---

## Documentation

### Specifications

📘 **[CARTRIDGE_SPECIFICATION.md](CARTRIDGE_SPECIFICATION.md)** - Complete binary format spec (like SQLite's spec)

- Byte-level format definition
- All data structures documented
- Reproducible from spec alone

📗 **[TESTING_STATUS.md](TESTING_STATUS.md)** - Testing status & results

- All 6 phases documented
- Security fixes detailed
- Production readiness checklist

📙 **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** - Implementation guide

- Component interactions
- Design decisions
- Performance characteristics

### Guides

📕 **[LIBRARY_USAGE.md](LIBRARY_USAGE.md)** - Comprehensive usage guide
📔 **[TODO.md](TODO.md)** - Roadmap & pending features
📓 **[API Docs](https://docs.rs/cartridge-rs)** - Full API reference

---

## Ecosystem

Cartridge is part of the **Blackfall Labs** offline-first technology stack:

| Project                                   | Description                                  |
| ----------------------------------------- | -------------------------------------------- |
| **[SAM](../sam)**                         | Offline AI assistant for crisis call centers |
| **[Engram](../engram-rs)**                | Immutable archives with Ed25519 signatures   |
| **[Cartridge-S3](../cartridge-rs-s3-rs)** | S3-compatible HTTP API for Cartridge         |
| **[CML](../content-markup-language)**     | Semantic content markup format               |
| **[BytePunch](../bytepunch-rs)**          | Profile-aware compression (40-70% ratios)    |
| **[Research Engine](../research-engine)** | Tauri desktop research application           |

All projects share the **offline-first, cryptographically verified, privacy-first** philosophy.

---

## Roadmap

### v0.2.4 (Current) ✅

- ✅ Auto-growth containers
- ✅ SQLite VFS integration
- ✅ Compression & encryption (AES-256-GCM)
- ✅ Snapshots & IAM policies
- ✅ 234 tests passing (100% coverage)

### v0.3.0 (Q2 2026) 🚧

- Multi-page B-tree catalog (millions of files)
- Binary serialization (replace JSON)
- Write-Ahead Log (WAL) for crash recovery
- Defragmentation & compaction
- Multi-process support (shared memory locks)
- MVCC for concurrent access

### Future Ideas 💡

- Distributed synchronization (CRDTs)
- Incremental snapshots (delta compression)
- Cloud backends (S3, Azure, GCS)
- FUSE filesystem integration
- WebAssembly support

---

## FAQ

### How is this different from SQLite?

SQLite is a **relational database**. Cartridge is a **file container** with optional SQLite support.

| Feature            | SQLite                                                           | Cartridge       |
| ------------------ | ---------------------------------------------------------------- | --------------- |
| Purpose            | Relational database                                              | File container  |
| Data Model         | Tables, SQL                                                      | Files, paths    |
| Auto-Growth        | ✅ Yes                                                           | ✅ Yes          |
| Immutable Archives | ❌ No                                                            | ✅ Yes (Engram) |
| IAM Policies       | ❌ No                                                            | ✅ Yes          |
| Snapshots          | ❌ No                                                            | ✅ Yes          |
| **Use Together?**  | **✅ Yes! SQLite databases can run INSIDE Cartridge containers** |

### How is this different from ZIP/TAR?

ZIP/TAR are **archive formats**. Cartridge is a **mutable container**.

| Feature       | ZIP/TAR                                | Cartridge                    |
| ------------- | -------------------------------------- | ---------------------------- |
| Mutability    | ❌ Immutable (recreate entire archive) | ✅ Mutable (update in-place) |
| Auto-Growth   | ❌ No                                  | ✅ Yes                       |
| Snapshots     | ❌ No                                  | ✅ Yes                       |
| SQLite VFS    | ❌ No                                  | ✅ Yes                       |
| Encryption    | ⚠️ Limited (ZIP only)                  | ✅ AES-256-GCM               |
| Random Access | ⚠️ Slow                                | ✅ Fast (4KB pages)          |

### What about security?

**Encryption:** AES-256-GCM with hardware acceleration (AES-NI)
**Signatures:** Ed25519 (via Engram freezing)
**Access Control:** IAM policies (AWS-style)
**Checksums:** SHA-256 per page (optional)

**Security Audits:** 19 security tests, 2 CVEs fixed during development.

### What's the performance like?

**Very fast.** 17.9 GiB/s reads, 9.4 GiB/s writes on modern hardware.

Faster than most SSDs for cached reads. Comparable to native filesystem for uncached access.

### Can I use this in production?

**Yes!** v0.2.4 is production-ready with 234 passing tests (100% coverage).

**Known Limitations:**

- Single-page catalog (10k-50k files max)
- Single-process only (no multi-process locking)
- No WAL (v0.3 will add)

For most applications, these are not blockers.

### How do I contribute?

1. Fork the repo
2. Create a branch (`git checkout -b feature/amazing-feature`)
3. Make changes
4. Run tests (`cargo test`)
5. Run formatter (`cargo fmt`)
6. Run clippy (`cargo clippy -- -D warnings`)
7. Commit (`git commit -m 'Add amazing feature'`)
8. Push (`git push origin feature/amazing-feature`)
9. Open a Pull Request

We welcome contributions! 🎉

---

## License

Licensed under your choice of:

- **MIT License** ([LICENSE-MIT](LICENSE-MIT))
- **Apache License 2.0** ([LICENSE-APACHE](LICENSE-APACHE))

---

## Credits

**Created by:** [Blackfall Labs](https://github.com/blackfall-labs)
**Inspired by:** SQLite, LMDB, Git, RocksDB, ZFS
**Built with:** Rust 🦀

**Special Thanks:**

- [rusqlite](https://github.com/rusqlite/rusqlite) - SQLite bindings
- [lz4_flex](https://github.com/PSeitz/lz4_flex) - Fast LZ4
- [zstd-rs](https://github.com/gyscos/zstd-rs) - Zstandard
- [aes-gcm](https://github.com/RustCrypto/AEADs) - AES encryption
- [engram-rs](../engram-rs) - Immutable archives

---

<div align="center">

**Questions? Issues? Ideas?**

[Open an Issue](https://github.com/blackfall-labs/cartridge-rs/issues) · [Read the Docs](https://docs.rs/cartridge-rs) · [View Examples](examples/)

**⭐ Star us on GitHub if you find this useful!**

</div>
