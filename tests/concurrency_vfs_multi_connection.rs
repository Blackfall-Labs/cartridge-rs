//! SQLite VFS multi-connection concurrency tests
//!
//! Note: VFS tests currently simplified due to API constraints

use cartridge_rs::Cartridge;

#[test]
fn test_sequential_vfs_operations() {
    // Test sequential VFS-like operations
    let mut cart = Cartridge::create("vfs-seq", "VFS Sequential").unwrap();

    // Simulate database operations
    cart.write("/db.sqlite", b"SQLite format 3\0initial data").unwrap();

    // Multiple reads
    for _ in 0..10 {
        let data = cart.read("/db.sqlite").unwrap();
        assert!(!data.is_empty());
    }

    // Update
    cart.write("/db.sqlite", b"SQLite format 3\0updated data").unwrap();
    let data = cart.read("/db.sqlite").unwrap();
    assert!(data.starts_with(b"SQLite format 3\0updated"));

    std::fs::remove_file("vfs-seq.cart").ok();
}

#[test]
fn test_vfs_like_concurrent_reads() {
    use std::sync::Arc;
    use parking_lot::RwLock;

    let cart = Arc::new(RwLock::new(
        Cartridge::create("vfs-concurrent", "VFS Concurrent").unwrap()
    ));

    // Create test database
    {
        let mut c = cart.write();
        c.write("/db.sqlite", b"SQLite format 3\0test database content").unwrap();
    }

    // Multiple reader threads
    let handles: Vec<_> = (0..5).map(|_| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for _ in 0..50 {
                let c = cart_clone.read();
                let data = c.read("/db.sqlite").unwrap();
                assert!(!data.is_empty());
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    std::fs::remove_file("vfs-concurrent.cart").ok();
}

#[test]
fn test_vfs_write_then_read() {
    use std::sync::Arc;
    use parking_lot::RwLock;

    let cart = Arc::new(RwLock::new(
        Cartridge::create("vfs-wr", "VFS Write-Read").unwrap()
    ));

    // Writer thread
    let cart_clone1 = cart.clone();
    let writer = std::thread::spawn(move || {
        let mut c = cart_clone1.write();
        for i in 0..20 {
            let content = format!("SQLite format 3\0row{}", i);
            c.write("/db.sqlite", content.as_bytes()).unwrap();
        }
    });

    // Wait for writer to start
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Reader threads
    let reader_handles: Vec<_> = (0..3).map(|_| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for _ in 0..30 {
                let c = cart_clone.read();
                let _ = c.read("/db.sqlite");
            }
        })
    }).collect();

    writer.join().unwrap();
    for h in reader_handles {
        h.join().unwrap();
    }

    std::fs::remove_file("vfs-wr.cart").ok();
}

#[test]
fn test_vfs_cleanup_consistency() {
    // Test that rapid create/destroy doesn't leave inconsistencies
    for iteration in 0..20 {
        let slug = format!("vfs-cleanup-{}", iteration);
        let mut cart = Cartridge::create(&slug, "VFS Cleanup").unwrap();

        cart.write("/db.sqlite", b"SQLite format 3\0test").unwrap();

        // Verify can read
        let data = cart.read("/db.sqlite").unwrap();
        assert!(!data.is_empty());

        drop(cart);

        // Cleanup
        std::fs::remove_file(format!("{}.cart", slug)).ok();
    }
}

#[test]
fn test_multiple_database_files() {
    use std::sync::Arc;
    use parking_lot::RwLock;

    let cart = Arc::new(RwLock::new(
        Cartridge::create("multi-db", "Multi DB").unwrap()
    ));

    // Create multiple database files
    {
        let mut c = cart.write();
        c.write("/db1.sqlite", b"SQLite format 3\0database 1").unwrap();
        c.write("/db2.sqlite", b"SQLite format 3\0database 2").unwrap();
        c.write("/db3.sqlite", b"SQLite format 3\0database 3").unwrap();
    }

    // Concurrent access to different databases
    let handles: Vec<_> = (0..3).map(|db_idx| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for _ in 0..50 {
                let c = cart_clone.read();
                let data = c.read(&format!("/db{}.sqlite", db_idx + 1)).unwrap();
                assert!(!data.is_empty());
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    std::fs::remove_file("multi-db.cart").ok();
}
