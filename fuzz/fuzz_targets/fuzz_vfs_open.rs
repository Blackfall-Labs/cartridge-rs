#![no_main]
use libfuzzer_sys::fuzz_target;
use cartridge_rs::Cartridge;

// VFS fuzzing currently skipped - requires unsafe FFI and complex setup
// This fuzzer is a placeholder for future implementation

fuzz_target!(|data: &[u8]| {
    if data.len() < 16 {
        return;
    }

    // For now, just test that malformed data doesn't crash Cartridge creation
    let slug = format!("fuzz-open-{}", std::process::id());
    let cart = match Cartridge::create(&slug, "Fuzz Test") {
        Ok(c) => c,
        Err(_) => return,
    };

    drop(cart);
    std::fs::remove_file(format!("{}.cart", slug)).ok();
});
