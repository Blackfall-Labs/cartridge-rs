# Using Cartridge as a Library

This guide explains how to use Cartridge in your Rust projects as a dependency.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
# High-level API (recommended for most users)
cartridge-rs = { git = "https://github.com/blackfall-labs/cartridge-rs", branch = "main" }

# Low-level API (for advanced use cases)
# cartridge-core = { git = "https://github.com/blackfall-labs/cartridge-rs", branch = "main" }

# S3 server functionality (optional)
# cartridge-s3 = { git = "https://github.com/blackfall-labs/cartridge-rs", branch = "main" }
```

## Which Crate Should I Use?

- **`cartridge-rs`** (RECOMMENDED): High-level, batteries-included API. Sensible defaults, simple methods, automatic resource management.
- **`cartridge-core`**: Low-level primitives. Use only if you need fine-grained control over allocators, page management, or are building your own abstraction.
- **`cartridge-s3`**: S3-compatible server. Use if you need to expose cartridges over HTTP with S3 API compatibility.

### Authentication for Private Repository

Since this is a private GitHub repository, ensure you have:

1. **Git Credential Manager** configured (handles authentication automatically)
2. **GitHub Personal Access Token** with repo read access

The repository automatically uses your system's git credentials when fetching dependencies.

## Public API Overview

Cartridge exposes a clean, controlled API surface. Only the following modules are public:

### Core Modules

- `cartridge` - Main `Cartridge` struct for archive operations
- `error` - Error types (`CartridgeError`, `Result`)
- `header` - Archive header inspection (`Header`, `PAGE_SIZE`)
- `page` - Low-level page access (`Page`, `PageHeader`, `PageType`)
- `io` - Advanced file operations (`CartridgeFile`)

### Feature Modules

- `iam` - IAM policy engine for access control
- `snapshot` - Copy-on-write snapshots
- `vfs` - SQLite VFS integration
- `audit` - Audit logging
- `engram_integration` - Freeze to immutable Engram archives
- `catalog` - File metadata (`FileMetadata`, `FileType`)
- `allocator` - Block allocation strategies (for extensibility)

### Internal Modules (Not Public)

The following are **implementation details** and cannot be imported:

- `buffer_pool` - ARC caching (reserved for future use)
- `compression` - LZ4/Zstd compression logic (internal)
- `encryption` - AES-256-GCM encryption logic (internal)
- `integration_tests` - Test utilities

## Basic Usage (cartridge-rs)

### Creating and Using Archives

```rust
use cartridge_rs::{Cartridge, Result};

fn main() -> Result<()> {
    // Create a new archive (simple!)
    let mut cart = Cartridge::create("data.cart")?;

    // Write a file
    cart.write("documents/report.txt", b"Hello, World!")?;

    // Read a file
    let content = cart.read("documents/report.txt")?;
    println!("Content: {}", String::from_utf8_lossy(&content));

    // List files in a directory
    let entries = cart.list("documents")?;
    for entry in entries {
        println!("Found: {}", entry);
    }

    // Get file metadata
    let metadata = cart.metadata("documents/report.txt")?;
    println!("Size: {} bytes", metadata.size);

    // Delete a file
    cart.delete("documents/report.txt")?;

    Ok(())
}
```

### Using the Builder Pattern

```rust
use cartridge_rs::CartridgeBuilder;

fn main() -> Result<()> {
    // Custom configuration
    let mut cart = CartridgeBuilder::new()
        .path("large.cart")
        .blocks(100_000)  // ~400MB
        .with_audit_logging()
        .build()?;

    cart.write("data.txt", b"content")?;

    Ok(())
}
```

### Opening Existing Archives

```rust
use cartridge_core::Cartridge;

// Open an existing archive
let mut cart = Cartridge::open("data.cart")?;

// Read-only operations don't modify the archive
let content = cart.read_file("data/file.txt")?;
```

### Directory Operations

```rust
// Create a directory
cart.create_dir("projects/code")?;

// Create nested directories
cart.create_dir("projects/code/rust/examples")?;

// List directory contents
let entries = cart.list_dir("projects")?;
for entry in entries {
    match entry.file_type {
        FileType::File => println!("File: {}", entry.path),
        FileType::Directory => println!("Dir:  {}", entry.path),
    }
}
```

## Advanced Features

### IAM Policies

Control access with AWS-style IAM policies:

```rust
use cartridge_core::iam::{Policy, Statement, Effect, Action};

let mut cart = Cartridge::create("secure.cart")?;

// Create a policy that allows read but denies write to sensitive files
let policy = Policy::new(
    "read-only-policy",
    vec![
        Statement {
            effect: Effect::Allow,
            actions: vec![Action::ReadFile, Action::ListDir],
            resources: vec!["*".to_string()],
            conditions: vec![],
        },
        Statement {
            effect: Effect::Deny,
            actions: vec![Action::WriteFile, Action::DeleteFile],
            resources: vec!["sensitive/*".to_string()],
            conditions: vec![],
        },
    ],
);

cart.set_policy(policy)?;

// This will be denied
let result = cart.write_file("sensitive/secret.txt", b"data");
assert!(result.is_err());
```

### Snapshots

Create point-in-time snapshots:

```rust
use cartridge_core::snapshot::SnapshotManager;

let mut cart = Cartridge::create("data.cart")?;

// Write some data
cart.write_file("data.txt", b"version 1")?;

// Create a snapshot
let snapshot_id = cart.create_snapshot("backup-v1")?;

// Modify data
cart.write_file("data.txt", b"version 2")?;

// Restore from snapshot
cart.restore_snapshot(&snapshot_id)?;

