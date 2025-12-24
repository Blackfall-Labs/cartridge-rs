//! Integration tests for CopyObject and DeleteObjects operations

use cartridge::Cartridge;
use cartridge_s3::CartridgeS3Backend;
use parking_lot::RwLock;
use std::sync::Arc;

/// Helper to create a backend for testing
fn create_backend() -> CartridgeS3Backend {
    let cartridge = Cartridge::create("test-cart", "Test Cartridge").unwrap();
    let cart_arc = Arc::new(RwLock::new(cartridge));
    CartridgeS3Backend::new(cart_arc)
}

#[test]
fn test_copy_object_same_bucket() {
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    // Create source object
    let source_data = b"Hello, World!".to_vec();
    let etag1 = backend
        .put_object("test-bucket", "source.txt", &source_data)
        .unwrap();

    // Copy object within same bucket
    let etag2 = backend
        .copy_object("test-bucket", "source.txt", "test-bucket", "copy.txt")
        .unwrap();

    // Both should have same ETag (same content)
    assert_eq!(etag1, etag2);

    // Verify copy exists and has same data
    let copied_data = backend.get_object("test-bucket", "copy.txt").unwrap();
    assert_eq!(copied_data, source_data);

    // Verify source still exists
    let source_data_check = backend.get_object("test-bucket", "source.txt").unwrap();
    assert_eq!(source_data_check, source_data);
}

#[test]
fn test_copy_object_different_buckets() {
    let backend = create_backend();
    backend.create_bucket("source-bucket").unwrap();
    backend.create_bucket("dest-bucket").unwrap();

    // Create source object
    let source_data = b"Cross-bucket copy test".to_vec();
    backend
        .put_object("source-bucket", "file.txt", &source_data)
        .unwrap();

    // Copy to different bucket
    backend
        .copy_object("source-bucket", "file.txt", "dest-bucket", "file.txt")
        .unwrap();

    // Verify copy exists in destination
    let copied_data = backend.get_object("dest-bucket", "file.txt").unwrap();
    assert_eq!(copied_data, source_data);

    // Verify source still exists in original bucket
    let source_data_check = backend.get_object("source-bucket", "file.txt").unwrap();
    assert_eq!(source_data_check, source_data);
}

#[test]
fn test_copy_object_overwrite_existing() {
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    // Create source and destination objects with different content
    let source_data = b"New content".to_vec();
    let dest_data = b"Old content".to_vec();

    backend
        .put_object("test-bucket", "source.txt", &source_data)
        .unwrap();
    backend
        .put_object("test-bucket", "dest.txt", &dest_data)
        .unwrap();

    // Copy should overwrite destination
    backend
        .copy_object("test-bucket", "source.txt", "test-bucket", "dest.txt")
        .unwrap();

    // Verify destination now has source content
    let result_data = backend.get_object("test-bucket", "dest.txt").unwrap();
    assert_eq!(result_data, source_data);
}

#[test]
fn test_copy_object_large_file() {
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    // Create a 1MB file
    let source_data = vec![0xAB; 1024 * 1024];
    backend
        .put_object("test-bucket", "large.bin", &source_data)
        .unwrap();

    // Copy the large file
    backend
        .copy_object("test-bucket", "large.bin", "test-bucket", "large-copy.bin")
        .unwrap();

    // Verify copy is identical
    let copied_data = backend.get_object("test-bucket", "large-copy.bin").unwrap();
    assert_eq!(copied_data.len(), source_data.len());
    assert_eq!(copied_data, source_data);
}

#[test]
fn test_copy_object_nonexistent_source() {
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    // Attempt to copy nonexistent object
    let result = backend.copy_object("test-bucket", "nonexistent.txt", "test-bucket", "dest.txt");
    assert!(result.is_err());
}

#[test]
fn test_delete_objects_single() {
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    // Create object
    backend
        .put_object("test-bucket", "file1.txt", b"data")
        .unwrap();

    // Delete single object
    let results = backend
        .delete_objects("test-bucket", &["file1.txt".to_string()])
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "file1.txt");
    assert_eq!(results[0].1, true); // Success
    assert!(results[0].2.is_none()); // No error

    // Verify object is gone
    assert!(backend.get_object("test-bucket", "file1.txt").is_err());
}

