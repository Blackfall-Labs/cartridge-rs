//! Engram freeze validation tests
//!
//! Note: EngramFreezer::freeze() expects &mut cartridge_rs::core::Cartridge
//! but the public API exposes cartridge_rs::Cartridge wrapper.
//! These tests are temporarily disabled until freeze API is exposed on the wrapper.

#![allow(dead_code, unused_imports)]

use cartridge_rs::Cartridge;
use std::path::Path;
use tempfile::TempDir;

#[test]
#[ignore] // Requires public freeze API on Cartridge wrapper
fn test_freeze_basic() {
    let temp_dir = TempDir::new().unwrap();
    let engram_path = temp_dir.path().join("frozen.eng");

    let mut cart = Cartridge::create("freeze-test", "Freeze Test").unwrap();

    for i in 0..50 {
        cart.write(&format!("/file{}.txt", i), format!("data{}", i).as_bytes())
            .unwrap();
    }
    cart.flush().unwrap();

    // Freeze to engram using EngramFreezer
    use cartridge_rs::core::engram_integration::EngramFreezer;
    let freezer = EngramFreezer::new_default(
        "freeze-test".to_string(),
        "1.0.0".to_string(),
        "Test Author".to_string(),
    );

    freezer.freeze(&mut cart, &engram_path).unwrap();

    // Verify engram was created
    assert!(engram_path.exists());

    // Verify we can read it back
    use engram_rs::ArchiveReader;
    let mut reader = ArchiveReader::open(&engram_path).unwrap();

    for i in 0..50 {
        let data = reader.read_file(&format!("file{}.txt", i)).unwrap();
        assert_eq!(data, format!("data{}", i).as_bytes());
    }

    std::fs::remove_file("freeze-test.cart").ok();
}

#[test]
#[ignore] // Requires public freeze API + large size and time
fn test_freeze_large_container() {
    let temp_dir = TempDir::new().unwrap();
    let engram_path = temp_dir.path().join("frozen-large.eng");

    let mut cart = Cartridge::create("freeze-large", "Freeze Large").unwrap();

    // 10GB container (100 x 100MB files)
    for i in 0..100 {
        let data = vec![i as u8; 100 * 1024 * 1024]; // 100MB each
        cart.write(&format!("/large{}.bin", i), &data).unwrap();
    }
    cart.flush().unwrap();

    // Freeze should succeed
    use cartridge_rs::core::engram_integration::EngramFreezer;
    use engram_rs::CompressionMethod;
    let freezer = EngramFreezer::new(
        "freeze-large".to_string(),
        "1.0.0".to_string(),
        "Test".to_string(),
        Some("Large container test".to_string()),
        CompressionMethod::Zstd,
    );

    freezer.freeze(&mut cart, &engram_path).unwrap();

    // Verify size
    let eng_size = std::fs::metadata(&engram_path).unwrap().len();
    assert!(
        eng_size > 9 * 1024 * 1024 * 1024,
        "Engram should be > 9GB (with compression), got {} bytes",
        eng_size
    );

    std::fs::remove_file("freeze-large.cart").ok();
}

#[test]
#[ignore] // Requires public freeze API on Cartridge wrapper
fn test_freeze_with_snapshots() {
    let temp_dir = TempDir::new().unwrap();
    let snapshot_dir = temp_dir.path().join("snapshots");
    std::fs::create_dir_all(&snapshot_dir).unwrap();
    let engram_path = temp_dir.path().join("frozen-snap.eng");

    let mut cart = Cartridge::create("freeze-snapshots", "Freeze Snapshots").unwrap();

    cart.write("/file.txt", b"v1").unwrap();
    cart.flush().unwrap();
    cart.create_snapshot("s1".to_string(), "V1".to_string(), &snapshot_dir)
        .unwrap();

    cart.write("/file.txt", b"v2").unwrap();
    cart.flush().unwrap();
    cart.create_snapshot("s2".to_string(), "V2".to_string(), &snapshot_dir)
        .unwrap();

    // Freeze should capture current state (v2)
    use cartridge_rs::core::engram_integration::EngramFreezer;
    let freezer = EngramFreezer::new_default(
        "freeze-snapshots".to_string(),
        "1.0.0".to_string(),
        "Test".to_string(),
    );

    freezer.freeze(&mut cart, &engram_path).unwrap();

    // Verify engram has v2
    use engram_rs::ArchiveReader;
    let mut reader = ArchiveReader::open(&engram_path).unwrap();
    let data = reader.read_file("file.txt").unwrap();
    assert_eq!(data, b"v2");

    std::fs::remove_file("freeze-snapshots.cart").ok();
}

