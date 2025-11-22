//! Integration tests for multipart upload functionality
//!
//! These tests verify end-to-end multipart upload behavior with real data,
//! ensuring data integrity across various scenarios.

use cartridge::Cartridge;
use cartridge_s3::CartridgeS3Backend;
use parking_lot::RwLock;
use std::sync::Arc;

/// Helper to create a backend for testing
fn create_backend() -> CartridgeS3Backend {
    let cartridge = Cartridge::new(10000);
    let cart_arc = Arc::new(RwLock::new(cartridge));
    CartridgeS3Backend::new(cart_arc)
}

/// Helper to create test data with a repeating pattern
fn create_test_data(size: usize, pattern: u8) -> Vec<u8> {
    vec![pattern; size]
}

/// Helper to create test data with distinct patterns per chunk
fn create_chunked_data(chunks: &[(usize, u8)]) -> Vec<u8> {
    let mut data = Vec::new();
    for (size, pattern) in chunks {
        data.extend_from_slice(&vec![*pattern; *size]);
    }
    data
}

#[test]
fn test_multipart_two_parts_small() {
    // Test with small parts to verify basic assembly logic
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    let manager = backend.multipart_manager();
    let upload_id = manager.create_upload("test-bucket".to_string(), "small-file".to_string());

    // Upload 2 parts with distinct patterns
    let part1_data = create_test_data(100, b'A');
    let part2_data = create_test_data(50, b'B');

    manager
        .upload_part(&upload_id, 1, part1_data.clone(), "etag1".to_string())
        .unwrap();
    manager
        .upload_part(&upload_id, 2, part2_data.clone(), "etag2".to_string())
        .unwrap();

    // Complete and verify
    let (bucket, key, assembled) = manager.complete_upload(&upload_id).unwrap();
    assert_eq!(bucket, "test-bucket");
    assert_eq!(key, "small-file");
    assert_eq!(assembled.len(), 150);

    // Verify data integrity
    assert_eq!(&assembled[0..100], part1_data.as_slice());
    assert_eq!(&assembled[100..150], part2_data.as_slice());
}

#[test]
fn test_multipart_three_parts_medium() {
    // Test with medium-sized parts
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    let manager = backend.multipart_manager();
    let upload_id = manager.create_upload("test-bucket".to_string(), "medium-file".to_string());

    // Upload 3 parts with distinct patterns
    let part1_data = create_test_data(1024, b'X');
    let part2_data = create_test_data(2048, b'Y');
    let part3_data = create_test_data(512, b'Z');

    manager
        .upload_part(&upload_id, 1, part1_data.clone(), "etag1".to_string())
        .unwrap();
    manager
        .upload_part(&upload_id, 2, part2_data.clone(), "etag2".to_string())
        .unwrap();
    manager
        .upload_part(&upload_id, 3, part3_data.clone(), "etag3".to_string())
        .unwrap();

    // Complete and verify
    let (_, _, assembled) = manager.complete_upload(&upload_id).unwrap();
    assert_eq!(assembled.len(), 3584);

    // Verify each part
    assert_eq!(&assembled[0..1024], part1_data.as_slice());
    assert_eq!(&assembled[1024..3072], part2_data.as_slice());
    assert_eq!(&assembled[3072..3584], part3_data.as_slice());
}

#[test]
fn test_multipart_large_parts() {
    // Test with larger parts (1MB each) to simulate real AWS CLI behavior
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    let manager = backend.multipart_manager();
    let upload_id = manager.create_upload("test-bucket".to_string(), "large-file".to_string());

    // Create 2 parts: 1MB + 512KB
    let mb = 1024 * 1024;
    let part1_data = create_test_data(mb, 0x11);
    let part2_data = create_test_data(mb / 2, 0x22);

    manager
        .upload_part(&upload_id, 1, part1_data.clone(), "etag1".to_string())
        .unwrap();
    manager
        .upload_part(&upload_id, 2, part2_data.clone(), "etag2".to_string())
        .unwrap();

    // Complete and verify
    let (_, _, assembled) = manager.complete_upload(&upload_id).unwrap();
    let expected_size = mb + (mb / 2);
    assert_eq!(assembled.len(), expected_size);

    // Verify no duplication by checking boundaries
    assert_eq!(assembled[0], 0x11);
    assert_eq!(assembled[mb - 1], 0x11);
    assert_eq!(assembled[mb], 0x22); // First byte of part 2
    assert_eq!(assembled[expected_size - 1], 0x22);

    // Verify complete integrity
    assert_eq!(&assembled[0..mb], part1_data.as_slice());
    assert_eq!(&assembled[mb..expected_size], part2_data.as_slice());
}

