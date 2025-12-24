//! Integration tests for S3 Feature Fuses (v0.2)
//!
//! Tests ACL, SSE, and versioning features with different fuse modes.

use cartridge::{S3AclMode, S3FeatureFuses, S3SseMode, S3VersioningMode};
use cartridge::Cartridge;
use cartridge_s3::{CartridgeS3Backend, S3Acl, S3Grant, S3Permission, SseHeaders};
use parking_lot::RwLock;
use std::sync::Arc;
use tempfile::TempDir;

/// Helper: Create a test cartridge with specific fuse settings
fn create_test_backend(fuses: S3FeatureFuses) -> (CartridgeS3Backend, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("test.cart");

    let mut cart = Cartridge::create_at(&cart_path, "test-cartridge", "Test Cartridge").unwrap();

    // Set S3 fuses in header
    cart.header_mut().set_s3_fuses(fuses);

    let cart_arc = Arc::new(RwLock::new(cart));
    let backend = CartridgeS3Backend::new(cart_arc);

    (backend, temp_dir)
}

#[test]
fn test_acl_ignore_mode() {
    let fuses = S3FeatureFuses {
        versioning_mode: S3VersioningMode::None,
        acl_mode: S3AclMode::Ignore,
        sse_mode: S3SseMode::Ignore,
    };

    let (backend, _temp) = create_test_backend(fuses);

    // Create bucket and object
    backend.create_bucket("test-bucket").unwrap();
    backend
        .put_object("test-bucket", "test.txt", b"test data")
        .unwrap();

    // Try to put ACL - should succeed but not store anything
    let acl = S3Acl {
        owner: Some("user1".to_string()),
        grants: vec![S3Grant {
            grantee: "user2".to_string(),
            permission: S3Permission::Read,
        }],
    };

    backend
        .put_object_acl("test-bucket", "test.txt", &acl)
        .unwrap();

    // Get ACL should return empty ACL in Ignore mode
    let retrieved_acl = backend.get_object_acl("test-bucket", "test.txt").unwrap();
    assert!(retrieved_acl.grants.is_empty());
}

#[test]
fn test_acl_record_mode() {
    let fuses = S3FeatureFuses {
        versioning_mode: S3VersioningMode::None,
        acl_mode: S3AclMode::Record,
        sse_mode: S3SseMode::Ignore,
    };

    let (backend, _temp) = create_test_backend(fuses);

    // Create bucket and object
    backend.create_bucket("test-bucket").unwrap();
    backend
        .put_object("test-bucket", "test.txt", b"test data")
        .unwrap();

    // Put ACL - should store in metadata
    let acl = S3Acl {
        owner: Some("user1".to_string()),
        grants: vec![S3Grant {
            grantee: "user2".to_string(),
            permission: S3Permission::Read,
        }],
    };

    backend
        .put_object_acl("test-bucket", "test.txt", &acl)
        .unwrap();

    // Get ACL should return stored ACL
    let retrieved_acl = backend.get_object_acl("test-bucket", "test.txt").unwrap();
    assert_eq!(retrieved_acl.owner, Some("user1".to_string()));
    assert_eq!(retrieved_acl.grants.len(), 1);
    assert_eq!(retrieved_acl.grants[0].grantee, "user2");
    assert_eq!(retrieved_acl.grants[0].permission, S3Permission::Read);
}

#[test]
fn test_acl_enforce_mode() {
    let fuses = S3FeatureFuses {
        versioning_mode: S3VersioningMode::None,
        acl_mode: S3AclMode::Enforce,
        sse_mode: S3SseMode::Ignore,
    };

    let (backend, _temp) = create_test_backend(fuses);

    // Create bucket and object
    backend.create_bucket("test-bucket").unwrap();
    backend
        .put_object("test-bucket", "test.txt", b"test data")
        .unwrap();

    // Put ACL with specific permissions
    let acl = S3Acl {
        owner: Some("owner".to_string()),
        grants: vec![
            S3Grant {
                grantee: "reader".to_string(),
                permission: S3Permission::Read,
            },
            S3Grant {
                grantee: "writer".to_string(),
                permission: S3Permission::Write,
            },
        ],
    };

    backend
        .put_object_acl("test-bucket", "test.txt", &acl)
        .unwrap();

    // Check permissions
    let can_read = backend
        .check_acl_permission("test-bucket", "test.txt", "reader", &S3Permission::Read)
        .unwrap();
    assert!(can_read);

    let can_write = backend
        .check_acl_permission("test-bucket", "test.txt", "writer", &S3Permission::Write)
        .unwrap();
    assert!(can_write);

    let cannot_write = backend
        .check_acl_permission("test-bucket", "test.txt", "reader", &S3Permission::Write)
        .unwrap();
    assert!(!cannot_write);
}

