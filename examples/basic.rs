//! Basic Cartridge Usage Example
//!
//! Demonstrates the core functionality:
//! - Creating containers with slug/title
//! - Writing and reading files
//! - Auto-growth from minimal size
//!
//! Run with: cargo run --example basic

use cartridge_rs::Cartridge;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cartridge Basic Usage ===\n");

    // Create a new container - starts at 12KB, grows automatically!
    println!("1. Creating container...");
    let mut cart = Cartridge::create("my-data", "My Data Container")?;
    println!("   ✓ Created: {} ({})", cart.title()?, cart.slug()?);
    println!();

    // Write some files
    println!("2. Writing files...");
    cart.write("documents/readme.txt", b"Welcome to Cartridge!")?;
    cart.write(
        "documents/notes.txt",
        b"High-performance mutable containers",
    )?;
    cart.write("config/settings.json", br#"{"version": "1.0"}"#)?;
    println!("   ✓ Written 3 files");
    println!();

    // Read a file back
    println!("3. Reading 'documents/readme.txt'...");
    let content = cart.read("documents/readme.txt")?;
    println!("   Content: {}", String::from_utf8_lossy(&content));
    println!();

    // List directory
    println!("4. Listing 'documents/' directory...");
    let files = cart.list("documents")?;
    println!("   Found {} entries:", files.len());
    for file in &files {
        println!("   - {}", file);
    }
    println!();

    // Update a file
    println!("5. Updating file...");
    cart.write(
        "documents/readme.txt",
        b"Cartridge: Mutable containers with auto-growth!",
    )?;
    let updated = cart.read("documents/readme.txt")?;
    println!("   New content: {}", String::from_utf8_lossy(&updated));
    println!();

    // Delete a file
    println!("6. Deleting 'documents/notes.txt'...");
    cart.delete("documents/notes.txt")?;
    println!("   ✓ File deleted");
    println!();

    // Update manifest
    println!("7. Updating manifest metadata...");
    cart.update_manifest(|manifest| {
        manifest.description = Some("Example container demonstrating basic operations".to_string());
    })?;
    let manifest = cart.read_manifest()?;
    println!("   Version: {}", manifest.version);
    println!("   Description: {:?}", manifest.description);
    println!();

    // Flush to disk
    println!("8. Flushing to disk...");
    cart.flush()?;
    println!("   ✓ All changes persisted");
    println!();

    println!("=== Example Complete ===");
    println!("\nContainer 'my-data.cart' created successfully!");
    println!("You can inspect it with: ls -lh my-data.cart");

    Ok(())
}
