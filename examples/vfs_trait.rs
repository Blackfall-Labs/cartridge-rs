//! VFS Trait Example
//!
//! Demonstrates the Virtual Filesystem trait which provides a unified
//! interface for working with different storage backends.
//!
//! Run with: cargo run --example vfs_trait

use cartridge_rs::{Cartridge, Vfs};

/// Generic function that works with any VFS implementation
fn analyze_storage<V: Vfs>(vfs: &V, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Analyzing storage at '{}' ===\n", path);

    let entries = vfs.list_entries(path)?;

    let mut total_files = 0;
    let mut total_dirs = 0;
    let mut total_size = 0u64;
    let mut total_compressed = 0u64;

    println!("Contents:");
    for entry in &entries {
        if entry.is_dir {
            println!("  📁 {}/", entry.name);
            total_dirs += 1;
        } else {
            let size = entry.size.unwrap_or(0);
            let compressed = entry.compressed_size.unwrap_or(size);
            let compression_ratio = if size > 0 {
                (compressed as f64 / size as f64) * 100.0
            } else {
                100.0
            };

            println!(
                "  📄 {} ({} bytes, {} bytes on disk, {:.1}%)",
                entry.name, size, compressed, compression_ratio
            );
            total_files += 1;
            total_size += size;
            total_compressed += compressed;
        }
    }

    println!();
    println!("Summary:");
    println!("  Files: {}", total_files);
    println!("  Directories: {}", total_dirs);
    println!("  Total size: {} bytes", total_size);
    println!("  Size on disk: {} bytes", total_compressed);

    if total_size > 0 {
        let overall_ratio = (total_compressed as f64 / total_size as f64) * 100.0;
        println!("  Overall compression: {:.1}%", overall_ratio);
        println!(
            "  Space saved: {} bytes ({:.1}%)",
            total_size.saturating_sub(total_compressed),
            100.0 - overall_ratio
        );
    }

    Ok(())
}

/// Process files using VFS trait (works with any backend)
fn process_files<V: Vfs>(vfs: &mut V, prefix: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Processing files in '{}' ===\n", prefix);

    let entries = vfs.list_entries(prefix)?;

    for entry in entries {
        if !entry.is_dir {
            let content = vfs.read(&entry.path)?;
            println!("  Processed: {} ({} bytes)", entry.path, content.len());
        }
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cartridge VFS Trait Example ===\n");

    // Create a Cartridge container
    let mut cart = Cartridge::create("vfs-demo", "VFS Demo Container")?;

    // Add some files
    println!("Creating test files...");
    cart.write("documents/readme.txt", b"Welcome to the VFS trait!")?;
    cart.write(
        "documents/guide.md",
        b"# Guide\n\nThe VFS trait provides a unified interface for storage backends.",
    )?;
    cart.write("config/settings.json", br#"{"theme": "dark"}"#)?;
    cart.write("data/values.txt", b"1,2,3,4,5")?;

    // Add larger file to show compression
    let large_text = "AAAA".repeat(1000); // Highly compressible
    cart.write("data/large.txt", large_text.as_bytes())?;

    println!("✓ Created test files\n");

    // Use the VFS trait for generic operations
    println!("=== Using VFS Trait (Generic Code) ===\n");

    // Analyze entire container
    analyze_storage(&cart, "")?;

    // Analyze specific directory
    analyze_storage(&cart, "documents")?;

    // Process files generically
    process_files(&mut cart, "config")?;

    // Demonstrate that VFS methods work exactly like direct methods
    println!("\n=== VFS Methods vs Direct Methods ===\n");

    // Via VFS trait
    let vfs_entries = Vfs::list_entries(&cart, "documents")?;
    println!("Via VFS trait: {} entries", vfs_entries.len());

    // Direct method
    let direct_entries = cart.list_entries("documents")?;
    println!("Via direct method: {} entries", direct_entries.len());

    assert_eq!(vfs_entries.len(), direct_entries.len());
    println!("✓ Both methods return the same results!");

    // Show compressed_size field
    println!("\n=== Compression Analysis ===\n");

    let entries = cart.list_entries("data")?;
    for entry in entries {
        if !entry.is_dir {
            if let (Some(size), Some(compressed)) = (entry.size, entry.compressed_size) {
                let ratio = (compressed as f64 / size as f64) * 100.0;
                let saved = size.saturating_sub(compressed);
                println!("File: {}", entry.name);
                println!("  Logical size: {} bytes", size);
                println!("  Physical size: {} bytes", compressed);
                println!("  Compression ratio: {:.1}%", ratio);
                println!("  Space saved: {} bytes", saved);
            }
        }
    }

    println!("\n=== Example Complete ===");
    println!("\nKey Benefits:");
    println!("  • VFS trait allows generic code that works with any backend");
    println!("  • compressed_size field shows actual space usage");
    println!("  • Same API for Cartridge, Engram, or future backends");

    // Cleanup
    std::fs::remove_file("vfs-demo.cart")?;
    println!("\n(Cleaned up vfs-demo.cart)");

    Ok(())
}
