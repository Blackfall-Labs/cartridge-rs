//! Auto-growth integration tests
//!
//! Tests to verify that auto-growth works correctly with:
//! - Multiple large files (≥256KB using extent allocator)
//! - Multiple small files (<256KB using bitmap allocator)
//! - Mixed workloads (both small and large files)

use cartridge_rs::Cartridge;

#[test]
fn test_two_large_files() {
    // Regression test for auto-growth bug where second large file would fail
    let mut cart = Cartridge::create("test-two-large", "Test Two Large").unwrap();

    // Write first 1MB file
    let data = vec![0xAB; 1024 * 1024];
    cart.write("/bucket/file1.bin", &data).unwrap();

    // Write second 1MB file (previously failed here)
    cart.write("/bucket/file2.bin", &data).unwrap();

    // Verify both files exist
    let file1 = cart.read("/bucket/file1.bin").unwrap();
    assert_eq!(file1.len(), 1024 * 1024);

    let file2 = cart.read("/bucket/file2.bin").unwrap();
    assert_eq!(file2.len(), 1024 * 1024);
}

#[test]
fn test_three_large_files() {
    // Verify auto-growth works for 3+ large files
    let mut cart = Cartridge::create("test-three-large", "Test Three Large").unwrap();

    let data = vec![0xCD; 512 * 1024]; // 512KB each

    cart.write("/file1.bin", &data).unwrap();
    cart.write("/file2.bin", &data).unwrap();
    cart.write("/file3.bin", &data).unwrap();

    // Verify all three files
    assert_eq!(cart.read("/file1.bin").unwrap().len(), 512 * 1024);
    assert_eq!(cart.read("/file2.bin").unwrap().len(), 512 * 1024);
    assert_eq!(cart.read("/file3.bin").unwrap().len(), 512 * 1024);
}

#[test]
fn test_many_small_files() {
    // Verify auto-growth works with many small files (bitmap allocator)
    let mut cart = Cartridge::create("test-many-small", "Test Many Small").unwrap();

    // Write 50 small files (4KB each)
    for i in 0..50 {
        let data = vec![i as u8; 4 * 1024];
        let path = format!("/small{}.bin", i);
        cart.write(&path, &data).unwrap();
    }

    // Verify all files
    for i in 0..50 {
        let path = format!("/small{}.bin", i);
        let data = cart.read(&path).unwrap();
        assert_eq!(data.len(), 4 * 1024);
        assert_eq!(data[0], i as u8);
    }
}

#[test]
fn test_mixed_small_and_large() {
    // Verify auto-growth works with mixed workload
    let mut cart = Cartridge::create("test-mixed", "Test Mixed").unwrap();

    // Alternate between small and large files
    for i in 0..10 {
        // Small file (bitmap allocator)
        let small_data = vec![i as u8; 10 * 1024]; // 10KB
        cart.write(&format!("/small{}.bin", i), &small_data)
            .unwrap();

        // Large file (extent allocator)
        let large_data = vec![((i + 100) % 256) as u8; 300 * 1024]; // 300KB
        cart.write(&format!("/large{}.bin", i), &large_data)
            .unwrap();
    }

    // Verify all files
    for i in 0..10 {
        let small = cart.read(&format!("/small{}.bin", i)).unwrap();
        assert_eq!(small.len(), 10 * 1024, "small{}.bin has wrong length", i);
        assert_eq!(small[0], i as u8, "small{}.bin has wrong first byte", i);

        let large = cart.read(&format!("/large{}.bin", i)).unwrap();
        assert_eq!(large.len(), 300 * 1024, "large{}.bin has wrong length", i);
        let expected_byte = ((i + 100) % 256) as u8;
        assert_eq!(large[0], expected_byte,
            "large{}.bin: expected first byte {}, got {}. Full first 16 bytes: {:?}",
            i, expected_byte, large[0], &large[0..16.min(large.len())]);
    }
}

#[test]
fn test_sequential_growth_levels() {
    // Verify container grows through multiple levels
    let mut cart = Cartridge::create("test-growth-levels", "Test Growth Levels").unwrap();

    let initial_blocks = cart.header().total_blocks;
    println!("Initial blocks: {}", initial_blocks);

    // Write files until we've grown at least 4 times (3 -> 6 -> 12 -> 24 -> 48)
    let mut previous_blocks = initial_blocks;
    let mut growth_count = 0;

    for i in 0..20 {
        let data = vec![i as u8; 256 * 1024]; // 256KB
        cart.write(&format!("/file{}.bin", i), &data).unwrap();

        let current_blocks = cart.header().total_blocks;
        if current_blocks > previous_blocks {
            growth_count += 1;
            println!("Growth #{}: {} -> {} blocks", growth_count, previous_blocks, current_blocks);
            previous_blocks = current_blocks;
        }

        if growth_count >= 4 {
            break;
        }
    }

    assert!(growth_count >= 4, "Expected at least 4 growth events, got {}", growth_count);
}

#[test]
fn test_hybrid_allocator_free_blocks_tracking() {
    // Verify free_blocks is correctly tracked across both allocators
    // Use a larger initial container to avoid auto-growth during test
    let mut cart = Cartridge::create("test-free-tracking", "Test Free Tracking").unwrap();

    // Write a file to trigger growth to sufficient size
    let warmup_data = vec![0x00; 1024 * 1024]; // 1MB to trigger growth
    cart.write("/warmup.bin", &warmup_data).unwrap();

    // Now test with stable container size
    let free_before = cart.header().free_blocks;
    println!("Before test allocations: {} free blocks", free_before);

    // Allocate a large file (extent allocator)
    let large_data = vec![0xAB; 256 * 1024]; // 256KB = 64 blocks
    cart.write("/large.bin", &large_data).unwrap();

    let free_after_large = cart.header().free_blocks;
    println!("After large file (256KB): {} free blocks", free_after_large);

    // Allocate a small file (bitmap allocator)
    let small_data = vec![0xCD; 10 * 1024]; // 10KB = 3 blocks
    cart.write("/small.bin", &small_data).unwrap();

    let free_after_small = cart.header().free_blocks;
    println!("After small file (10KB): {} free blocks", free_after_small);

    // Verify free_blocks decreased after each allocation (no growth should occur)
    assert!(free_after_large < free_before,
        "Free blocks should decrease after large file: {} -> {}", free_before, free_after_large);
    assert!(free_after_small < free_after_large,
        "Free blocks should decrease after small file: {} -> {}", free_after_large, free_after_small);

    println!("✓ Free blocks correctly tracked: {} -> {} -> {}", free_before, free_after_large, free_after_small);
}
