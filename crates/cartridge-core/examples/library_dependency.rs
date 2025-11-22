//! Example: Using Cartridge as a Library Dependency
//!
//! This example shows how another Rust project would use Cartridge
//! as a library dependency via git.
//!
//! In your project's Cargo.toml:
//! ```toml
//! [dependencies]
//! cartridge-core = { git = "https://github.com/manifest-humanity/cartridge", branch = "main" }
//! ```
//!
//! Run with: cargo run --example library_dependency -p cartridge-core

use cartridge_core::{Cartridge, Result, CartridgeError};

fn main() -> Result<()> {
    println!("=== Using Cartridge as a Library ===\n");

    // Example: Document archive system
    create_document_archive()?;

    // Example: Configuration storage
    create_config_store()?;

    // Example: Error handling
    demonstrate_error_handling()?;

    Ok(())
}

/// Example: Simple document archive system
fn create_document_archive() -> Result<()> {
    println!("📄 Document Archive Example");
    println!("   Creating a document management system...\n");

    let mut archive = Cartridge::create("documents.cart", 5000)?;

    // Store different document types
    let documents = vec![
        ("invoices/2025-01.pdf", b"PDF invoice data..." as &[u8]),
        ("invoices/2025-02.pdf", b"PDF invoice data..."),
        ("reports/quarterly.docx", b"DOCX report data..."),
        ("reports/annual.xlsx", b"XLSX spreadsheet data..."),
        ("contracts/client-a.pdf", b"PDF contract data..."),
    ];

    for (path, content) in documents {
        archive.create_file(path, content)?;
        println!("   ✓ Stored: {}", path);
    }

    // Search for all files in invoices
    println!("\n   Searching for invoices...");
    let invoices = archive.list_dir("invoices")?;
    println!("   Found {} invoices:", invoices.len());
    for invoice in invoices {
        println!("     - {}", invoice);
    }

    drop(archive);
    std::fs::remove_file("documents.cart")?;
    println!("   ✓ Complete\n");

    Ok(())
}

/// Example: Configuration storage
fn create_config_store() -> Result<()> {
    println!("⚙️  Configuration Store Example");
    println!("   Storing application configuration...\n");

    let mut config = Cartridge::create("config.cart", 1000)?;

    // Store various config files
    config.create_file("app/settings.json", br#"{"theme": "dark", "autosave": true}"#)?;
    config.create_file("app/database.toml", b"host = \"localhost\"\nport = 5432")?;
    config.create_file("users/permissions.yaml", b"admin: [read, write, delete]")?;

    // Read config
    let settings = config.read_file("app/settings.json")?;
    println!("   Settings: {}", String::from_utf8_lossy(&settings));

    // List all directories
    println!("\n   All directories:");
    let dirs = config.list_dir("/")?;
    for dir in dirs {
        println!("     - {}", dir);
    }

    drop(config);
    std::fs::remove_file("config.cart")?;
    println!("   ✓ Complete\n");

    Ok(())
}

/// Example: Proper error handling
fn demonstrate_error_handling() -> Result<()> {
    println!("⚠️  Error Handling Example");
    println!("   Demonstrating error handling patterns...\n");

    let mut archive = Cartridge::create("errors.cart", 1000)?;

    // Write a file
    archive.create_file("test.txt", b"content")?;

    // Try to read non-existent file
    match archive.read_file("missing.txt") {
        Ok(_) => println!("   ✗ Unexpected success"),
        Err(e) => println!("   ✓ Correctly handled error: {}", e),
    }

    // Try to create a file that already exists
    match archive.create_file("test.txt", b"new content") {
        Ok(_) => println!("   ✗ Created duplicate file"),
        Err(e) => println!("   ✓ Correctly prevented duplicate: {}", e),
    }

    // Use write_file to update instead
    archive.write_file("test.txt", b"updated content")?;
    println!("   ✓ Updated file with write_file");

    drop(archive);
    std::fs::remove_file("errors.cart")?;
    println!("   ✓ Complete\n");

    Ok(())
}