#[test]
fn test_multipart_part_replacement() {
    // Test that uploading the same part number twice replaces the old part
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    let manager = backend.multipart_manager();
    let upload_id = manager.create_upload("test-bucket".to_string(), "replaced-file".to_string());

    // Upload part 1 with pattern 'A'
    let part1_v1 = create_test_data(100, b'A');
    manager
        .upload_part(&upload_id, 1, part1_v1, "etag1-v1".to_string())
        .unwrap();

    // Upload part 1 AGAIN with pattern 'B' (should replace)
    let part1_v2 = create_test_data(100, b'B');
    manager
        .upload_part(&upload_id, 1, part1_v2.clone(), "etag1-v2".to_string())
        .unwrap();

    // Upload part 2
    let part2 = create_test_data(50, b'C');
    manager
        .upload_part(&upload_id, 2, part2.clone(), "etag2".to_string())
        .unwrap();

    // Complete and verify - should use part1_v2, not part1_v1
    let (_, _, assembled) = manager.complete_upload(&upload_id).unwrap();
    assert_eq!(assembled.len(), 150);

    // Verify part 1 was replaced (should be all 'B', not 'A')
    assert!(assembled[0..100].iter().all(|&b| b == b'B'));
    assert!(assembled[100..150].iter().all(|&b| b == b'C'));
}

#[test]
fn test_multipart_missing_part() {
    // Test that assembly fails if parts are not sequential
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    let manager = backend.multipart_manager();
    let upload_id = manager.create_upload("test-bucket".to_string(), "incomplete-file".to_string());

    // Upload parts 1 and 3, skipping 2
    manager
        .upload_part(&upload_id, 1, vec![1; 100], "etag1".to_string())
        .unwrap();
    manager
        .upload_part(&upload_id, 3, vec![3; 100], "etag3".to_string())
        .unwrap();

    // Complete should fail due to missing part 2
    let result = manager.complete_upload(&upload_id);
    assert!(result.is_none(), "Assembly should fail with missing part");
}

#[test]
fn test_multipart_abort() {
    // Test that aborting an upload removes all state
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    let manager = backend.multipart_manager();
    let upload_id = manager.create_upload("test-bucket".to_string(), "aborted-file".to_string());

    // Upload some parts
    manager
        .upload_part(&upload_id, 1, vec![1; 100], "etag1".to_string())
        .unwrap();
    manager
        .upload_part(&upload_id, 2, vec![2; 100], "etag2".to_string())
        .unwrap();

    // Abort the upload
    assert!(manager.abort_upload(&upload_id));

    // Verify upload is gone
    assert!(manager.get_upload(&upload_id).is_none());
    assert!(manager.complete_upload(&upload_id).is_none());
}

#[test]
fn test_multipart_list_parts() {
    // Test listing parts of an in-progress upload
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    let manager = backend.multipart_manager();
    let upload_id = manager.create_upload("test-bucket".to_string(), "list-parts-file".to_string());

    // Upload 3 parts out of order
    manager
        .upload_part(&upload_id, 2, vec![2; 200], "etag2".to_string())
        .unwrap();
    manager
        .upload_part(&upload_id, 1, vec![1; 100], "etag1".to_string())
        .unwrap();
    manager
        .upload_part(&upload_id, 3, vec![3; 300], "etag3".to_string())
        .unwrap();

    // List parts - should be sorted by part number
    let parts = manager.list_parts(&upload_id).unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].part_number, 1);
    assert_eq!(parts[0].data.len(), 100);
    assert_eq!(parts[1].part_number, 2);
    assert_eq!(parts[1].data.len(), 200);
    assert_eq!(parts[2].part_number, 3);
    assert_eq!(parts[2].data.len(), 300);
}

#[test]
fn test_multipart_end_to_end_with_backend() {
    // Full end-to-end test: upload parts, complete, then retrieve via backend
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    let manager = backend.multipart_manager();
    let upload_id = manager.create_upload("test-bucket".to_string(), "e2e-file".to_string());

    // Create distinct data for each part
    let part1 = create_test_data(1024, 0xAA);
    let part2 = create_test_data(2048, 0xBB);
    let part3 = create_test_data(512, 0xCC);

    // Upload parts
    manager
        .upload_part(&upload_id, 1, part1.clone(), "etag1".to_string())
        .unwrap();
    manager
        .upload_part(&upload_id, 2, part2.clone(), "etag2".to_string())
        .unwrap();
    manager
        .upload_part(&upload_id, 3, part3.clone(), "etag3".to_string())
        .unwrap();

    // Complete the upload
    let (bucket, key, assembled) = manager.complete_upload(&upload_id).unwrap();

    // Store the assembled data via backend
    backend.put_object(&bucket, &key, &assembled).unwrap();

    // Retrieve the object and verify
    let retrieved = backend.get_object("test-bucket", "e2e-file").unwrap();
    assert_eq!(retrieved.len(), 3584);
    assert_eq!(&retrieved[0..1024], part1.as_slice());
    assert_eq!(&retrieved[1024..3072], part2.as_slice());
    assert_eq!(&retrieved[3072..3584], part3.as_slice());
}

