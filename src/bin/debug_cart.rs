use cartridge_rs::Cartridge;
use std::path::Path;

fn main() {
    let cart_path = std::env::args().nth(1)
        .unwrap_or_else(|| "E:/repos/blackfall-labs/astromind/testing/cartridges/active/corvus.cart".to_string());

    println!("Opening: {}", cart_path);
    let cart = Cartridge::open(Path::new(&cart_path)).expect("Failed to open");

    println!("\n=== All files ===");
    let files = cart.list("").expect("Failed to list");
    for f in &files {
        println!("  {}", f);
    }
    println!("Total: {} files", files.len());

    // Check bootstrap.eng specifically
    println!("\n=== bootstrap.eng check ===");
    match cart.read("bootstrap.eng") {
        Ok(data) => println!("  EXISTS: {} bytes", data.len()),
        Err(e) => println!("  MISSING: {}", e),
    }
}
