//! Concurrent readers/writers stress tests

use cartridge_rs::Cartridge;
use std::sync::Arc;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn test_10_concurrent_readers_2_writers() {
    let cart = Arc::new(RwLock::new(
        Cartridge::create("concurrent-stress", "Concurrent Stress").unwrap()
    ));

    // Pre-populate
    {
        let mut c = cart.write();
        for i in 0..50 {
            c.write(&format!("/file{}.txt", i), format!("data{}", i).as_bytes()).unwrap();
        }
    }

    let handles: Vec<_> = (0..12).map(|thread_id| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            if thread_id < 2 {
                // Writer thread
                for i in 0..100 {
                    let mut c = cart_clone.write();
                    c.write(&format!("/writer{}_{}.txt", thread_id, i), b"new data").unwrap();
                }
            } else {
                // Reader thread
                for _ in 0..1000 {
                    let c = cart_clone.read();
                    let idx = rand::random::<usize>() % 50;
                    let _ = c.read(&format!("/file{}.txt", idx));
                }
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    // Verify integrity
    let c = cart.read();
    assert!(c.list("/").unwrap().len() >= 50);

    std::fs::remove_file("concurrent-stress.cart").ok();
}

#[test]
fn test_reader_writer_lock_fairness() {
    let cart = Arc::new(RwLock::new(
        Cartridge::create("fairness", "Fairness").unwrap()
    ));

    let read_count = Arc::new(AtomicUsize::new(0));
    let write_count = Arc::new(AtomicUsize::new(0));

    // Continuous readers
    let reader_handles: Vec<_> = (0..10).map(|_| {
        let cart_clone = cart.clone();
        let read_count_clone = read_count.clone();
        std::thread::spawn(move || {
            for _ in 0..50 {
                let c = cart_clone.read();
                let _ = c.list("/");
                read_count_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
    }).collect();

    // Continuous writers
    let writer_handles: Vec<_> = (0..5).map(|thread_id| {
        let cart_clone = cart.clone();
        let write_count_clone = write_count.clone();
        std::thread::spawn(move || {
            for i in 0..50 {
                let mut c = cart_clone.write();
                c.write(&format!("/w{}_{}.txt", thread_id, i), b"data").unwrap();
                write_count_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
    }).collect();

    for h in reader_handles.into_iter().chain(writer_handles) {
        h.join().unwrap();
    }

    // Both readers and writers should have made progress
    assert_eq!(read_count.load(Ordering::Relaxed), 10 * 50);
    assert_eq!(write_count.load(Ordering::Relaxed), 5 * 50);

    std::fs::remove_file("fairness.cart").ok();
}

#[test]
fn test_concurrent_deletes() {
    let cart = Arc::new(RwLock::new(
        Cartridge::create("concurrent-delete", "Concurrent Delete").unwrap()
    ));

    // Pre-populate
    {
        let mut c = cart.write();
        for i in 0..100 {
            c.write(&format!("/file{}.txt", i), b"data").unwrap();
        }
    }

    // Multiple threads delete different files
    let handles: Vec<_> = (0..10).map(|thread_id| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for i in 0..10 {
                let file_idx = thread_id * 10 + i;
                let mut c = cart_clone.write();
                c.delete(&format!("/file{}.txt", file_idx)).unwrap();
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    // Verify all or most deleted (may include directory entries)
    let c = cart.read();
    let remaining = c.list("/").unwrap();
    assert!(remaining.len() <= 10, "Expected <= 10 remaining, got {}", remaining.len());

    std::fs::remove_file("concurrent-delete.cart").ok();
}

#[test]
fn test_concurrent_mixed_operations() {
    let cart = Arc::new(RwLock::new(
        Cartridge::create("mixed-ops", "Mixed Ops").unwrap()
    ));

    // Writers, readers, deleters
    let handles: Vec<_> = (0..15).map(|thread_id| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            match thread_id % 3 {
                0 => {
                    // Writer
                    for i in 0..50 {
                        let mut c = cart_clone.write();
                        c.write(&format!("/w{}_{}.txt", thread_id, i), b"data").unwrap();
                    }
                }
                1 => {
                    // Reader
                    for _ in 0..100 {
                        let c = cart_clone.read();
                        let _ = c.list("/");
                    }
                }
                2 => {
                    // Deleter - create then delete
                    for i in 0..30 {
                        let mut c = cart_clone.write();
                        let path = format!("/temp{}_{}.txt", thread_id, i);
                        c.write(&path, b"temp").unwrap();
                        c.delete(&path).unwrap();
                    }
                }
                _ => {}
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    std::fs::remove_file("mixed-ops.cart").ok();
}

#[test]
fn test_concurrent_metadata_reads() {
    let cart = Arc::new(RwLock::new(
        Cartridge::create("metadata-reads", "Metadata Reads").unwrap()
    ));

    // Pre-populate
    {
        let mut c = cart.write();
        for i in 0..30 {
            c.write(&format!("/file{}.txt", i), &vec![i as u8; 1024]).unwrap();
        }
    }

    // Multiple threads reading metadata
    let handles: Vec<_> = (0..8).map(|_| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for _ in 0..200 {
                let c = cart_clone.read();
                let idx = rand::random::<usize>() % 30;
                let meta = c.metadata(&format!("/file{}.txt", idx)).unwrap();
                assert_eq!(meta.size, 1024);
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    std::fs::remove_file("metadata-reads.cart").ok();
}
