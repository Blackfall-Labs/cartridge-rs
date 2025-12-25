use cartridge_rs::Cartridge;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut cart = Cartridge::create("test-two", "Test Two")?;
    
    println!("Writing first 1MB file...");
    let data = vec![0xAB; 1024 * 1024];
    cart.write("/bucket/file1.bin", &data)?;
    println!("✓ First 1MB written");
    
    println!("Writing second 1MB file...");
    cart.write("/bucket/file2.bin", &data)?;
    println!("✓ Second 1MB written");
    
    println!("Reading both files...");
    let f1 = cart.read("/bucket/file1.bin")?;
    let f2 = cart.read("/bucket/file2.bin")?;
    println!("✓ Read {} and {} bytes", f1.len(), f2.len());
    
    std::fs::remove_file("test-two.cart")?;
    Ok(())
}
