//! Snapshot consistency tests
//!
//! Note: Snapshot functionality may not be fully implemented yet

use cartridge_rs::Cartridge;

#[test]
fn test_basic_snapshot_workflow() {
    let mut cart = Cartridge::create("snapshot-basic", "Snapshot Basic").unwrap();

    // Initial data
    for i in 0..20 {
        cart.write(&format!("/file{}.txt", i), format!("v0_{}", i).as_bytes()).unwrap();
    }

    // Note: Snapshot API may not exist yet - this tests regular operations
    // that would be needed for snapshots

    // Modify files
    for i in 0..20 {
        cart.write(&format!("/file{}.txt", i), format!("v1_{}", i).as_bytes()).unwrap();
    }

    // Verify modifications
    for i in 0..20 {
        let data = cart.read(&format!("/file{}.txt", i)).unwrap();
        assert_eq!(data, format!("v1_{}", i).as_bytes());
    }

    std::fs::remove_file("snapshot-basic.cart").ok();
}

#[test]
fn test_concurrent_reads_during_writes() {
    use std::sync::Arc;
    use parking_lot::RwLock;

    let cart = Arc::new(RwLock::new(
        Cartridge::create("snapshot-reads", "Snapshot Reads").unwrap()
    ));

    // Pre-populate
    {
        let mut c = cart.write();
        for i in 0..30 {
            c.write(&format!("/file{}.txt", i), b"original").unwrap();
        }
    }

    // Writer thread (modifies data)
    let cart_clone1 = cart.clone();
    let writer_handle = std::thread::spawn(move || {
        for i in 0..30 {
            let mut c = cart_clone1.write();
            c.write(&format!("/file{}.txt", i), b"modified").unwrap();
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
    });

    // Reader threads
    let reader_handles: Vec<_> = (0..5).map(|_| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for _ in 0..100 {
                let c = cart_clone.read();
                let idx = rand::random::<usize>() % 30;
                let _ = c.read(&format!("/file{}.txt", idx));
            }
        })
    }).collect();

    writer_handle.join().unwrap();
    for h in reader_handles {
        h.join().unwrap();
    }

    std::fs::remove_file("snapshot-reads.cart").ok();
}

#[test]
fn test_data_consistency_across_operations() {
    let mut cart = Cartridge::create("consistency", "Consistency").unwrap();

    // Create initial state
    cart.write("/file.txt", b"version 1").unwrap();

    // Read back
    let v1 = cart.read("/file.txt").unwrap();
    assert_eq!(v1, b"version 1");

    // Update
    cart.write("/file.txt", b"version 2").unwrap();

    // Read again
    let v2 = cart.read("/file.txt").unwrap();
    assert_eq!(v2, b"version 2");

    // Close and reopen
    drop(cart);
    let cart = Cartridge::open("consistency.cart").unwrap();

    // Should still have version 2
    let v2_after_reopen = cart.read("/file.txt").unwrap();
    assert_eq!(v2_after_reopen, b"version 2");

    std::fs::remove_file("consistency.cart").ok();
}

#[test]
fn test_concurrent_file_creation() {
    use std::sync::Arc;
    use parking_lot::RwLock;

    let cart = Arc::new(RwLock::new(
        Cartridge::create("concurrent-create", "Concurrent Create").unwrap()
    ));

    // Multiple threads creating files
    let handles: Vec<_> = (0..5).map(|thread_id| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for i in 0..20 {
                let mut c = cart_clone.write();
                c.write(&format!("/t{}_{}.txt", thread_id, i), b"data").unwrap();
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    // Verify all files created
    let c = cart.read();
    let files = c.list("/").unwrap();
    assert!(files.len() >= 100);

    std::fs::remove_file("concurrent-create.cart").ok();
}

#[test]
fn test_isolation_between_operations() {
    use std::sync::Arc;
    use parking_lot::RwLock;

    let cart = Arc::new(RwLock::new(
        Cartridge::create("isolation", "Isolation").unwrap()
    ));

    {
        let mut c = cart.write();
        c.write("/file.txt", b"initial").unwrap();
    }

    // Reader should not see incomplete writes
    let cart_clone1 = cart.clone();
    let reader = std::thread::spawn(move || {
        for _ in 0..100 {
            let c = cart_clone1.read();
            let data = c.read("/file.txt").unwrap();
            // Should be either "initial" or "updated", never partial
            assert!(data == b"initial" || data == b"updated");
        }
    });

    // Writer updates file
    let cart_clone2 = cart.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let mut c = cart_clone2.write();
        c.write("/file.txt", b"updated").unwrap();
    });

    reader.join().unwrap();
    writer.join().unwrap();

    std::fs::remove_file("isolation.cart").ok();
}
