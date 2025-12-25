use cartridge_rs::Cartridge;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut cart = Cartridge::create("debug-grow", "Debug Growth")?;
    
    println!("=== Initial state ===");
    
    println!("\n=== Writing first 1MB ===");
    let data = vec![0xAB; 1024 * 1024];
    cart.write("/file1.bin", &data)?;
    println!("✓ First 1MB written");
    
    // Force flush to see state
    cart.flush()?;
    
    println!("\n=== Writing second 1MB ===");
    match cart.write("/file2.bin", &data) {
        Ok(_) => println!("✓ Second 1MB written"),
        Err(e) => println!("✗ Second write failed: {:?}", e),
    }
    
    std::fs::remove_file("debug-grow.cart").ok();
    Ok(())
}
