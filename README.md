# Cartridge

High-performance mutable archive format with SQLite VFS support and S3-compatible API.

## Overview

Cartridge is a production-ready storage system designed for high-performance file archiving with support for:

- **Mutable archives** with in-place modifications
- **SQLite VFS integration** for running databases directly inside archives
- **S3-compatible HTTP API** for cloud-native workflows
- **Advanced features**: compression, encryption, snapshots, IAM policies

## Components

This repository contains two main crates:

### 📦 cartridge-core

The core mutable archive format with SQLite VFS support.

**Features:**
- Fixed 4KB pages for optimal alignment
- Hybrid allocator (bitmap + extent)
- B-tree catalog for file metadata
- ARC buffer pool (Adaptive Replacement Cache)
- LZ4/Zstd compression (transparent)
- AES-256-GCM encryption
- IAM policies with wildcard matching
- Copy-on-write snapshots
- Engram freezing (mutable → immutable)

**Performance:**
- Read: 18 GiB/s (64KB blocks)
- Write: 9 GiB/s (64KB blocks)
- LZ4 Compression: 9.77 GiB/s
- LZ4 Decompression: 38.12 GiB/s

[Read more →](crates/cartridge-core/README.md)

### 🌐 cartridge-s3

S3-compatible HTTP API for Cartridge storage.

**Features:**
- Full S3 bucket operations (create, delete, list, head)
- Full S3 object operations (put, get, delete, head, list, copy)
- Multipart upload support (AWS CLI compatible)
- Bulk delete (up to 1000 keys)
- AWS Signature V4 authentication
- Feature fuses (versioning, ACL, SSE modes)

**AWS CLI Compatible:**
```bash
aws s3 --endpoint-url=http://localhost:9000 cp file.txt s3://my-bucket/
```

[Read more →](crates/cartridge-s3/README.md)

## Quick Start

### Installation

```bash
git clone https://github.com/manifest-humanity/cartridge.git
cd cartridge
cargo build --release
```

### Using cartridge-core

```rust
use cartridge_core::Cartridge;

// Create a new archive
let mut cart = Cartridge::create("data.cart")?;

// Write files
cart.write_file("documents/report.txt", b"Hello, World!")?;

// Read files
let content = cart.read_file("documents/report.txt")?;

// Create snapshots
let snapshot = cart.create_snapshot("backup-2025")?;

// Use as SQLite VFS
let conn = Connection::open_with_flags(
    "file:data.db?vfs=cartridge",
    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
)?;
```

### Running cartridge-s3 Server

```bash
# Start the S3 server
./target/release/cartridge-s3-server \
    --bind 127.0.0.1:9000 \
    --storage-path ./data

# Use with AWS CLI
aws s3 --endpoint-url=http://localhost:9000 mb s3://my-bucket
aws s3 --endpoint-url=http://localhost:9000 cp file.txt s3://my-bucket/
```

## Architecture

```
┌─────────────────────────────────────┐
│      cartridge-s3 (S3 API)         │
│  ┌──────────────────────────────┐  │
│  │ S3 Handler (hyper + s3s)     │  │
│  └──────────┬───────────────────┘  │
│             │                       │
│  ┌──────────▼───────────────────┐  │
│  │ CartridgeS3Backend           │  │
│  └──────────┬───────────────────┘  │
└─────────────┼───────────────────────┘
              │
┌─────────────▼───────────────────────┐
│      cartridge-core                 │
│  ┌──────────────────────────────┐  │
│  │ Cartridge API                │  │
│  ├──────────────────────────────┤  │
│  │ SQLite VFS                   │  │
│  ├──────────────────────────────┤  │
│  │ B-tree Catalog               │  │
│  ├──────────────────────────────┤  │
│  │ Hybrid Allocator             │  │
│  ├──────────────────────────────┤  │
│  │ ARC Buffer Pool              │  │
│  ├──────────────────────────────┤  │
│  │ 4KB Page Layer               │  │
│  └──────────────────────────────┘  │
└─────────────────────────────────────┘
```

## Testing

```bash
# Test all components
cargo test

# Test specific crate
cargo test -p cartridge-core
cargo test -p cartridge-s3

# Run benchmarks
cargo bench -p cartridge-core

# With logging
RUST_LOG=debug cargo test
```

**Test Coverage:**
- **cartridge-core**: 192/193 tests passing (99.5%)
- **cartridge-s3**: 32/32 tests passing (100%)

## Documentation

- [Cartridge Core README](crates/cartridge-core/README.md)
- [Cartridge Core Architecture](crates/cartridge-core/ARCHITECTURE.md)
- [Cartridge Core Specification](crates/cartridge-core/SPECIFICATION.md)
- [Cartridge S3 README](crates/cartridge-s3/README.md)
- [Performance Benchmarks](crates/cartridge-core/performance.md)

## Status

**Production Ready** - Both components are stable and ready for production use:

- ✅ **cartridge-core v0.1.0** - Phase 7 Complete
- ✅ **cartridge-s3 v0.2.0** - Feature Fuses Complete

## Dependencies

- **Engram**: Git-based signed archives ([manifest-humanity/engram](https://github.com/manifest-humanity/engram))
- Rust 2021 edition
- See individual crate `Cargo.toml` files for complete dependency lists

## License

Licensed under either of:

- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)

at your option.

## Contributing

Contributions are welcome! Please ensure:

1. All tests pass (`cargo test`)
2. Code is formatted (`cargo fmt`)
3. Clippy is happy (`cargo clippy`)
4. Documentation is updated

## Ecosystem

Cartridge is part of the Manifest Humanity technology stack:

- [SAM (Societal Advisory Module)](https://github.com/manifest-humanity/sam) - Offline AI assistant
- [CML (Content Markup Language)](https://github.com/manifest-humanity/content-markup-language) - Semantic content format
- [Engram](https://github.com/manifest-humanity/engram) - Signed archives with Git integration
- [Byte Punch](https://github.com/manifest-humanity/byte-punch) - Profile-aware compression

---

**Cartridge**: High-performance archiving for the modern age.
