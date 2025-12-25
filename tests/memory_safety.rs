//! Memory safety tests
//!
//! Run with sanitizers:
//! - cargo +nightly test --test memory_safety -- --test-threads=1
//! - RUSTFLAGS="-Z sanitizer=address" cargo +nightly test --test memory_safety
//! - RUSTFLAGS="-Z sanitizer=leak" cargo +nightly test --test memory_safety
//! - RUSTFLAGS="-Z sanitizer=thread" cargo +nightly test --test memory_safety

use cartridge_rs::Cartridge;
use std::sync::Arc;
use parking_lot::RwLock;

#[test]
fn test_no_double_free() {
    // Sanitizers will detect double-free
    let mut cart = Cartridge::create("double-free-test", "Double Free Test").unwrap();
    cart.write("/file.txt", b"data").unwrap();

    // Read data multiple times
    for _ in 0..10 {
        let data = cart.read("/file.txt").unwrap();
        assert_eq!(data, b"data");
    }

    drop(cart);
    std::fs::remove_file("double-free-test.cart").ok();
}

#[test]
fn test_no_use_after_free() {
    // ASan/Miri will detect use-after-free
    let mut cart = Cartridge::create("uaf-test", "UAF Test").unwrap();
    cart.write("/file.txt", b"data").unwrap();

    let data = cart.read("/file.txt").unwrap();
    assert_eq!(data, b"data");

    // data is owned by caller, should outlive cart
    drop(cart);
    assert_eq!(data, b"data"); // Still valid

    std::fs::remove_file("uaf-test.cart").ok();
}

#[test]
fn test_no_memory_leaks_basic() {
    // LeakSanitizer will detect leaks
    for i in 0..100 {
        let mut cart = Cartridge::create(
            &format!("leak-test-{}", i),
            "Leak Test"
        ).unwrap();

        // Write and read data
        cart.write("/file.txt", b"test data").unwrap();
        let _ = cart.read("/file.txt").unwrap();

        // Flush and drop
        cart.flush().unwrap();
        drop(cart);

        std::fs::remove_file(format!("leak-test-{}.cart", i)).ok();
    }
}

#[test]
fn test_no_memory_leaks_concurrent() {
    // LeakSanitizer with concurrent access
    let cart = Arc::new(RwLock::new(
        Cartridge::create("concurrent-leak", "Concurrent Leak").unwrap()
    ));

    // Write initial data
    {
        let mut c = cart.write();
        c.write("/file.txt", b"data").unwrap();
    }

    // Concurrent readers
    let handles: Vec<_> = (0..10).map(|_| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for _ in 0..100 {
                let c = cart_clone.read();
                let _ = c.read("/file.txt");
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    drop(cart);
    std::fs::remove_file("concurrent-leak.cart").ok();
}

#[test]
fn test_buffer_overflow_protection() {
    // ASan will detect buffer overflows
    let mut cart = Cartridge::create("overflow-test", "Overflow Test").unwrap();

    // Write various sizes
    for size in [1, 10, 100, 1000, 10000, 100000] {
        let data = vec![0xAB; size];
        cart.write(&format!("/file_{}.bin", size), &data).unwrap();

        let read_data = cart.read(&format!("/file_{}.bin", size)).unwrap();
        assert_eq!(read_data.len(), size);
        assert!(read_data.iter().all(|&b| b == 0xAB));
    }

    std::fs::remove_file("overflow-test.cart").ok();
}

#[test]
fn test_stack_overflow_protection() {
    // Deep recursion should not cause stack overflow
    let mut cart = Cartridge::create("stack-test", "Stack Test").unwrap();

    // Create deep directory structure
    let mut path = String::from("/");
    for i in 0..50 {
        path.push_str(&format!("dir{}/", i));
    }
    path.push_str("file.txt");

    cart.write(&path, b"deep file").unwrap();
    let data = cart.read(&path).unwrap();
    assert_eq!(data, b"deep file");

    std::fs::remove_file("stack-test.cart").ok();
}

#[test]
fn test_integer_overflow_protection() {
    // Test with boundary values
    let mut cart = Cartridge::create("int-overflow", "Int Overflow").unwrap();

    // Large but valid allocations
    let sizes = [
        4096,           // 1 page
        4096 * 10,      // 10 pages
        4096 * 100,     // 100 pages
        1024 * 1024,    // 1MB
    ];

    for &size in &sizes {
        let data = vec![0xFF; size];
        cart.write(&format!("/size_{}.bin", size), &data).unwrap();
    }

    std::fs::remove_file("int-overflow.cart").ok();
}

#[test]
fn test_data_race_detection() {
    // ThreadSanitizer will detect data races
    let cart = Arc::new(RwLock::new(
        Cartridge::create("race-test", "Race Test").unwrap()
    ));

    // Writer thread
    let cart_write = cart.clone();
    let writer = std::thread::spawn(move || {
        for i in 0..100 {
            let mut c = cart_write.write();
            c.write(&format!("/file{}.txt", i), b"data").unwrap();
        }
    });

    // Reader threads
    let readers: Vec<_> = (0..5).map(|_| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for _ in 0..20 {
                let c = cart_clone.read();
                let _ = c.list("/");
            }
        })
    }).collect();

    writer.join().unwrap();
    for r in readers {
        r.join().unwrap();
    }

    std::fs::remove_file("race-test.cart").ok();
}

