use cartridge::Cartridge;

fn main() {
    let mut cart = Cartridge::create("test-large", "Test Large").unwrap();
    
    println!("Created cartridge, writing 1MB...");
    let data = vec![0xAB; 1024 * 1024];
    match cart.write("large.bin", &data) {
        Ok(_) => println!("Success!"),
        Err(e) => println!("Error: {:?}", e),
    }
}
