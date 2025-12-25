#![no_main]
use libfuzzer_sys::{fuzz_target, arbitrary::{Arbitrary, Unstructured}};
use cartridge_rs::Cartridge;

#[derive(Debug, Arbitrary)]
struct FileOp {
    path_idx: u8,
    data: Vec<u8>,
}

// VFS fuzzing placeholder - tests basic write operations
fuzz_target!(|input: &[u8]| {
    let mut u = Unstructured::new(input);

    let ops: Vec<FileOp> = match u.arbitrary() {
        Ok(ops) => ops,
        Err(_) => return,
    };

    if ops.is_empty() {
        return;
    }

    let slug = format!("fuzz-rw-{}", std::process::id());
    let mut cart = match Cartridge::create(&slug, "Fuzz RW") {
        Ok(c) => c,
        Err(_) => return,
    };

    // Test write operations don't crash
    for op in ops.iter().take(10) {
        let path = format!("/file{}.bin", op.path_idx);
        let _ = cart.write(&path, &op.data);
    }

    std::fs::remove_file(format!("{}.cart", slug)).ok();
});
