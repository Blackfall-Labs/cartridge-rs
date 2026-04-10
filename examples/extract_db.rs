fn main() {
    let cart_path = std::env::args().nth(1).expect("cart path");
    let db_name = std::env::args().nth(2).expect("db name");
    let out_path = std::env::args().nth(3).expect("output path");
    let cart = cartridge_rs::Cartridge::open(&cart_path).expect("open cart");
    let data = cart.read(&db_name).expect("read file");
    std::fs::write(&out_path, &data).expect("write");
    println!("Extracted {} bytes to {}", data.len(), out_path);
}
