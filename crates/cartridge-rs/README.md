# cartridge-rs

High-level API for the Cartridge mutable archive format.

## Overview

`cartridge-rs` provides an easy-to-use, batteries-included API for working with Cartridge archives. It wraps the low-level `cartridge-core` implementation with sensible defaults and simplified methods.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
cartridge-rs = { git = "https://github.com/manifest-humanity/cartridge", branch = "main" }
```

## Quick Start

```rust
use cartridge_rs::{Cartridge, Result};

fn main() -> Result<()> {
    // Create a new archive
    let mut cart = Cartridge::create("data", "My Data")?;

    // Write files (automatically creates or updates)
    cart.write("/documents/report.txt", b"Hello, World!")?;
    cart.write("/data/config.json", br#"{"version": "1.0"}"#)?;

    // Read files
    let content = cart.read("/documents/report.txt")?;
    println!("Content: {}", String::from_utf8_lossy(&content));

    // List directory (returns flat paths)
    let files = cart.list("/documents")?;
    for file in files {
        println!("Found: {}", file);
    }

    // Delete files
    cart.delete("/old_file.txt")?;

    Ok(())
}
```

## Rich File/Directory Metadata with Entry

**New in v0.2.0!** The `Entry` API provides rich metadata about files and directories, eliminating the need to manually parse paths and build hierarchies.

### Entry Struct

```rust
pub struct Entry {
    pub path: String,           // Full path: "/docs/readme.md"
    pub name: String,           // Just the name: "readme.md"
    pub parent: String,         // Parent directory: "/docs"
    pub is_dir: bool,           // True for directories
    pub size: Option<u64>,      // File size in bytes
    pub created: Option<u64>,   // Creation timestamp (Unix epoch)
    pub modified: Option<u64>,  // Modification timestamp
    pub content_type: Option<String>,  // MIME type
    pub file_type: FileType,    // File/Directory/Symlink
}
```

### List Entries with Metadata

```rust
use cartridge_rs::{Cartridge, Result};

fn main() -> Result<()> {
    let cart = Cartridge::open("data.cart")?;

    // Get all entries with rich metadata
    let entries = cart.list_entries("/")?;

    for entry in entries {
        if entry.is_dir {
            println!("📁 {} (directory)", entry.name);
        } else {
            let size = entry.size.unwrap_or(0);
            println!("📄 {} ({} bytes)", entry.name, size);
        }
    }

    Ok(())
}
```

### List Immediate Children Only

```rust
// List only direct children (not nested files)
let children = cart.list_children("/docs")?;

for child in children {
    println!("{} - parent: {}", child.name, child.parent);
}
```

### Check if Path is Directory

```rust
if cart.is_dir("/documents")? {
    println!("/documents is a directory");
}
```

### Migration Guide from v0.1.0

**v0.1.0 (Manual path parsing):**
```rust
let paths = cart.list("/research")?;
let mut entries = Vec::new();

for path in paths {
    let name = path.rsplit('/').next().unwrap();
    let parent = path.rsplit_once('/').map(|(p, _)| p).unwrap_or("/");
    let is_dir = paths.iter().any(|p| p.starts_with(&format!("{}/", path)));

    entries.push(MyEntry { path, name, parent, is_dir });
}
```

**v0.2.0 (Rich Entry API):**
```rust
let entries = cart.list_entries("/research")?;

for entry in entries {
    // All metadata is already parsed and available!
    println!("{} (parent: {}, size: {:?})",
        entry.name, entry.parent, entry.size);
}
```

### Benefits

✅ **No duplication** - Tree-building logic lives in one place
✅ **Rich metadata** - Get name, parent, is_dir, size, timestamps without parsing
✅ **Multiple views** - Use `list_entries()` or `list_children()` as needed
✅ **Type safety** - `Entry` struct instead of parsing strings
✅ **Future-proof** - Can add more metadata without breaking API

## Advanced Usage

### Builder Pattern

```rust
use cartridge_rs::CartridgeBuilder;

let cart = CartridgeBuilder::new()
    .path("large.cart")
    .blocks(100_000)  // ~400MB
    .with_audit_logging()
    .build()?;
```

### Access Low-Level Features

```rust
use cartridge_rs::{Cartridge, Policy};

let mut cart = Cartridge::create("data.cart")?;

// Access cartridge-core features
let policy = Policy::new("my-policy", vec![]);
cart.inner_mut().set_policy(policy);
```

## Features

- **Simple API**: Sensible defaults, clear method names
- **Rich Metadata** (v0.2.0): Entry API with parsed path components, file sizes, timestamps
- **Automatic**: Creates files/directories as needed, handles resource cleanup
- **Flexible**: Builder pattern for customization
- **Low-Level Access**: Can access `cartridge-core` when needed
- **Documented**: Comprehensive examples and API docs

## Comparison with cartridge-core

| Feature | cartridge-rs | cartridge-core |
|---------|-------------|----------------|
| Ease of Use | ⭐⭐⭐⭐⭐ | ⭐⭐ |
| Sensible Defaults | ✅ | ❌ |
| Simple Methods | ✅ (`write`, `read`) | ❌ (`write_file` vs `create_file`) |
| Low-Level Control | Via `inner()` | Full access |
| Recommended For | Most users | Power users, library authors |

## When to Use cartridge-core Instead

Use `cartridge-core` if you need:
- Custom allocator implementations
- Direct page-level access
- Fine-grained control over buffer pools
- Building your own high-level abstraction

## Documentation

- [API Docs](https://docs.rs/cartridge-rs) (run `cargo doc --open`)
- [Library Usage Guide](../../LIBRARY_USAGE.md)
- [Examples](../../examples/)

## License

Licensed under either of:

- MIT license ([LICENSE-MIT](../../LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))

at your option.