#[test]
fn test_delete_objects_multiple() {
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    // Create multiple objects
    backend
        .put_object("test-bucket", "file1.txt", b"data1")
        .unwrap();
    backend
        .put_object("test-bucket", "file2.txt", b"data2")
        .unwrap();
    backend
        .put_object("test-bucket", "file3.txt", b"data3")
        .unwrap();

    // Bulk delete
    let keys = vec![
        "file1.txt".to_string(),
        "file2.txt".to_string(),
        "file3.txt".to_string(),
    ];
    let results = backend.delete_objects("test-bucket", &keys).unwrap();

    assert_eq!(results.len(), 3);
    for (i, result) in results.iter().enumerate() {
        assert_eq!(result.0, keys[i]);
        assert_eq!(result.1, true); // Success
        assert!(result.2.is_none()); // No error
    }

    // Verify all objects are gone
    assert!(backend.get_object("test-bucket", "file1.txt").is_err());
    assert!(backend.get_object("test-bucket", "file2.txt").is_err());
    assert!(backend.get_object("test-bucket", "file3.txt").is_err());
}

#[test]
fn test_delete_objects_partial_success() {
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    // Create only some of the objects
    backend
        .put_object("test-bucket", "exists1.txt", b"data1")
        .unwrap();
    backend
        .put_object("test-bucket", "exists2.txt", b"data2")
        .unwrap();

    // Try to delete both existing and nonexistent
    let keys = vec![
        "exists1.txt".to_string(),
        "nonexistent.txt".to_string(),
        "exists2.txt".to_string(),
    ];
    let results = backend.delete_objects("test-bucket", &keys).unwrap();

    assert_eq!(results.len(), 3);

    // Check exists1.txt succeeded
    assert_eq!(results[0].0, "exists1.txt");
    assert_eq!(results[0].1, true);
    assert!(results[0].2.is_none());

    // Check nonexistent.txt failed
    assert_eq!(results[1].0, "nonexistent.txt");
    assert_eq!(results[1].1, false);
    assert!(results[1].2.is_some());

    // Check exists2.txt succeeded
    assert_eq!(results[2].0, "exists2.txt");
    assert_eq!(results[2].1, true);
    assert!(results[2].2.is_none());

    // Verify existing objects were deleted
    assert!(backend.get_object("test-bucket", "exists1.txt").is_err());
    assert!(backend.get_object("test-bucket", "exists2.txt").is_err());
}

#[test]
fn test_delete_objects_empty_list() {
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    // Delete empty list
    let results = backend.delete_objects("test-bucket", &[]).unwrap();

    assert_eq!(results.len(), 0);
}

#[test]
fn test_delete_objects_many() {
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    // Create 100 objects
    let mut keys = Vec::new();
    for i in 0..100 {
        let key = format!("file{}.txt", i);
        backend
            .put_object("test-bucket", &key, format!("data{}", i).as_bytes())
            .unwrap();
        keys.push(key);
    }

    // Bulk delete all 100
    let results = backend.delete_objects("test-bucket", &keys).unwrap();

    assert_eq!(results.len(), 100);
    for (i, result) in results.iter().enumerate() {
        assert_eq!(result.0, keys[i]);
        assert_eq!(result.1, true);
        assert!(result.2.is_none());
    }

    // Verify all are gone
    for key in &keys {
        assert!(backend.get_object("test-bucket", key).is_err());
    }
}

#[test]
fn test_copy_then_delete() {
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    // Create source
    let data = b"Test data for copy and delete".to_vec();
    backend
        .put_object("test-bucket", "original.txt", &data)
        .unwrap();

    // Copy to multiple destinations
    backend
        .copy_object("test-bucket", "original.txt", "test-bucket", "copy1.txt")
        .unwrap();
    backend
        .copy_object("test-bucket", "original.txt", "test-bucket", "copy2.txt")
        .unwrap();
    backend
        .copy_object("test-bucket", "original.txt", "test-bucket", "copy3.txt")
        .unwrap();

    // Verify all copies exist
    assert_eq!(
        backend.get_object("test-bucket", "copy1.txt").unwrap(),
        data
    );
    assert_eq!(
        backend.get_object("test-bucket", "copy2.txt").unwrap(),
        data
    );
    assert_eq!(
        backend.get_object("test-bucket", "copy3.txt").unwrap(),
        data
    );

    // Bulk delete all copies
    let keys = vec![
        "copy1.txt".to_string(),
        "copy2.txt".to_string(),
        "copy3.txt".to_string(),
    ];
    let results = backend.delete_objects("test-bucket", &keys).unwrap();

    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|(_, success, _)| *success));

    // Verify copies are gone but original remains
    assert!(backend.get_object("test-bucket", "copy1.txt").is_err());
    assert!(backend.get_object("test-bucket", "copy2.txt").is_err());
    assert!(backend.get_object("test-bucket", "copy3.txt").is_err());
    assert_eq!(
        backend.get_object("test-bucket", "original.txt").unwrap(),
        data
    );
}
