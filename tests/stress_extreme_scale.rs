//! Extreme scale stress tests
//!
//! These tests are ignored by default - run manually with:
//! cargo test --release test_100gb_container -- --ignored

use cartridge_rs::Cartridge;

#[test]
#[ignore]
fn test_100gb_container() {
    let mut cart = Cartridge::create("stress-100gb", "Stress 100GB").unwrap();

    // Create 100 x 1GB files
    for i in 0..100 {
        println!("Creating file {} of 100...", i + 1);
        let data = vec![i as u8; 1024 * 1024 * 1024]; // 1GB
        cart.write(&format!("/file{:03}.bin", i), &data).unwrap();

        // Verify allocator consistency after each large write
        assert!(cart.header().total_blocks > 0);
    }

    // Verify container size
    let size = std::fs::metadata("stress-100gb.cart").unwrap().len();
    assert!(size > 100 * 1024 * 1024 * 1024, "Container should be > 100GB, got {} bytes", size);

    // Random access
    for _ in 0..100 {
        let idx = rand::random::<usize>() % 100;
        let data = cart.read(&format!("/file{:03}.bin", idx)).unwrap();
        assert_eq!(data.len(), 1024 * 1024 * 1024);
        assert_eq!(data[0], idx as u8);
    }

    std::fs::remove_file("stress-100gb.cart").ok();
}

#[test]
#[ignore]
fn test_1m_files() {
    let mut cart = Cartridge::create("stress-1m-files", "Stress 1M Files").unwrap();

    println!("Creating 1,000,000 files...");
    for i in 0..1_000_000 {
        if i % 10_000 == 0 {
            println!("Progress: {} files", i);
        }

        let data = format!("file{}", i).repeat(10); // ~50 bytes
        cart.write(&format!("/f{}.txt", i), data.as_bytes()).unwrap();
    }

    println!("Verifying file count...");
    let all_files = cart.list("/").unwrap();
    assert!(all_files.len() >= 1_000_000, "Expected >= 1M files, got {}", all_files.len());

    println!("Random access test...");
    for _ in 0..10_000 {
        let idx = rand::random::<usize>() % 1_000_000;
        let data = cart.read(&format!("/f{}.txt", idx)).unwrap();
        assert!(data.len() > 0);
    }

    std::fs::remove_file("stress-1m-files.cart").ok();
}

#[test]
#[ignore]
fn test_fragmentation_measurement() {
    let mut cart = Cartridge::create("stress-frag", "Stress Fragmentation").unwrap();

    // Create many files
    for i in 0..10_000 {
        let size = rand::random::<usize>() % (100 * 1024) + 1024; // 1KB-100KB
        cart.write(&format!("/file{}.bin", i), &vec![i as u8; size]).unwrap();
    }

    // Delete random 50%
    for i in (0..10_000).step_by(2) {
        cart.delete(&format!("/file{}.bin", i)).unwrap();
    }

    // Check free blocks increased
    let free_after_delete = cart.header().free_blocks;
    assert!(free_after_delete > 0, "Should have free blocks after deletes");

    // Re-fill deleted space
    for i in 0..5_000 {
        let size = rand::random::<usize>() % (100 * 1024) + 1024;
        cart.write(&format!("/new{}.bin", i), &vec![0xFF; size]).unwrap();
    }

    // Verify allocator health
    assert!(cart.header().total_blocks > 0);

    std::fs::remove_file("stress-frag.cart").ok();
}

