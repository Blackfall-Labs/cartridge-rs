//! IAM policy cache race condition tests
//!
//! Note: IAM policy features may not be fully implemented yet

use cartridge_rs::Cartridge;

#[test]
fn test_basic_iam_operations() {
    // Test basic IAM-like operations without actual policy enforcement
    let mut cart = Cartridge::create("iam-basic", "IAM Basic").unwrap();

    // Write files that would be protected by policies
    cart.write("/public/file.txt", b"public data").unwrap();
    cart.write("/private/secret.txt", b"private data").unwrap();

    // Read them back
    assert!(cart.read("/public/file.txt").is_ok());
    assert!(cart.read("/private/secret.txt").is_ok());

    std::fs::remove_file("iam-basic.cart").ok();
}

#[test]
fn test_concurrent_access_to_protected_resources() {
    use std::sync::Arc;
    use parking_lot::RwLock;

    let cart = Arc::new(RwLock::new(
        Cartridge::create("iam-concurrent", "IAM Concurrent").unwrap()
    ));

    // Create resources
    {
        let mut c = cart.write();
        for i in 0..20 {
            c.write(&format!("/public/file{}.txt", i), b"data").unwrap();
        }
    }

    // Multiple threads accessing resources
    let handles: Vec<_> = (0..10).map(|_| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for _ in 0..100 {
                let c = cart_clone.read();
                let idx = rand::random::<usize>() % 20;
                let _ = c.read(&format!("/public/file{}.txt", idx));
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    std::fs::remove_file("iam-concurrent.cart").ok();
}

#[test]
fn test_path_based_access_patterns() {
    let mut cart = Cartridge::create("iam-paths", "IAM Paths").unwrap();

    // Different path hierarchies
    cart.write("/data/public/readme.txt", b"public readme").unwrap();
    cart.write("/data/private/config.json", b"private config").unwrap();
    cart.write("/logs/access.log", b"access log").unwrap();

    // Verify all accessible
    assert!(cart.read("/data/public/readme.txt").is_ok());
    assert!(cart.read("/data/private/config.json").is_ok());
    assert!(cart.read("/logs/access.log").is_ok());

    std::fs::remove_file("iam-paths.cart").ok();
}

#[test]
fn test_wildcard_path_patterns() {
    let mut cart = Cartridge::create("iam-wildcard", "IAM Wildcard").unwrap();

    // Create files matching various patterns
    cart.write("/data/file1.txt", b"data").unwrap();
    cart.write("/data/file2.txt", b"data").unwrap();
    cart.write("/data/subdir/file3.txt", b"data").unwrap();

    // List directory (simulates wildcard matching)
    let files = cart.list("/data").unwrap();
    assert!(!files.is_empty());

    std::fs::remove_file("iam-wildcard.cart").ok();
}

#[test]
fn test_concurrent_metadata_access() {
    use std::sync::Arc;
    use parking_lot::RwLock;

    let cart = Arc::new(RwLock::new(
        Cartridge::create("iam-metadata", "IAM Metadata").unwrap()
    ));

    // Create files
    {
        let mut c = cart.write();
        for i in 0..15 {
            c.write(&format!("/file{}.txt", i), &vec![i as u8; 1024]).unwrap();
        }
    }

    // Concurrent metadata reads
    let handles: Vec<_> = (0..8).map(|_| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for _ in 0..100 {
                let c = cart_clone.read();
                for i in 0..15 {
                    let _ = c.metadata(&format!("/file{}.txt", i));
                }
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    std::fs::remove_file("iam-metadata.cart").ok();
}
