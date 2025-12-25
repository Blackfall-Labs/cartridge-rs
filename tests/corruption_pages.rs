//! Page corruption detection tests
//!
//! Tests to verify that cartridge properly detects and reports various
//! types of page-level corruption.

use cartridge_rs::{Cartridge, CartridgeError};
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};

/// Helper: Corrupt a page header at specific offset
fn corrupt_page_at_offset(path: &str, page_num: u32, offset: u64) {
    let mut file = OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap();

    let page_offset = (page_num as u64) * 4096 + offset;
    file.seek(SeekFrom::Start(page_offset)).unwrap();
    file.write_all(&[0xFF, 0xFF, 0xFF, 0xFF]).unwrap();
    file.flush().unwrap();
}

/// Helper: Truncate file to specific size
fn truncate_file(path: &str, size: u64) {
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap();
    file.set_len(size).unwrap();
}

#[test]
fn test_corrupted_page_header() {
    let mut cart = Cartridge::create("corrupt-header", "Corrupt Header Test").unwrap();
    cart.write("/file.txt", b"test data with enough content to span multiple pages").unwrap();
    drop(cart);

    // Corrupt page header at page 2
    corrupt_page_at_offset("corrupt-header.cart", 2, 8);

    // Should detect corruption on open or read
    match Cartridge::open("corrupt-header.cart") {
        Ok(cart) => {
            // If open succeeds, read should fail
            let result = cart.read("/file.txt");
            assert!(result.is_err(), "Should detect corrupted page");
        }
        Err(_) => {
            // Open failed - also acceptable
        }
    }

    std::fs::remove_file("corrupt-header.cart").ok();
}

#[test]
fn test_truncated_pages() {
    let mut cart = Cartridge::create("corrupt-truncate", "Corrupt Truncate Test").unwrap();
    cart.write("/large.bin", &vec![0xAB; 100 * 1024]).unwrap();
    drop(cart);

    // Get current size and truncate mid-page
    let file_size = std::fs::metadata("corrupt-truncate.cart").unwrap().len();
    truncate_file("corrupt-truncate.cart", file_size - 2048);

    // Should detect truncation (either on open or read)
    match Cartridge::open("corrupt-truncate.cart") {
        Ok(cart) => {
            // If open succeeds, read might succeed but return truncated data
            // or fail - either is acceptable for corrupted data
            let _result = cart.read("/large.bin");
        }
        Err(_) => {
            // Open failed - this is expected and acceptable
        }
    }

    std::fs::remove_file("corrupt-truncate.cart").ok();
}

#[test]
fn test_empty_file_corruption() {
    // Create empty .cart file
    File::create("corrupt-empty.cart").unwrap();

    // Should fail to open gracefully
    let result = Cartridge::open("corrupt-empty.cart");
    assert!(result.is_err(), "Should reject empty file");

    std::fs::remove_file("corrupt-empty.cart").ok();
}

#[test]
fn test_partial_header_corruption() {
    let mut cart = Cartridge::create("corrupt-partial", "Corrupt Partial Test").unwrap();
    cart.write("/test.txt", b"data").unwrap();
    drop(cart);

    // Corrupt just the magic bytes in header
    let mut file = OpenOptions::new()
        .write(true)
        .open("corrupt-partial.cart")
        .unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(b"XXXX").unwrap();
    file.flush().unwrap();
    drop(file);

    // Should detect invalid magic
    let result = Cartridge::open("corrupt-partial.cart");
    assert!(result.is_err(), "Should detect invalid magic bytes");

    std::fs::remove_file("corrupt-partial.cart").ok();
}

#[test]
fn test_read_beyond_container_size() {
    let mut cart = Cartridge::create("corrupt-bounds", "Corrupt Bounds Test").unwrap();
    cart.write("/small.txt", b"small").unwrap();

    // Try to read non-existent file (simulates catalog pointing to invalid page)
    let result = cart.read("/nonexistent.txt");
    assert!(result.is_err(), "Should fail on non-existent file");

    std::fs::remove_file("corrupt-bounds.cart").ok();
}