#[test]
fn test_sse_ignore_mode() {
    let fuses = S3FeatureFuses {
        versioning_mode: S3VersioningMode::None,
        acl_mode: S3AclMode::Ignore,
        sse_mode: S3SseMode::Ignore,
    };

    let (backend, _temp) = create_test_backend(fuses);

    // Create bucket
    backend.create_bucket("test-bucket").unwrap();

    // Put object with SSE headers - should succeed but not store
    let sse = SseHeaders {
        algorithm: Some("AES256".to_string()),
        customer_algorithm: None,
        customer_key_md5: None,
        kms_key_id: None,
    };

    backend
        .put_object_with_sse("test-bucket", "test.txt", b"test data", &sse)
        .unwrap();

    // Get object with SSE - should return None in Ignore mode
    let (data, retrieved_sse) = backend
        .get_object_with_sse("test-bucket", "test.txt")
        .unwrap();
    assert_eq!(data, b"test data");
    assert!(retrieved_sse.is_none());
}

#[test]
fn test_sse_record_mode() {
    let fuses = S3FeatureFuses {
        versioning_mode: S3VersioningMode::None,
        acl_mode: S3AclMode::Ignore,
        sse_mode: S3SseMode::Record,
    };

    let (backend, _temp) = create_test_backend(fuses);

    // Create bucket
    backend.create_bucket("test-bucket").unwrap();

    // Put object with SSE headers - should store but not return
    let sse = SseHeaders {
        algorithm: Some("AES256".to_string()),
        customer_algorithm: None,
        customer_key_md5: None,
        kms_key_id: None,
    };

    backend
        .put_object_with_sse("test-bucket", "test.txt", b"test data", &sse)
        .unwrap();

    // Get object with SSE - should return None in Record mode
    let (data, retrieved_sse) = backend
        .get_object_with_sse("test-bucket", "test.txt")
        .unwrap();
    assert_eq!(data, b"test data");
    assert!(retrieved_sse.is_none());

    // But metadata should be stored (verifiable via get_sse_headers in Transparent mode)
}

#[test]
fn test_sse_transparent_mode() {
    let fuses = S3FeatureFuses {
        versioning_mode: S3VersioningMode::None,
        acl_mode: S3AclMode::Ignore,
        sse_mode: S3SseMode::Transparent,
    };

    let (backend, _temp) = create_test_backend(fuses);

    // Create bucket
    backend.create_bucket("test-bucket").unwrap();

    // Put object with SSE headers - should store and return
    let sse = SseHeaders {
        algorithm: Some("AES256".to_string()),
        customer_algorithm: Some("AES256".to_string()),
        customer_key_md5: Some("abcdef123456".to_string()),
        kms_key_id: Some("arn:aws:kms:us-east-1:123456789012:key/12345678-1234-1234-1234-123456789012".to_string()),
    };

    backend
        .put_object_with_sse("test-bucket", "test.txt", b"test data", &sse)
        .unwrap();

    // Get object with SSE - should return SSE headers in Transparent mode
    let (data, retrieved_sse) = backend
        .get_object_with_sse("test-bucket", "test.txt")
        .unwrap();
    assert_eq!(data, b"test data");
    assert!(retrieved_sse.is_some());

    let retrieved_sse = retrieved_sse.unwrap();
    assert_eq!(retrieved_sse.algorithm, Some("AES256".to_string()));
    assert_eq!(
        retrieved_sse.customer_algorithm,
        Some("AES256".to_string())
    );
    assert_eq!(
        retrieved_sse.customer_key_md5,
        Some("abcdef123456".to_string())
    );
    assert!(retrieved_sse.kms_key_id.is_some());

    // Test get_sse_headers
    let headers = backend
        .get_sse_headers("test-bucket", "test.txt")
        .unwrap();
    assert!(headers.is_some());
    let headers = headers.unwrap();
    assert_eq!(headers.algorithm, Some("AES256".to_string()));
}

#[test]
fn test_combined_acl_and_sse() {
    let fuses = S3FeatureFuses {
        versioning_mode: S3VersioningMode::None,
        acl_mode: S3AclMode::Record,
        sse_mode: S3SseMode::Transparent,
    };

    let (backend, _temp) = create_test_backend(fuses);

    // Create bucket and object
    backend.create_bucket("test-bucket").unwrap();

    // Put object with both ACL and SSE
    let sse = SseHeaders {
        algorithm: Some("AES256".to_string()),
        customer_algorithm: None,
        customer_key_md5: None,
        kms_key_id: None,
    };

    backend
        .put_object_with_sse("test-bucket", "test.txt", b"test data", &sse)
        .unwrap();

    let acl = S3Acl {
        owner: Some("user1".to_string()),
        grants: vec![S3Grant {
            grantee: "user2".to_string(),
            permission: S3Permission::Read,
        }],
    };

    backend
        .put_object_acl("test-bucket", "test.txt", &acl)
        .unwrap();

    // Verify both are stored and retrieved correctly
    let retrieved_acl = backend.get_object_acl("test-bucket", "test.txt").unwrap();
    assert_eq!(retrieved_acl.owner, Some("user1".to_string()));
    assert_eq!(retrieved_acl.grants.len(), 1);

    let (data, retrieved_sse) = backend
        .get_object_with_sse("test-bucket", "test.txt")
        .unwrap();
    assert_eq!(data, b"test data");
    assert!(retrieved_sse.is_some());
    assert_eq!(
        retrieved_sse.unwrap().algorithm,
        Some("AES256".to_string())
    );
}

