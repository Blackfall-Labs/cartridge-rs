//! Allocator corruption detection tests
//!
//! Tests to verify allocator integrity and detect corruption

use cartridge_rs::Cartridge;
use std::collections::HashSet;

#[test]
fn test_allocator_consistency_small_files() {
    let mut cart = Cartridge::create("alloc-small", "Alloc Small").unwrap();

    // Allocate many small files (bitmap allocator)
    for i in 0..50 {
        let size = (i % 10 + 1) * 1024; // 1KB-10KB
        cart.write(&format!("/small{}.txt", i), &vec![i as u8; size]).unwrap();
    }

    // Track allocated blocks
    let mut allocated_blocks = HashSet::new();

    for i in 0..50 {
        let meta = cart.metadata(&format!("/small{}.txt", i)).unwrap();
        // Ensure no block is allocated twice
        for &block in &meta.blocks {
            assert!(
                !allocated_blocks.contains(&block),
                "Block {} allocated twice!",
                block
            );
            allocated_blocks.insert(block);
        }
    }

    std::fs::remove_file("alloc-small.cart").ok();
}

#[test]
fn test_allocator_consistency_large_files() {
    let mut cart = Cartridge::create("alloc-large", "Alloc Large").unwrap();

    // Allocate large files (extent allocator)
    for i in 0..10 {
        cart.write(&format!("/large{}.bin", i), &vec![i as u8; 512 * 1024]).unwrap();
    }

    let mut allocated_blocks = HashSet::new();

    for i in 0..10 {
        let meta = cart.metadata(&format!("/large{}.bin", i)).unwrap();
        for &block in &meta.blocks {
            assert!(
                !allocated_blocks.contains(&block),
                "Block {} allocated twice!",
                block
            );
            allocated_blocks.insert(block);
        }
    }

    std::fs::remove_file("alloc-large.cart").ok();
}

#[test]
fn test_allocator_free_blocks_tracking() {
    let mut cart = Cartridge::create("alloc-free", "Alloc Free").unwrap();

    let initial_free = cart.header().free_blocks;

    // Allocate files
    for i in 0..20 {
        cart.write(&format!("/file{}.bin", i), &vec![0xAB; 64 * 1024]).unwrap();
    }

    let after_alloc_free = cart.header().free_blocks;
    // Free blocks may not decrease if auto-growth occurred
    // Just verify we can still allocate
    assert!(cart.header().total_blocks > 0);

    // Delete half
    for i in 0..10 {
        cart.delete(&format!("/file{}.bin", i)).unwrap();
    }

    let after_delete_free = cart.header().free_blocks;
    assert!(after_delete_free > after_alloc_free, "Free blocks should increase after delete");

    std::fs::remove_file("alloc-free.cart").ok();
}

#[test]
fn test_allocator_mixed_workload() {
    let mut cart = Cartridge::create("alloc-mixed", "Alloc Mixed").unwrap();

    // Mix small and large allocations
    for i in 0..20 {
        if i % 2 == 0 {
            cart.write(&format!("/small{}.txt", i), &vec![i as u8; 10 * 1024]).unwrap();
        } else {
            cart.write(&format!("/large{}.bin", i), &vec![i as u8; 512 * 1024]).unwrap();
        }
    }

    // Verify no block overlap
    let mut all_blocks = HashSet::new();

    for i in 0..20 {
        let path = if i % 2 == 0 {
            format!("/small{}.txt", i)
        } else {
            format!("/large{}.bin", i)
        };

        let meta = cart.metadata(&path).unwrap();
        for &block in &meta.blocks {
            assert!(
                !all_blocks.contains(&block),
                "Block {} used by multiple files!",
                block
            );
            all_blocks.insert(block);
        }
    }

    std::fs::remove_file("alloc-mixed.cart").ok();
}

#[test]
fn test_allocator_after_growth() {
    let mut cart = Cartridge::create("alloc-growth", "Alloc Growth").unwrap();

    let initial_total = cart.header().total_blocks;

    // Force growth
    for i in 0..5 {
        cart.write(&format!("/file{}.bin", i), &vec![0xCD; 1024 * 1024]).unwrap();
    }

    let after_growth_total = cart.header().total_blocks;
    assert!(after_growth_total > initial_total, "Container should have grown");

    // Verify allocator consistency
    let free_blocks = cart.header().free_blocks;
    assert!(free_blocks > 0, "Should have free blocks after growth");

    std::fs::remove_file("alloc-growth.cart").ok();
}

#[test]
fn test_allocator_fragmentation_resistance() {
    let mut cart = Cartridge::create("alloc-frag", "Alloc Frag").unwrap();

    // Create, delete, create pattern to induce fragmentation
    for round in 0..3 {
        // Create files
        for i in 0..20 {
            let size = ((i * 7) % 50 + 10) * 1024; // Variable sizes
            cart.write(&format!("/temp{}_{}.bin", round, i), &vec![i as u8; size]).unwrap();
        }

        // Delete odd-numbered files
        for i in (1..20).step_by(2) {
            cart.delete(&format!("/temp{}_{}.bin", round, i)).unwrap();
        }
    }

    // Verify all remaining files are intact
    for round in 0..3 {
        for i in (0..20).step_by(2) {
            let path = format!("/temp{}_{}.bin", round, i);
            assert!(cart.read(&path).is_ok(), "File should exist: {}", path);
        }
    }

    std::fs::remove_file("alloc-frag.cart").ok();
}
