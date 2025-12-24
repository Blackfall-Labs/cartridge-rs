# Cartridge

High-performance mutable container format with auto-growth, SQLite VFS support, and advanced features.

## Overview

Cartridge is a production-ready storage system designed for high-performance mutable containers with:

- **Auto-growing containers** - Start at 12KB, grow automatically as needed
- **Mutable storage** - In-place modifications without rebuilding
- **SQLite VFS integration** - Run databases directly inside containers
- **Advanced features** - Compression, encryption, snapshots, IAM policies
- **Engram freezing** - Convert to immutable, cryptographically signed archives

## Quick Start

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
cartridge-rs = { git = "https://github.com/manifest-humanity/cartridge" }
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

### Auto-Growth Example

```rust
// No capacity planning needed!
let mut cart = Cartridge::create("my-data", "My Data")?;

// Add 100KB file - container automatically grows
let large_data = vec![0u8; 100_000];
cart.write("large.bin", &large_data)?;

// Container grew from 12KB to whatever was needed
```

### Advanced Configuration

```rust
use cartridge_rs::CartridgeBuilder;

let cart = CartridgeBuilder::new()
    .slug("my-data")
    .title("My Data Container")
    .path("/custom/path/my-data")  // Optional: custom location
    .with_audit_logging()           // Optional: enable audit log
    .build()?;
```

## Key Concepts

### Slug vs Title

- **Slug**: Kebab-case identifier used for filenames and registry keys (e.g., `"us-constitution"`)
- **Title**: Human-readable display name (e.g., `"U.S. Constitution"`)

```rust
let cart = Cartridge::create("us-constitution", "U.S. Constitution")?;
// Creates file: us-constitution.cart
// Display name: U.S. Constitution
```

### Container vs Archive

- **Container**: Mutable Cartridge instance (this crate)
- **Archive**: Immutable Engram archive (created by freezing)

```rust
// Mutable container
let mut cart = Cartridge::create("data", "My Data")?;
cart.write("file.txt", b"mutable")?;

// Freeze to immutable archive
cart.inner_mut().freeze_to_engram("data.eng")?;
```

## Features

### Core Features

- **Fixed 4KB pages** - Optimal alignment for filesystems and databases
- **Hybrid allocator** - Bitmap (small) + extent (large) for efficiency
- **B-tree catalog** - Fast file metadata lookups
- **ARC buffer pool** - Adaptive Replacement Cache for hot data
- **Auto-growth** - Starts minimal (12KB), doubles on demand

### Compression & Encryption

```rust
// Transparent LZ4 compression
cart.inner_mut().enable_compression()?;

// AES-256-GCM encryption
cart.inner_mut().enable_encryption(key)?;
```

### Snapshots

```rust
// Create snapshot
let snapshot_id = cart.inner_mut().create_snapshot("backup-2025")?;

// Restore snapshot
cart.inner_mut().restore_snapshot(snapshot_id)?;
```

### IAM Policies

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

cart.inner_mut().set_policy(policy);
```

### SQLite VFS

```rust
use rusqlite::{Connection, OpenFlags};

// Register VFS
cartridge_rs::register_vfs(cart)?;

// Open database inside container
let conn = Connection::open_with_flags(
    "file:mydb.db?vfs=cartridge",
    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
)?;

conn.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)", [])?;
```

## Performance

**Throughput** (64KB blocks):
- Read: 18 GiB/s
- Write: 9 GiB/s

**Compression**:
- LZ4 Compression: 9.77 GiB/s
- LZ4 Decompression: 38.12 GiB/s

**Auto-growth overhead**: < 1ms per doubling

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

# Compression analysis with compressed_size field
cargo run --example compression_analysis
```

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
│  │ SQLite VFS                   │  │
│  ├──────────────────────────────┤  │
│  │ IAM Policy Engine            │  │
│  ├──────────────────────────────┤  │
│  │ Snapshot Manager             │  │
│  ├──────────────────────────────┤  │
│  │ B-tree Catalog               │  │
│  ├──────────────────────────────┤  │
│  │ Hybrid Allocator             │  │
│  │  • Bitmap (small)            │  │
│  │  • Extent (large)            │  │
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

## Testing

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run benchmarks
cargo bench

# With logging
RUST_LOG=debug cargo test
```

**Test Coverage**: 232 tests passing

## Documentation

- [LIBRARY_USAGE.md](LIBRARY_USAGE.md) - Comprehensive library usage guide
- [DYNAMIC_PLAN.md](DYNAMIC_PLAN.md) - Auto-growth implementation plan
- [DYNAMIC_PLAN_STATUS.md](DYNAMIC_PLAN_STATUS.md) - Implementation status

## Ecosystem

Cartridge is part of the Blackfall Labs technology stack:

- **[SAM](https://github.com/manifest-humanity/sam)** - Offline AI assistant for crisis centers
- **[CML](https://github.com/manifest-humanity/content-markup-language)** - Semantic content format
- **[Engram](https://github.com/manifest-humanity/engram)** - Signed archives with Git integration
- **[Byte Punch](https://github.com/manifest-humanity/byte-punch)** - Profile-aware compression
- **[Research Engine](../research-engine)** - Tauri desktop research application

## Status

**Production Ready** - v0.2.0

- ✅ Auto-growth containers
- ✅ Slug/title manifest system
- ✅ SQLite VFS integration
- ✅ Compression & encryption
- ✅ Snapshots & IAM policies
- ✅ Engram freezing

## Contributing

Contributions are welcome! Please ensure:

1. All tests pass: `cargo test`
2. Code is formatted: `cargo fmt`
3. Clippy is happy: `cargo clippy -- -D warnings`
4. Documentation is updated

## License

Licensed under either of:

- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

---

**Cartridge**: Mutable containers that grow with your data.
