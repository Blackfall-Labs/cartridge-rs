//! Buffer pool coherency tests

use cartridge_rs::Cartridge;
use std::sync::Arc;
use parking_lot::RwLock;

#[test]
fn test_concurrent_reads_same_file() {
    let cart = Arc::new(RwLock::new(
        Cartridge::create("buffer-pool-reads", "Buffer Pool Reads").unwrap()
    ));

    // Create file
    {
        let mut c = cart.write();
        c.write("/cached.txt", b"cached data").unwrap();
    }

    // Multiple readers of same file (should be coherent)
    let handles: Vec<_> = (0..20).map(|_| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for _ in 0..50 {
                let c = cart_clone.read();
                let data = c.read("/cached.txt").unwrap();
                assert_eq!(data, b"cached data");
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    std::fs::remove_file("buffer-pool-reads.cart").ok();
}

#[test]
fn test_write_invalidation() {
    let cart = Arc::new(RwLock::new(
        Cartridge::create("buffer-pool-invalidate", "Buffer Pool Invalidate").unwrap()
    ));

    {
        let mut c = cart.write();
        c.write("/file.txt", b"version 1").unwrap();
    }

    // Reader thread
    let cart_clone1 = cart.clone();
    let reader_handle = std::thread::spawn(move || {
        let c = cart_clone1.read();
        let data1 = c.read("/file.txt").unwrap();
        assert_eq!(data1, b"version 1");
    });

    reader_handle.join().unwrap();

    // Writer thread (updates file)
    {
        let mut c = cart.write();
        c.write("/file.txt", b"version 2").unwrap();
    }

    // New reader should see updated version
    let c = cart.read();
    let final_data = c.read("/file.txt").unwrap();
    assert_eq!(final_data, b"version 2");

    std::fs::remove_file("buffer-pool-invalidate.cart").ok();
}

#[test]
fn test_large_file_caching() {
    let cart = Arc::new(RwLock::new(
        Cartridge::create("buffer-large", "Buffer Large").unwrap()
    ));

    // Create large file
    {
        let mut c = cart.write();
        c.write("/large.bin", &vec![0xAB; 512 * 1024]).unwrap();
    }

    // Multiple concurrent reads
    let handles: Vec<_> = (0..10).map(|_| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for _ in 0..20 {
                let c = cart_clone.read();
                let data = c.read("/large.bin").unwrap();
                assert_eq!(data.len(), 512 * 1024);
                assert!(data.iter().all(|&b| b == 0xAB));
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    std::fs::remove_file("buffer-large.cart").ok();
}

#[test]
fn test_multiple_files_coherency() {
    let cart = Arc::new(RwLock::new(
        Cartridge::create("buffer-multi", "Buffer Multi").unwrap()
    ));

    // Create multiple files
    {
        let mut c = cart.write();
        for i in 0..10 {
            c.write(&format!("/file{}.txt", i), format!("data{}", i).as_bytes()).unwrap();
        }
    }

    // Concurrent access to different files
    let handles: Vec<_> = (0..10).map(|file_id| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for _ in 0..100 {
                let c = cart_clone.read();
                let data = c.read(&format!("/file{}.txt", file_id)).unwrap();
                assert_eq!(data, format!("data{}", file_id).as_bytes());
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    std::fs::remove_file("buffer-multi.cart").ok();
}

#[test]
fn test_read_write_interleaving() {
    let cart = Arc::new(RwLock::new(
        Cartridge::create("buffer-interleave", "Buffer Interleave").unwrap()
    ));

    {
        let mut c = cart.write();
        c.write("/counter.txt", b"0").unwrap();
    }

    // Alternate reads and writes
    for i in 1..=10 {
        // Read
        {
            let c = cart.read();
            let data = c.read("/counter.txt").unwrap();
            assert!(!data.is_empty());
        }

        // Write
        {
            let mut c = cart.write();
            c.write("/counter.txt", format!("{}", i).as_bytes()).unwrap();
        }

        // Read again
        {
            let c = cart.read();
            let data = c.read("/counter.txt").unwrap();
            assert_eq!(data, format!("{}", i).as_bytes());
        }
    }

    std::fs::remove_file("buffer-interleave.cart").ok();
}

#[test]
fn test_delete_invalidation() {
    let cart = Arc::new(RwLock::new(
        Cartridge::create("buffer-delete", "Buffer Delete").unwrap()
    ));

    {
        let mut c = cart.write();
        c.write("/temp.txt", b"temporary data").unwrap();
    }

    // Read file
    {
        let c = cart.read();
        assert!(c.read("/temp.txt").is_ok());
    }

    // Delete file
    {
        let mut c = cart.write();
        c.delete("/temp.txt").unwrap();
    }

    // Should no longer exist
    {
        let c = cart.read();
        assert!(c.read("/temp.txt").is_err());
    }

    std::fs::remove_file("buffer-delete.cart").ok();
}