#[test]
fn test_multipart_simulated_aws_cli_10mb() {
    // Simulate AWS CLI uploading a 10MB file with 8MB threshold
    // This creates 2 parts: 8MB + 2MB
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    let manager = backend.multipart_manager();
    let upload_id = manager.create_upload("test-bucket".to_string(), "10mb-file".to_string());

    // AWS CLI default: 8MB parts
    let mb8 = 8 * 1024 * 1024;
    let mb2 = 2 * 1024 * 1024;

    let part1 = create_test_data(mb8, 0x01);
    let part2 = create_test_data(mb2, 0x02);

    manager
        .upload_part(&upload_id, 1, part1.clone(), "etag1".to_string())
        .unwrap();
    manager
        .upload_part(&upload_id, 2, part2.clone(), "etag2".to_string())
        .unwrap();

    // Complete and verify
    let (bucket, key, assembled) = manager.complete_upload(&upload_id).unwrap();

    // CRITICAL: Verify exact size (should be 10MB, NOT 18MB)
    let expected_size = mb8 + mb2;
    assert_eq!(
        assembled.len(),
        expected_size,
        "Assembled size should be {} bytes (10MB), not {} bytes",
        expected_size,
        assembled.len()
    );

    // Verify no duplication at boundaries
    assert_eq!(assembled[0], 0x01, "Start should be part 1");
    assert_eq!(assembled[mb8 - 1], 0x01, "End of part 1");
    assert_eq!(
        assembled[mb8], 0x02,
        "Start of part 2 should immediately follow part 1"
    );
    assert_eq!(assembled[expected_size - 1], 0x02, "End should be part 2");

    // Verify complete data integrity
    assert_eq!(&assembled[0..mb8], part1.as_slice(), "Part 1 data mismatch");
    assert_eq!(
        &assembled[mb8..expected_size],
        part2.as_slice(),
        "Part 2 data mismatch"
    );

    // Store and retrieve
    backend.put_object(&bucket, &key, &assembled).unwrap();
    let retrieved = backend.get_object(&bucket, &key).unwrap();

    assert_eq!(
        retrieved.len(),
        expected_size,
        "Retrieved size should be {} bytes (10MB), not {} bytes",
        expected_size,
        retrieved.len()
    );
    assert_eq!(
        retrieved, assembled,
        "Retrieved data should match assembled data"
    );
}

#[test]
fn test_multipart_varying_part_sizes() {
    // Test with intentionally varied part sizes to catch size-related bugs
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    let manager = backend.multipart_manager();
    let upload_id = manager.create_upload("test-bucket".to_string(), "varied-file".to_string());

    // Varying sizes: 100 bytes, 5MB, 1 byte, 2MB
    let sizes_and_patterns = vec![
        (100, 0x11),
        (5 * 1024 * 1024, 0x22),
        (1, 0x33),
        (2 * 1024 * 1024, 0x44),
    ];

    let mut expected_data = Vec::new();
    for (i, (size, pattern)) in sizes_and_patterns.iter().enumerate() {
        let part_data = create_test_data(*size, *pattern);
        expected_data.extend_from_slice(&part_data);
        manager
            .upload_part(
                &upload_id,
                (i + 1) as i32,
                part_data,
                format!("etag{}", i + 1),
            )
            .unwrap();
    }

    // Complete and verify
    let (_, _, assembled) = manager.complete_upload(&upload_id).unwrap();
    assert_eq!(assembled.len(), expected_data.len());
    assert_eq!(assembled, expected_data);
}

#[test]
fn test_multipart_single_part() {
    // Edge case: multipart upload with only 1 part
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    let manager = backend.multipart_manager();
    let upload_id = manager.create_upload("test-bucket".to_string(), "single-part".to_string());

    let part1 = create_test_data(1024, 0xFF);
    manager
        .upload_part(&upload_id, 1, part1.clone(), "etag1".to_string())
        .unwrap();

    let (_, _, assembled) = manager.complete_upload(&upload_id).unwrap();
    assert_eq!(assembled, part1);
}

#[test]
fn test_multipart_many_small_parts() {
    // Test with many small parts (e.g., 100 parts of 10 bytes each)
    let backend = create_backend();
    backend.create_bucket("test-bucket").unwrap();

    let manager = backend.multipart_manager();
    let upload_id = manager.create_upload("test-bucket".to_string(), "many-parts".to_string());

    let num_parts = 100;
    let part_size = 10;
    let mut expected_data = Vec::new();

    for i in 0..num_parts {
        let pattern = (i % 256) as u8;
        let part_data = create_test_data(part_size, pattern);
        expected_data.extend_from_slice(&part_data);
        manager
            .upload_part(&upload_id, i + 1, part_data, format!("etag{}", i + 1))
            .unwrap();
    }

    let (_, _, assembled) = manager.complete_upload(&upload_id).unwrap();
    assert_eq!(assembled.len(), num_parts as usize * part_size);
    assert_eq!(assembled, expected_data);
}
