use cartridge_rs::Cartridge;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).init();
    
    let mut cart = Cartridge::create("trace", "Trace")?;
    
    println!("Writing first 1MB...");
    let data = vec![0xAB; 1024 * 1024];
    cart.write("/file1.bin", &data)?;
    println!("✓ First write complete\n");
    
    println!("Writing second 1MB...");
    match cart.write("/file2.bin", &data) {
        Ok(_) => println!("✓ Second write complete"),
        Err(e) => println!("✗ Second write failed: {:?}", e),
    }
    
    std::fs::remove_file("trace.cart").ok();
    Ok(())
}
