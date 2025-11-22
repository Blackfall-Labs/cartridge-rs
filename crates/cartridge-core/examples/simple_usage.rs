//! Simple usage example for Cartridge with auto-growth and manifests

use cartridge_core::Cartridge;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a container with just slug and title - no capacity needed!
    let mut cart = Cartridge::create("my-data", "My Data Container")?;

    println!("Created container: {} ({})", cart.title()?, cart.slug()?);

    // Container starts small (12KB) and grows automatically
    let initial_stats = cart.stats();
    println!(
        "Initial size: {} blocks ({} KB)",
        initial_stats.total_blocks,
        initial_stats.total_blocks * 4
    );

    // Add some data - container will auto-grow as needed
    cart.create_file("hello.txt", b"Hello, Cartridge!")?;
    cart.create_file("data.json", br#"{"message": "Auto-growth works!"}"#)?;

    // Add a large file that requires growth
    let large_data = vec![42u8; 100_000]; // 100KB
    cart.create_file("large.bin", &large_data)?;

    let final_stats = cart.stats();
    println!(
        "After adding data: {} blocks ({} KB)",
        final_stats.total_blocks,
        final_stats.total_blocks * 4
    );
    println!("Container grew by {} blocks!", final_stats.total_blocks - initial_stats.total_blocks);

    // Read data back
    let content = cart.read_file("hello.txt")?;
    println!("Read back: {}", String::from_utf8_lossy(&content));

    // Update manifest metadata
    cart.update_manifest(|manifest| {
        manifest.description = Some("Example container with auto-growth".to_string());
    })?;

    cart.close()?;

    println!("\nContainer saved to: my-data.cart");
    Ok(())
}
