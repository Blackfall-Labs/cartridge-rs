//! Manifest and Metadata Example
//!
//! Demonstrates slug/title distinction and manifest management.
//!
//! Run with: cargo run --example manifest

use cartridge_rs::Cartridge;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cartridge Manifest Example ===\n");

    // Slug vs Title
    println!("1. Slug vs Title:");
    println!("   - Slug:  kebab-case identifier (filename, registry key)");
    println!("   - Title: human-readable display name");
    println!();

    // Create with distinct slug and title
    println!("2. Creating container...");
    let mut cart = Cartridge::create(
        "us-constitution",   // slug (filename)
        "U.S. Constitution", // title (display)
    )?;
    println!("   ✓ Created container");
    println!();

    // Access slug and title
    println!("3. Container identity:");
    println!("   Slug:  {}", cart.slug()?);
    println!("   Title: {}", cart.title()?);
    println!("   File:  {}.cart", cart.slug()?);
    println!();

    // Read full manifest
    println!("4. Reading manifest:");
    let manifest = cart.read_manifest()?;
    println!("   Slug:        {}", manifest.slug);
    println!("   Title:       {}", manifest.title);
    println!("   Version:     {}", manifest.version);
    println!("   Description: {:?}", manifest.description);
    println!();

    // Update manifest with closure
    println!("5. Updating manifest...");
    cart.update_manifest(|m| {
        m.description = Some("The supreme law of the United States".to_string());
    })?;
    println!("   ✓ Description updated");
    println!();

    // Verify update
    println!("6. Verifying update:");
    let updated = cart.read_manifest()?;
    println!("   Description: {:?}", updated.description);
    println!();

    // Add content
    println!("7. Adding content...");
    cart.write("preamble.txt", b"We the People of the United States...")?;
    cart.write("articles/article-1.txt", b"Article I: Legislative Branch")?;
    cart.write(
        "amendments/amendment-1.txt",
        b"First Amendment: Freedom of Speech",
    )?;
    println!("   ✓ Added 3 documents");
    println!();

    // List all files
    println!("8. Container contents:");
    let all_entries = cart.list_entries("")?;
    for entry in &all_entries {
        if entry.is_dir {
            println!("   📁 {}/", entry.name);
        } else {
            println!("   📄 {} ({} bytes)", entry.path, entry.size.unwrap_or(0));
        }
    }
    println!();

    // Persist changes
    cart.flush()?;
    println!("✓ All changes saved to: {}.cart\n", cart.slug()?);

    // Reopen to demonstrate manifest persistence
    println!("9. Reopening container...");
    drop(cart);
    let reopened = Cartridge::open("us-constitution.cart")?;
    println!(
        "   ✓ Reopened: {} ({})",
        reopened.title()?,
        reopened.slug()?
    );

    let manifest = reopened.read_manifest()?;
    println!("   Description: {:?}", manifest.description);
    println!();

    println!("=== Example Complete ===");
    println!("\nKey takeaways:");
    println!("  • Slug is the file identifier (us-constitution)");
    println!("  • Title is the display name (U.S. Constitution)");
    println!("  • Manifest stored at /.cartridge/manifest.json");
    println!("  • Manifest persists across open/close");

    // Cleanup
    std::fs::remove_file("us-constitution.cart")?;
    println!("\n(Cleaned up us-constitution.cart)");

    Ok(())
}
