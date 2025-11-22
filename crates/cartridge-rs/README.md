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
    let mut cart = Cartridge::create("data.cart")?;

    // Write files (automatically creates or updates)
    cart.write("documents/report.txt", b"Hello, World!")?;
    cart.write("data/config.json", br#"{"version": "1.0"}"#)?;

    // Read files
    let content = cart.read("documents/report.txt")?;
    println!("Content: {}", String::from_utf8_lossy(&content));

    // List directory
    let files = cart.list("documents")?;
    for file in files {
        println!("Found: {}", file);
    }

    // Delete files
    cart.delete("old_file.txt")?;

    Ok(())
}
```

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
