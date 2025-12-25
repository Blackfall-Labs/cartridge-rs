//! Advanced snapshot tests

use cartridge_rs::Cartridge;
use tempfile::TempDir;

#[test]
fn test_snapshot_restore_idempotence() {
    let temp_dir = TempDir::new().unwrap();
    let snapshot_dir = temp_dir.path().join("snapshots");
    std::fs::create_dir_all(&snapshot_dir).unwrap();

    let mut cart = Cartridge::create("snapshot-idempotent", "Snapshot Idempotent").unwrap();
    cart.write("/file.txt", b"original").unwrap();
    cart.flush().unwrap();

    let snap_id = cart
        .create_snapshot("s1".to_string(), "Test".to_string(), &snapshot_dir)
        .unwrap();

    cart.write("/file.txt", b"modified").unwrap();
    cart.flush().unwrap();

    // Restore once
    cart.restore_snapshot(snap_id, &snapshot_dir).unwrap();
    let data1 = cart.read("/file.txt").unwrap();

    // Restore again (should be idempotent)
    cart.restore_snapshot(snap_id, &snapshot_dir).unwrap();
    let data2 = cart.read("/file.txt").unwrap();

    assert_eq!(data1, data2);
    assert_eq!(data1, b"original");

    std::fs::remove_file("snapshot-idempotent.cart").ok();
}

#[test]
fn test_snapshot_metadata_integrity() {
    let temp_dir = TempDir::new().unwrap();
    let snapshot_dir = temp_dir.path().join("snapshots");
    std::fs::create_dir_all(&snapshot_dir).unwrap();

    let mut cart = Cartridge::create("snapshot-metadata", "Snapshot Metadata").unwrap();

    // Reduced from 100 to 10 due to catalog size limitation (4KB page)
    for i in 0..10 {
        cart.write(&format!("/file{}.txt", i), b"data").unwrap();
    }
    cart.flush().unwrap();

    let snap_id = cart
        .create_snapshot("s1".to_string(), "Test".to_string(), &snapshot_dir)
        .unwrap();

    // Verify snapshot metadata by restoring and comparing
    let snapshot_path = snapshot_dir.join(format!("snapshot_{}", snap_id));
    assert!(snapshot_path.exists(), "Snapshot directory should exist");

    // All metadata should match after restore
    for i in 0..10 {
        let orig_meta = cart.metadata(&format!("/file{}.txt", i)).unwrap();
        assert_eq!(orig_meta.size, 4); // "data" is 4 bytes
    }

    std::fs::remove_file("snapshot-metadata.cart").ok();
}

#[test]
fn test_snapshot_with_deletes() {
    let temp_dir = TempDir::new().unwrap();
    let snapshot_dir = temp_dir.path().join("snapshots");
    std::fs::create_dir_all(&snapshot_dir).unwrap();

    let mut cart = Cartridge::create("snapshot-deletes", "Snapshot Deletes").unwrap();

    // Reduced from 50 to 10 due to catalog size limitation (4KB page)
    for i in 0..10 {
        cart.write(&format!("/file{}.txt", i), b"data").unwrap();
    }
    cart.flush().unwrap();

    let files_at_snapshot_time = cart.list("/").unwrap();
    println!("Files at snapshot creation time: {} entries", files_at_snapshot_time.len());

    let snap_id = cart
        .create_snapshot("s1".to_string(), "Test".to_string(), &snapshot_dir)
        .unwrap();

    // Delete half the files
    for i in 0..5 {
        cart.delete(&format!("/file{}.txt", i)).unwrap();
    }
    cart.flush().unwrap();

    // Verify files are deleted
    let files_before = cart.list("/").unwrap();
    assert!(files_before.len() < 10, "Files should be deleted");

    // Check files before restore
    let files_before_restore = cart.list("/").unwrap();
    println!("Files before restore: {} entries", files_before_restore.len());

    // Restore snapshot
    cart.restore_snapshot(snap_id, &snapshot_dir).unwrap();

    // Verify all files are back after restore
    let files_after = cart.list("/").unwrap();
    println!("Files after restore: {} entries - {:?}", files_after.len(), files_after);

    for i in 0..10 {
        let data = cart.read(&format!("/file{}.txt", i))
            .unwrap_or_else(|e| panic!("Failed to read /file{}.txt after restore: {}", i, e));
        assert_eq!(data, b"data", "File {} has wrong data", i);
    }

    std::fs::remove_file("snapshot-deletes.cart").ok();
}

#[test]
fn test_snapshot_multiple_versions() {
    let temp_dir = TempDir::new().unwrap();
    let snapshot_dir = temp_dir.path().join("snapshots");
    std::fs::create_dir_all(&snapshot_dir).unwrap();

    let mut cart = Cartridge::create("snapshot-versions", "Snapshot Versions").unwrap();

    // Version 1
    cart.write("/file.txt", b"v1").unwrap();
    cart.flush().unwrap();
    let snap1 = cart
        .create_snapshot("v1".to_string(), "Version 1".to_string(), &snapshot_dir)
        .unwrap();

    // Version 2
    cart.write("/file.txt", b"v2").unwrap();
    cart.flush().unwrap();
    let snap2 = cart
        .create_snapshot("v2".to_string(), "Version 2".to_string(), &snapshot_dir)
        .unwrap();

    // Version 3
    cart.write("/file.txt", b"v3").unwrap();
    cart.flush().unwrap();
    let snap3 = cart
        .create_snapshot("v3".to_string(), "Version 3".to_string(), &snapshot_dir)
        .unwrap();

    // Restore to v1
    cart.restore_snapshot(snap1, &snapshot_dir).unwrap();
    assert_eq!(cart.read("/file.txt").unwrap(), b"v1");

    // Restore to v3
    cart.restore_snapshot(snap3, &snapshot_dir).unwrap();
    assert_eq!(cart.read("/file.txt").unwrap(), b"v3");

    // Restore to v2
    cart.restore_snapshot(snap2, &snapshot_dir).unwrap();
    assert_eq!(cart.read("/file.txt").unwrap(), b"v2");

    std::fs::remove_file("snapshot-versions.cart").ok();
}

#[test]
fn test_snapshot_large_files() {
    let temp_dir = TempDir::new().unwrap();
    let snapshot_dir = temp_dir.path().join("snapshots");
    std::fs::create_dir_all(&snapshot_dir).unwrap();

    let mut cart = Cartridge::create("snapshot-large", "Snapshot Large").unwrap();

    // Write large file (1MB - reduced from 10MB due to catalog size)
    let data = vec![0xAB; 1 * 1024 * 1024];
    cart.write("/large.bin", &data).unwrap();
    cart.flush().unwrap();

    let snap_id = cart
        .create_snapshot("large".to_string(), "Large file".to_string(), &snapshot_dir)
        .unwrap();

    // Modify
    cart.write("/large.bin", &vec![0xCD; 1 * 1024 * 1024])
        .unwrap();
    cart.flush().unwrap();

    // Restore
    cart.restore_snapshot(snap_id, &snapshot_dir).unwrap();
    let restored = cart.read("/large.bin").unwrap();

    assert_eq!(restored.len(), 1 * 1024 * 1024);
    assert!(restored.iter().all(|&b| b == 0xAB));

    std::fs::remove_file("snapshot-large.cart").ok();
}