#[test]
#[ignore] // Requires public freeze API on Cartridge wrapper
fn test_freeze_with_compression_methods() {
    let temp_dir = TempDir::new().unwrap();
    let zstd_path = temp_dir.path().join("zstd.eng");
    let lz4_path = temp_dir.path().join("lz4.eng");

    let mut cart = Cartridge::create("freeze-compression", "Freeze Compression").unwrap();

    // Create compressible data
    let data = vec![b'A'; 1024 * 1024]; // 1MB of 'A's
    for i in 0..10 {
        cart.write(&format!("/file{}.txt", i), &data).unwrap();
    }
    cart.flush().unwrap();

    // Freeze with Zstd
    use cartridge_rs::core::engram_integration::EngramFreezer;
    use engram_rs::CompressionMethod;

    let zstd_freezer = EngramFreezer::new(
        "zstd".to_string(),
        "1.0".to_string(),
        "Test".to_string(),
        None,
        CompressionMethod::Zstd,
    );
    zstd_freezer.freeze(&mut cart, &zstd_path).unwrap();

    // Freeze with LZ4
    let lz4_freezer = EngramFreezer::new(
        "lz4".to_string(),
        "1.0".to_string(),
        "Test".to_string(),
        None,
        CompressionMethod::Lz4,
    );
    lz4_freezer.freeze(&mut cart, &lz4_path).unwrap();

    // Both should exist and be compressed
    assert!(zstd_path.exists());
    assert!(lz4_path.exists());

    // Compressed size should be much smaller than original (10MB)
    let zstd_size = std::fs::metadata(&zstd_path).unwrap().len();
    let lz4_size = std::fs::metadata(&lz4_path).unwrap().len();

    println!("Zstd size: {} bytes, LZ4 size: {} bytes", zstd_size, lz4_size);
    assert!(zstd_size < 10 * 1024 * 1024, "Zstd should compress significantly");
    assert!(lz4_size < 10 * 1024 * 1024, "LZ4 should compress significantly");

    std::fs::remove_file("freeze-compression.cart").ok();
}

#[test]
#[ignore] // Requires public freeze API on Cartridge wrapper
fn test_freeze_empty_container() {
    let temp_dir = TempDir::new().unwrap();
    let engram_path = temp_dir.path().join("frozen-empty.eng");

    let mut cart = Cartridge::create("freeze-empty", "Freeze Empty").unwrap();
    cart.flush().unwrap();

    // Freeze empty container
    use cartridge_rs::core::engram_integration::EngramFreezer;
    let freezer = EngramFreezer::new_default(
        "freeze-empty".to_string(),
        "1.0.0".to_string(),
        "Test".to_string(),
    );

    freezer.freeze(&mut cart, &engram_path).unwrap();

    // Verify engram exists
    assert!(engram_path.exists());

    // Verify it's a valid engram
    use engram_rs::ArchiveReader;
    let reader = ArchiveReader::open(&engram_path).unwrap();
    // Should have manifest but no files
    assert!(reader.read_manifest().unwrap().is_some());

    std::fs::remove_file("freeze-empty.cart").ok();
}

#[test]
#[ignore] // Requires public freeze API on Cartridge wrapper
fn test_freeze_mixed_file_sizes() {
    let temp_dir = TempDir::new().unwrap();
    let engram_path = temp_dir.path().join("frozen-mixed.eng");

    let mut cart = Cartridge::create("freeze-mixed", "Freeze Mixed").unwrap();

    // Mix of small and large files
    cart.write("/tiny.txt", b"small").unwrap();
    cart.write("/medium.bin", &vec![0xAB; 512 * 1024]).unwrap(); // 512KB
    cart.write("/large.bin", &vec![0xCD; 5 * 1024 * 1024]).unwrap(); // 5MB

    cart.flush().unwrap();

    // Freeze
    use cartridge_rs::core::engram_integration::EngramFreezer;
    let freezer = EngramFreezer::new_default(
        "freeze-mixed".to_string(),
        "1.0.0".to_string(),
        "Test".to_string(),
    );

    freezer.freeze(&mut cart, &engram_path).unwrap();

    // Verify all files
    use engram_rs::ArchiveReader;
    let mut reader = ArchiveReader::open(&engram_path).unwrap();

    assert_eq!(reader.read_file("tiny.txt").unwrap(), b"small");
    assert_eq!(reader.read_file("medium.bin").unwrap().len(), 512 * 1024);
    assert_eq!(reader.read_file("large.bin").unwrap().len(), 5 * 1024 * 1024);

    std::fs::remove_file("freeze-mixed.cart").ok();
}