#[test]
fn test_backward_compatibility() {
    // Create cartridge without setting fuses (all zeros = defaults)
    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("test.cart");

    let cart = Cartridge::create_at(&cart_path, "test-cartridge", "Test Cartridge").unwrap();
    // Don't set fuses - reserved field remains zeros

    let cart_arc = Arc::new(RwLock::new(cart));
    let backend = CartridgeS3Backend::new(cart_arc);

    // Should work with default permissive behavior
    backend.create_bucket("test-bucket").unwrap();
    backend
        .put_object("test-bucket", "test.txt", b"test data")
        .unwrap();

    let data = backend.get_object("test-bucket", "test.txt").unwrap();
    assert_eq!(data, b"test data");
}

#[test]
fn test_fuse_mode_combinations() {
    // Test multiple fuse combinations work correctly

    // Combination 1: Versioning enabled, ACL Record, SSE Ignore
    let fuses1 = S3FeatureFuses {
        versioning_mode: S3VersioningMode::SnapshotBacked,
        acl_mode: S3AclMode::Record,
        sse_mode: S3SseMode::Ignore,
    };
    let (backend1, _temp1) = create_test_backend(fuses1);
    backend1.create_bucket("bucket1").unwrap();
    backend1.put_object("bucket1", "file.txt", b"data").unwrap();

    // Combination 2: Versioning ignore, ACL Enforce, SSE Transparent
    let fuses2 = S3FeatureFuses {
        versioning_mode: S3VersioningMode::None,
        acl_mode: S3AclMode::Enforce,
        sse_mode: S3SseMode::Transparent,
    };
    let (backend2, _temp2) = create_test_backend(fuses2);
    backend2.create_bucket("bucket2").unwrap();
    backend2.put_object("bucket2", "file.txt", b"data").unwrap();

    // Both should work independently
    assert_eq!(backend1.get_object("bucket1", "file.txt").unwrap(), b"data");
    assert_eq!(backend2.get_object("bucket2", "file.txt").unwrap(), b"data");
}

#[test]
fn test_acl_multiple_permissions() {
    let fuses = S3FeatureFuses {
        versioning_mode: S3VersioningMode::None,
        acl_mode: S3AclMode::Enforce,
        sse_mode: S3SseMode::Ignore,
    };

    let (backend, _temp) = create_test_backend(fuses);

    backend.create_bucket("test-bucket").unwrap();
    backend
        .put_object("test-bucket", "test.txt", b"test data")
        .unwrap();

    // Put ACL with multiple grants
    let acl = S3Acl {
        owner: Some("owner".to_string()),
        grants: vec![
            S3Grant {
                grantee: "user1".to_string(),
                permission: S3Permission::Read,
            },
            S3Grant {
                grantee: "user1".to_string(),
                permission: S3Permission::Write,
            },
            S3Grant {
                grantee: "user2".to_string(),
                permission: S3Permission::Read,
            },
        ],
    };

    backend
        .put_object_acl("test-bucket", "test.txt", &acl)
        .unwrap();

    // user1 should have both Read and Write
    let can_read = backend
        .check_acl_permission("test-bucket", "test.txt", "user1", &S3Permission::Read)
        .unwrap();
    assert!(can_read);

    let can_write = backend
        .check_acl_permission("test-bucket", "test.txt", "user1", &S3Permission::Write)
        .unwrap();
    assert!(can_write);

    // user2 should only have Read
    let can_read = backend
        .check_acl_permission("test-bucket", "test.txt", "user2", &S3Permission::Read)
        .unwrap();
    assert!(can_read);

    let cannot_write = backend
        .check_acl_permission("test-bucket", "test.txt", "user2", &S3Permission::Write)
        .unwrap();
    assert!(!cannot_write);
}

#[test]
fn test_sse_empty_headers() {
    let fuses = S3FeatureFuses {
        versioning_mode: S3VersioningMode::None,
        acl_mode: S3AclMode::Ignore,
        sse_mode: S3SseMode::Transparent,
    };

    let (backend, _temp) = create_test_backend(fuses);

    backend.create_bucket("test-bucket").unwrap();

    // Put object with empty SSE headers
    let sse = SseHeaders {
        algorithm: None,
        customer_algorithm: None,
        customer_key_md5: None,
        kms_key_id: None,
    };

    backend
        .put_object_with_sse("test-bucket", "test.txt", b"test data", &sse)
        .unwrap();

    // Get object - should succeed but no SSE headers stored
    let (data, retrieved_sse) = backend
        .get_object_with_sse("test-bucket", "test.txt")
        .unwrap();
    assert_eq!(data, b"test data");
    assert!(retrieved_sse.is_none()); // Empty SSE not stored
}
