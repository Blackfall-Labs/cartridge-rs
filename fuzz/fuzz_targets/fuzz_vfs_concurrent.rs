#![no_main]
use libfuzzer_sys::fuzz_target;
use cartridge_rs::Cartridge;

// VFS concurrent fuzzing placeholder
fuzz_target!(|data: &[u8]| {
    if data.len() < 10 {
        return;
    }

    let slug = format!("fuzz-concurrent-{}", std::process::id());
    let mut cart = match Cartridge::create(&slug, "Fuzz Concurrent") {
        Ok(c) => c,
        Err(_) => return,
    };

    // Basic test that operations don't crash
    for i in 0..4 {
        let chunk_size = data.len() / 4;
        let start = i * chunk_size;
        let end = (start + chunk_size).min(data.len());
        if start < end {
            let _ = cart.write(&format!("/file{}.bin", i), &data[start..end]);
        }
    }

    std::fs::remove_file(format!("{}.cart", slug)).ok();
});