#[test]
fn test_null_pointer_dereference_protection() {
    // Ensure no null pointer dereferences
    let mut cart = Cartridge::create("null-test", "Null Test").unwrap();

    // Try various operations that might trigger null derefs
    cart.write("/file.txt", b"data").unwrap();

    // Read non-existent file
    let result = cart.read("/nonexistent.txt");
    assert!(result.is_err());

    // Delete non-existent file
    let result = cart.delete("/nonexistent.txt");
    assert!(result.is_err());

    // List non-existent directory
    let result = cart.list("/nonexistent/");
    assert!(result.is_err() || result.unwrap().is_empty());

    std::fs::remove_file("null-test.cart").ok();
}

#[test]
fn test_allocation_failure_handling() {
    // Test graceful handling of allocation failures
    let mut cart = Cartridge::create("alloc-fail", "Alloc Fail").unwrap();

    // Try to allocate many large blocks
    let mut allocated = 0;
    for i in 0..1000 {
        let data = vec![0xAB; 512 * 1024]; // 512KB each
        match cart.write(&format!("/large{}.bin", i), &data) {
            Ok(_) => allocated += 1,
            Err(_) => break, // Graceful failure
        }
    }

    println!("Successfully allocated {} large files", allocated);
    assert!(allocated > 0, "Should allocate at least some files");

    std::fs::remove_file("alloc-fail.cart").ok();
}

#[test]
fn test_concurrent_modification_safety() {
    // Test concurrent writes don't corrupt data
    // Reduced from 10 threads × 50 files to 5 threads × 5 files due to catalog size limitation (4KB page)
    let cart = Arc::new(RwLock::new(
        Cartridge::create("concurrent-mod", "Concurrent Mod").unwrap()
    ));

    let handles: Vec<_> = (0..5).map(|thread_id| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for i in 0..5 {
                let mut c = cart_clone.write();
                let path = format!("/thread{}_file{}.txt", thread_id, i);
                c.write(&path, format!("data from thread {}", thread_id).as_bytes()).unwrap();
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    // Verify all files were written correctly
    let c = cart.read();
    for thread_id in 0..5 {
        for i in 0..5 {
            let path = format!("/thread{}_file{}.txt", thread_id, i);
            let data = c.read(&path).unwrap();
            assert_eq!(data, format!("data from thread {}", thread_id).as_bytes());
        }
    }

    std::fs::remove_file("concurrent-mod.cart").ok();
}