let content = cart.read_file("data.txt")?;
assert_eq!(content, b"version 1");
```

### SQLite VFS Integration

Run SQLite databases directly inside archives:

```rust
use cartridge_core::vfs;
use rusqlite::{Connection, OpenFlags};

// Register the Cartridge VFS
vfs::register_vfs()?;

// Create a database inside a cartridge archive
let conn = Connection::open_with_flags(
    "file:mydb.db?vfs=cartridge&cartridge=/path/to/archive.cart",
    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
)?;

// Use SQLite normally
conn.execute(
    "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT)",
    [],
)?;

conn.execute("INSERT INTO users (name) VALUES (?1)", ["Alice"])?;

// Database is stored inside the cartridge archive!
```

### Audit Logging

Track all operations:

```rust
use cartridge_core::audit::{AuditLogger, Operation};

let mut cart = Cartridge::create("audited.cart")?;

// Enable audit logging
let logger = AuditLogger::new(1000); // Ring buffer of 1000 entries
cart.set_audit_logger(logger)?;

// Perform operations (automatically logged)
cart.write_file("data.txt", b"content")?;
cart.read_file("data.txt")?;

// Retrieve audit log
let entries = cart.audit_entries()?;
for entry in entries {
    println!("{:?}: {} on {}", entry.timestamp, entry.operation, entry.path);
}
```

### Engram Integration

Freeze mutable archives to immutable Engram archives:

```rust
use cartridge_core::engram_integration::EngramFreezer;

let cart = Cartridge::open("data.cart")?;

// Freeze to an immutable, signed Engram archive
let freezer = EngramFreezer::new();
freezer.freeze(&cart, "output.eng")?;

// The .eng file is now:
// - Immutable
// - Compressed (LZ4/Zstd)
// - Ed25519 signed
// - Suitable for distribution
```

## Error Handling

All operations return `Result<T, CartridgeError>`:

```rust
use cartridge_core::{Cartridge, CartridgeError};

match cart.read_file("data.txt") {
    Ok(content) => println!("Success: {} bytes", content.len()),
    Err(CartridgeError::FileNotFound(path)) => {
        eprintln!("File not found: {}", path);
    }
    Err(CartridgeError::IoError(e)) => {
        eprintln!("I/O error: {}", e);
    }
    Err(e) => {
        eprintln!("Other error: {}", e);
    }
}
```

## Performance Tips

### Batch Operations

```rust
// Efficient: Single transaction
cart.begin_transaction()?;
for i in 0..1000 {
    cart.write_file(&format!("file_{}.txt", i), b"data")?;
}
cart.commit_transaction()?;

// Inefficient: Individual writes
// for i in 0..1000 {
//     cart.write_file(&format!("file_{}.txt", i), b"data")?;
// }
```

### Buffer Sizing

```rust
// Use larger buffers for better performance
let mut large_data = vec![0u8; 1024 * 1024]; // 1MB
cart.write_file("large.bin", &large_data)?;
```

### Statistics

```rust
// Monitor performance
let stats = cart.stats()?;
println!("Total blocks: {}", stats.total_blocks);
println!("Free blocks: {}", stats.free_blocks);
println!("Fragmentation: {:.2}%", stats.fragmentation_ratio * 100.0);
```

## Thread Safety

`Cartridge` uses interior mutability with `Mutex` locks. It's safe to share across threads:

```rust
use std::sync::Arc;
use std::thread;

let cart = Arc::new(Cartridge::open("data.cart")?);

let handles: Vec<_> = (0..4)
    .map(|i| {
        let cart = Arc::clone(&cart);
        thread::spawn(move || {
            cart.read_file(&format!("data_{}.txt", i))
        })
    })
    .collect();

for handle in handles {
    let result = handle.join().unwrap()?;
    println!("Read {} bytes", result.len());
}
```

## Low-Level Access

For advanced use cases, you can access lower-level primitives:

### Page-Level Access

```rust
use cartridge_core::{page::Page, header::PAGE_SIZE};

// Read raw page
let page = cart.read_page(page_id)?;

// Verify checksum
assert!(page.verify_checksum());

// Access raw data
let data = &page.data[0..PAGE_SIZE];
```

### Custom Allocators

```rust
use cartridge_core::allocator::{BlockAllocator, HybridAllocator};

// Create custom allocator
let mut allocator = HybridAllocator::new(10000);

// Allocate blocks manually
let blocks = allocator.allocate(64 * 1024)?; // 64KB
println!("Allocated {} blocks", blocks.len());

// Free blocks
allocator.free(&blocks)?;
```

## Examples

See the [`examples/`](examples/) directory for complete working examples:

- `basic_usage.rs` - Creating and reading archives
- `sqlite_vfs.rs` - Using SQLite inside archives
- `iam_policies.rs` - Access control with IAM
- `snapshots.rs` - Snapshot management
- `engram_freeze.rs` - Converting to Engram archives

## API Documentation

Generate and view the complete API documentation:

```bash
cargo doc --open --no-deps -p cartridge-core
```

## Getting Help

- **Documentation**: See [`docs/`](docs/) directory
- **Issues**: [GitHub Issues](https://github.com/blackfall-labs/cartridge-rs/issues)
- **Examples**: [`examples/`](examples/) directory

## Version Compatibility

- **Rust**: 2021 edition or later
- **Minimum Rust Version**: 1.70+
- **Platform**: Windows, macOS, Linux

## What's Not Public

The following are internal implementation details and **cannot** be imported:

```rust
// ❌ These will fail to compile
use cartridge_core::buffer_pool::BufferPool;  // Error: module is private
use cartridge_core::compression::compress;     // Error: module is private
use cartridge_core::encryption::encrypt;       // Error: module is private
```

If you need functionality that seems to be missing, please open an issue to discuss whether it should be part of the public API.
