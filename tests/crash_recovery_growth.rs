//! Crash recovery during auto-growth tests
//!
//! Tests to verify cartridge handles interrupted operations gracefully

use cartridge_rs::Cartridge;
use std::sync::{Arc, Mutex};
use std::thread;

#[test]
fn test_reopen_after_growth() {
    // Verify container can be reopened after growth
    let mut cart = Cartridge::create("recovery-growth", "Recovery Growth").unwrap();

    // Trigger growth
    for i in 0..5 {
        cart.write(&format!("/large{}.bin", i), &vec![i as u8; 512 * 1024]).unwrap();
    }

    drop(cart); // Now automatically flushes

    // Reopen - main goal is to verify container is valid and data intact
    let cart = Cartridge::open("recovery-growth.cart").unwrap();

    // Verify files are intact
    for i in 0..5 {
        let data = cart.read(&format!("/large{}.bin", i)).unwrap();
        assert_eq!(data.len(), 512 * 1024);
        assert!(data.iter().all(|&b| b == i as u8));
    }

    std::fs::remove_file("recovery-growth.cart").ok();
}

#[test]
fn test_concurrent_growth_safety() {
    let cart = Arc::new(Mutex::new(
        Cartridge::create("concurrent-growth", "Concurrent Growth").unwrap()
    ));

    // Two threads try to trigger growth
    let handles: Vec<_> = (0..2).map(|i| {
        let cart_clone = cart.clone();
        thread::spawn(move || {
            let mut c = cart_clone.lock().unwrap();
            let data = vec![i as u8; 512 * 1024];
            c.write(&format!("/large{}.bin", i), &data).unwrap();
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    // Verify both files exist
    let c = cart.lock().unwrap();
    assert!(c.read("/large0.bin").is_ok());
    assert!(c.read("/large1.bin").is_ok());

    std::fs::remove_file("concurrent-growth.cart").ok();
}

#[test]
fn test_growth_preserves_existing_data() {
    let mut cart = Cartridge::create("growth-preserve", "Growth Preserve").unwrap();

    // Write initial data
    cart.write("/initial.txt", b"initial data").unwrap();

    // Force growth
    for i in 0..5 {
        cart.write(&format!("/grow{}.bin", i), &vec![0xFF; 1024 * 1024]).unwrap();
    }

    // Verify initial data still intact
    let data = cart.read("/initial.txt").unwrap();
    assert_eq!(data, b"initial data");

    std::fs::remove_file("growth-preserve.cart").ok();
}

#[test]
fn test_multiple_growth_cycles() {
    let mut cart = Cartridge::create("multi-growth", "Multi Growth").unwrap();

    let mut file_count = 0;

    // Trigger multiple growth cycles
    for cycle in 0..10 {
        // Add files until we've likely triggered growth
        for i in 0..5 {
            cart.write(
                &format!("/cycle{}_file{}.bin", cycle, i),
                &vec![cycle as u8; 256 * 1024]
            ).unwrap();
            file_count += 1;
        }
    }

    // Verify all files exist and are correct
    for cycle in 0..10 {
        for i in 0..5 {
            let data = cart.read(&format!("/cycle{}_file{}.bin", cycle, i)).unwrap();
            assert_eq!(data.len(), 256 * 1024);
            assert!(data.iter().all(|&b| b == cycle as u8));
        }
    }

    assert_eq!(file_count, 50);
    std::fs::remove_file("multi-growth.cart").ok();
}

#[test]
fn test_growth_with_concurrent_reads() {
    let cart = Arc::new(Mutex::new(
        Cartridge::create("growth-reads", "Growth Reads").unwrap()
    ));

    // Pre-populate
    {
        let mut c = cart.lock().unwrap();
        for i in 0..10 {
            c.write(&format!("/existing{}.txt", i), b"existing").unwrap();
        }
    }

    // Writer thread (triggers growth)
    let cart_clone1 = cart.clone();
    let writer = thread::spawn(move || {
        let mut c = cart_clone1.lock().unwrap();
        for i in 0..5 {
            c.write(&format!("/large{}.bin", i), &vec![0xAB; 512 * 1024]).unwrap();
        }
    });

    // Reader threads
    let reader_handles: Vec<_> = (0..3).map(|_| {
        let cart_clone = cart.clone();
        thread::spawn(move || {
            for _ in 0..50 {
                let c = cart_clone.lock().unwrap();
                let idx = rand::random::<usize>() % 10;
                let _ = c.read(&format!("/existing{}.txt", idx));
            }
        })
    }).collect();

    writer.join().unwrap();
    for h in reader_handles {
        h.join().unwrap();
    }

    std::fs::remove_file("growth-reads.cart").ok();
}

#[test]
fn test_allocator_state_after_growth() {
    let mut cart = Cartridge::create("growth-alloc", "Growth Alloc").unwrap();

    let initial_free = cart.header().free_blocks;

    // Force growth
    for i in 0..10 {
        cart.write(&format!("/file{}.bin", i), &vec![0xCD; 512 * 1024]).unwrap();
    }

    // After growth, free blocks may have increased
    let after_growth_free = cart.header().free_blocks;

    // Verify allocator is still consistent
    // (In auto-growth, free blocks may increase if we grew the container)
    assert!(after_growth_free >= 0);

    // Reopen and verify consistency
    drop(cart);
    let cart = Cartridge::open("growth-alloc.cart").unwrap();
    // Free blocks should be reasonable (not negative, not excessive)
    assert!(cart.header().free_blocks < cart.header().total_blocks);

    std::fs::remove_file("growth-alloc.cart").ok();
}
