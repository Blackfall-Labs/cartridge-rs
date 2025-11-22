//! Basic Cartridge Usage Example
//!
//! This example demonstrates the core functionality of the Cartridge library:
//! - Creating archives
//! - Writing and reading files
//! - Directory operations
//! - Metadata inspection
//!
//! Run with: cargo run --example basic_usage -p cartridge-core

use cartridge_core::{Cartridge, Result};

fn main() -> Result<()> {
    println!("=== Cartridge Basic Usage Example ===\n");

    // Create a new archive (10,000 blocks = ~40MB)
    println!("1. Creating new archive 'example.cart'...");
    let mut cart = Cartridge::create("example.cart", 10000)?;
    println!("   ✓ Archive created with 10,000 blocks\n");

    // Write some files
    println!("2. Writing files...");
    cart.create_file("documents/readme.txt", b"Welcome to Cartridge!")?;
    cart.create_file("documents/notes.txt", b"High-performance archive format")?;
    cart.create_file("data/config.json", br#"{"version": "1.0"}"#)?;
    println!("   ✓ Written 3 files\n");

    // Read a file
    println!("3. Reading 'documents/readme.txt'...");
    let content = cart.read_file("documents/readme.txt")?;
    println!("   Content: {}", String::from_utf8_lossy(&content));
    println!("   Size: {} bytes\n", content.len());

    // List directory contents
    println!("4. Listing 'documents/' directory...");
    let entries = cart.list_dir("documents")?;
    println!("   Found {} entries:", entries.len());
    for entry in &entries {
        println!("   - {}", entry);
    }
    println!();

    // Get file metadata
    println!("5. Getting metadata for 'documents/readme.txt'...");
    let metadata = cart.metadata("documents/readme.txt")?;
    println!("   Size: {} bytes", metadata.size);
    println!("   Type: {:?}", metadata.file_type);
    println!("   Blocks: {}\n", metadata.blocks.len());

    // Get archive statistics
    println!("6. Archive statistics...");
    let stats = cart.stats();
    println!("   Total blocks: {}", stats.total_blocks);
    println!("   Used blocks: {}", stats.used_blocks);
    println!("   Free blocks: {}", stats.free_blocks);
    println!("   Fragmentation: {:.2}%\n", stats.fragmentation * 100.0);

    // Update a file
    println!("7. Updating 'documents/readme.txt'...");
    cart.write_file("documents/readme.txt", b"Cartridge: High-performance mutable archives!")?;
    let updated = cart.read_file("documents/readme.txt")?;
    println!("   New content: {}", String::from_utf8_lossy(&updated));
    println!();

    // Check if file exists
    println!("8. Checking file existence...");
    let exists = cart.exists("documents/readme.txt")?;
    println!("   'documents/readme.txt' exists: {}", exists);
    let missing = cart.exists("missing.txt")?;
    println!("   'missing.txt' exists: {}\n", missing);

    // Delete a file
    println!("9. Deleting 'documents/notes.txt'...");
    cart.delete_file("documents/notes.txt")?;
    println!("   ✓ File deleted\n");

    // Verify deletion
    println!("10. Verifying deletion...");
    match cart.read_file("documents/notes.txt") {
        Ok(_) => println!("    ✗ File still exists!"),
        Err(_) => println!("    ✓ File successfully deleted"),
    }
    println!();

    // Create nested directories
    println!("11. Creating nested directory structure...");
    cart.create_dir("projects/rust/examples")?;
    cart.create_file("projects/rust/examples/hello.rs", b"fn main() { println!(\"Hello!\"); }")?;
    println!("    ✓ Created nested structure\n");

    // Flush changes to disk
    println!("12. Flushing changes to disk...");
    cart.flush()?;
    println!("    ✓ All changes persisted\n");

    println!("=== Example Complete ===");
    println!("\nArchive 'example.cart' created successfully!");
    println!("You can now:");
    println!("  - Open it with: Cartridge::open(\"example.cart\")");
    println!("  - Inspect it with tools");
    println!("  - Use it as a SQLite VFS");

    // Clean up
    drop(cart);
    std::fs::remove_file("example.cart")?;
    println!("\n(Cleaned up example.cart)");

    Ok(())
}
