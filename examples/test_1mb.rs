use cartridge_rs::Cartridge;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating cartridge with default size...");
    let mut cart = Cartridge::create("test-1mb", "Test 1MB")?;
    
    println!("Writing 1MB file...");
    let data = vec![0xAB; 1024 * 1024];
    cart.write("/test-bucket/large.bin", &data)?;
    
    println!("✓ Successfully wrote 1MB file!");
    
    let read_data = cart.read("/test-bucket/large.bin")?;
    println!("✓ Read back {} bytes", read_data.len());
    
    std::fs::remove_file("test-1mb.cart")?;
    Ok(())
}