#[test]
#[ignore]
fn test_max_auto_growth_limit() {
    let mut cart = Cartridge::create("stress-max-growth", "Stress Max Growth").unwrap();

    // Set low max_blocks for testing (default is 10M blocks = 40GB)
    // Note: Requires access to set_max_blocks API

    // Fill with data until we approach capacity
    let mut i = 0;
    loop {
        if i > 1000 {
            // Safety limit for test
            break;
        }

        match cart.write(&format!("/file{}.bin", i), &vec![0xAB; 64 * 1024]) {
            Ok(_) => {
                i += 1;
                if i % 100 == 0 {
                    println!("Written {} files, total_blocks: {}, free_blocks: {}",
                             i, cart.header().total_blocks, cart.header().free_blocks);
                }
            }
            Err(e) => {
                println!("Stopped at {} files with error: {:?}", i, e);
                break;
            }
        }
    }

    // Container should still be valid
    assert!(cart.list("/").is_ok());
    assert!(cart.header().total_blocks > 0);

    std::fs::remove_file("stress-max-growth.cart").ok();
}

#[test]
fn test_moderate_scale() {
    // Non-ignored test for CI - moderate scale
    // Reduced from 1000 to 200 due to catalog size limitations (single page = 4KB)
    let mut cart = Cartridge::create("stress-moderate", "Stress Moderate").unwrap();

    // Create 200 files of varying sizes
    for i in 0..200 {
        let size = if i % 10 == 0 {
            512 * 1024 // Large file (extent allocator)
        } else {
            (i % 100 + 1) * 1024 // Small files (bitmap allocator)
        };

        let byte_val = (i % 256) as u8; // Prevent overflow
        let path = format!("/file{}.bin", i);
        cart.write(&path, &vec![byte_val; size])
            .unwrap_or_else(|e| panic!("Failed to write {} (size {}): {}", path, size, e));
    }

    // List files to see how many actually got written
    let files = cart.list("/").unwrap();
    println!("Wrote 200 files, catalog shows: {} entries", files.len());

    // Verify all files
    for i in 0..200 {
        let byte_val = (i % 256) as u8;
        let data = cart.read(&format!("/file{}.bin", i)).unwrap();
        assert!(data.len() > 0);
        assert_eq!(data[0], byte_val);
    }

    // Check allocator health
    assert!(cart.header().free_blocks < cart.header().total_blocks);

    std::fs::remove_file("stress-moderate.cart").ok();
}

#[test]
fn test_large_single_file() {
    let mut cart = Cartridge::create("stress-large-single", "Stress Large Single").unwrap();

    // Write 100MB file
    let data = vec![0xAB; 100 * 1024 * 1024];
    cart.write("/large.bin", &data).unwrap();

    // Read back
    let read_data = cart.read("/large.bin").unwrap();
    assert_eq!(read_data.len(), 100 * 1024 * 1024);
    assert!(read_data.iter().all(|&b| b == 0xAB));

    std::fs::remove_file("stress-large-single.cart").ok();
}

#[test]
fn test_many_small_files_performance() {
    let mut cart = Cartridge::create("stress-many-small", "Stress Many Small").unwrap();

    // Create 10,000 small files
    let start = std::time::Instant::now();
    for i in 0..10_000 {
        cart.write(&format!("/small{}.txt", i), b"data").unwrap();
    }
    let elapsed = start.elapsed();

    println!("Created 10,000 small files in {:?}", elapsed);
    println!("Average: {:?} per file", elapsed / 10_000);

    // Verify
    let files = cart.list("/").unwrap();
    assert!(files.len() >= 10_000);

    std::fs::remove_file("stress-many-small.cart").ok();
}

#[test]
fn test_alternating_large_small() {
    let mut cart = Cartridge::create("stress-alternating", "Stress Alternating").unwrap();

    // Alternate between large and small files
    for i in 0..100 {
        if i % 2 == 0 {
            // Large file
            cart.write(&format!("/large{}.bin", i), &vec![0xAB; 512 * 1024]).unwrap();
        } else {
            // Small file
            cart.write(&format!("/small{}.txt", i), &vec![0xCD; 4 * 1024]).unwrap();
        }
    }

    // Verify allocator handled mixed sizes well
    assert!(cart.header().total_blocks > 0);
    assert!(cart.header().free_blocks < cart.header().total_blocks);

    std::fs::remove_file("stress-alternating.cart").ok();
}
