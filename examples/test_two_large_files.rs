//! Test writing two large files to verify auto-growth bug is fixed

use cartridge_rs::Cartridge;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing auto-growth with two large files...");

    // Create auto-growing cartridge
    let mut cart = Cartridge::create("test-two-large", "Test Two Large Files")?;

    println!("Initial state: {} total blocks, {} free blocks",
        cart.header().total_blocks,
        cart.header().free_blocks);

    // Write first 1MB file (256 blocks)
    println!("\nWriting first 1MB file...");
    let data = vec![0xAB; 1024 * 1024];
    cart.write("/bucket/file1.bin", &data)?;

    println!("After first write: {} total blocks, {} free blocks",
        cart.header().total_blocks,
        cart.header().free_blocks);

    // Write second 1MB file (256 blocks)
    println!("\nWriting second 1MB file...");
    cart.write("/bucket/file2.bin", &data)?;

    println!("After second write: {} total blocks, {} free blocks",
        cart.header().total_blocks,
        cart.header().free_blocks);

    // Verify both files exist
    println!("\nVerifying both files...");
    let file1 = cart.read("/bucket/file1.bin")?;
    assert_eq!(file1.len(), 1024 * 1024);
    println!("✓ file1.bin: {} bytes", file1.len());

    let file2 = cart.read("/bucket/file2.bin")?;
    assert_eq!(file2.len(), 1024 * 1024);
    println!("✓ file2.bin: {} bytes", file2.len());

    println!("\n✓ SUCCESS: Both large files written and verified!");

    Ok(())
}
