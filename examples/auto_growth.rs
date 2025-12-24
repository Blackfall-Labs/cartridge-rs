//! Auto-Growth Example
//!
//! Demonstrates how Cartridge containers automatically grow from minimal size.
//! No need to pre-allocate capacity!
//!
//! Run with: cargo run --example auto_growth

use cartridge_rs::Cartridge;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Cartridge Auto-Growth Example ===\n");

    // Create container - starts at just 12KB (3 blocks)
    println!("Creating container...");
    let mut cart = Cartridge::create("growth-demo", "Auto-Growth Demo")?;
    println!("✓ Created: {}\n", cart.slug()?);

    // Check initial size
    let initial_stats = cart.inner().stats();
    println!("Initial size:");
    println!("  Total blocks: {}", initial_stats.total_blocks);
    println!("  Size: {} KB", initial_stats.total_blocks * 4);
    println!("  Free blocks: {}", initial_stats.free_blocks);
    println!();

    // Add small files
    println!("Adding small files...");
    cart.write("hello.txt", b"Hello, Cartridge!")?;
    cart.write("data.json", br#"{"message": "Auto-growth works!"}"#)?;
    println!("✓ Written 2 small files\n");

    let after_small = cart.inner().stats();
    println!("After small files:");
    println!("  Total blocks: {}", after_small.total_blocks);
    println!("  Free blocks: {}", after_small.free_blocks);
    println!();

    // Add a large file that requires growth
    println!("Adding large file (100KB)...");
    let large_data = vec![42u8; 100_000];
    cart.write("large.bin", &large_data)?;
    println!("✓ Written large file\n");

    let after_large = cart.inner().stats();
    println!("After large file:");
    println!("  Total blocks: {}", after_large.total_blocks);
    println!("  Size: {} KB", after_large.total_blocks * 4);
    println!("  Free blocks: {}", after_large.free_blocks);
    println!(
        "  Growth: {} blocks ({} KB)\n",
        after_large.total_blocks - initial_stats.total_blocks,
        (after_large.total_blocks - initial_stats.total_blocks) * 4
    );

    // Add more data to trigger additional growth
    println!("Adding multiple medium files...");
    for i in 0..10 {
        let data = vec![i as u8; 50_000];
        cart.write(&format!("file_{}.dat", i), &data)?;
    }
    println!("✓ Written 10 medium files (50KB each)\n");

    let final_stats = cart.inner().stats();
    println!("Final size:");
    println!("  Total blocks: {}", final_stats.total_blocks);
    println!(
        "  Size: {} KB (~{} MB)",
        final_stats.total_blocks * 4,
        (final_stats.total_blocks * 4) / 1024
    );
    println!("  Used blocks: {}", final_stats.used_blocks);
    println!("  Free blocks: {}", final_stats.free_blocks);
    println!(
        "  Total growth: {}x original size\n",
        final_stats.total_blocks / initial_stats.total_blocks
    );

    // Summary
    println!("=== Summary ===");
    println!(
        "Started:  {} KB ({} blocks)",
        initial_stats.total_blocks * 4,
        initial_stats.total_blocks
    );
    println!(
        "Finished: {} KB ({} blocks)",
        final_stats.total_blocks * 4,
        final_stats.total_blocks
    );
    println!(
        "Growth:   {}x",
        final_stats.total_blocks / initial_stats.total_blocks
    );
    println!("\n✓ Container automatically grew as needed!");
    println!("  No capacity planning required!");

    // Cleanup
    std::fs::remove_file("growth-demo.cart")?;
    println!("\n(Cleaned up growth-demo.cart)");

    Ok(())
}
