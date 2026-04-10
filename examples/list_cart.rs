fn main() {
    let path = std::env::args().nth(1).expect("usage: list_cart <path>");
    let cart = cartridge_rs::Cartridge::open(&path).expect("open");
    let entries = cart.list_entries("").expect("list");
    for e in &entries {
        println!("{:>10}  {}", e.size.unwrap_or(0), e.path);
    }
    println!("\n{} entries", entries.len());
}
