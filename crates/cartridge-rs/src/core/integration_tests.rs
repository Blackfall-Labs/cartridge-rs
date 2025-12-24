//! Integration tests for Phase 7 advanced features
//!
//! Tests the interaction between:
//! - Snapshots
//! - Engram freezing
//! - Compression
//! - Encryption
//! - IAM policies

#[cfg(test)]
mod tests {
    use crate::core::cartridge::Cartridge;
    use crate::core::compression::{CompressionConfig, CompressionMethod};
    use crate::core::encryption::EncryptionConfig;
    use crate::core::engram_integration::EngramFreezer;
    use crate::core::header::Header;
    use crate::core::iam::{Action, Effect, Policy, Statement};
    use crate::core::snapshot::SnapshotManager;
    use engram_rs::ArchiveReader;
    use tempfile::TempDir;

    #[test]
    fn test_snapshot_and_restore_workflow() {
        let temp_dir = TempDir::new().unwrap();
        let snapshot_dir = temp_dir.path().join("snapshots");

        // Create cartridge with data
        let mut cart = Cartridge::new(1000);
        cart.create_file("/data.txt", b"Version 1").unwrap();
        cart.create_file("/config.json", b"{\"version\": 1}")
            .unwrap();

        // Create snapshot manager and take snapshot
        let mut snap_mgr = SnapshotManager::new(&snapshot_dir).unwrap();
        let pages: std::collections::HashMap<u64, Vec<u8>> = std::collections::HashMap::new();
        let snap_id = snap_mgr
            .create_snapshot(
                "v1".to_string(),
                "Initial version".to_string(),
                temp_dir.path().to_path_buf(),
                Header::new(),
                &pages,
            )
            .unwrap();

        // Modify cartridge
        cart.write_file("/data.txt", b"Version 2").unwrap();

        // Verify snapshot exists
        assert!(snap_mgr.get_snapshot(snap_id).is_some());
        assert_eq!(snap_mgr.list_snapshots().len(), 1);

        // Cleanup
        snap_mgr.delete_snapshot(snap_id).unwrap();
        assert_eq!(snap_mgr.list_snapshots().len(), 0);
    }

    #[test]
    fn test_engram_with_compression() {
        let temp_dir = TempDir::new().unwrap();
        let engram_path = temp_dir.path().join("compressed.eng");

        // Create cartridge with compressible data
        let mut cart = Cartridge::new(1000);
        cart.create_file("/repeating.txt", &b"AAAA".repeat(500))
            .unwrap();
        cart.create_file("/random.bin", &vec![0x42; 1000]).unwrap();

        // Freeze with LZ4 compression
        let freezer = EngramFreezer::new(
            "test".to_string(),
            "1.0".to_string(),
            "Test".to_string(),
            None,
            engram_rs::CompressionMethod::Lz4,
        );

        freezer.freeze(&mut cart, &engram_path).unwrap();

        // Verify engram was created and is compressed
        assert!(engram_path.exists());
        let metadata = std::fs::metadata(&engram_path).unwrap();
        // Should be smaller than uncompressed (2000 bytes + overhead)
        assert!(metadata.len() < 2500);

        // Verify we can read it back
        let mut reader = ArchiveReader::open(&engram_path).unwrap();
        let repeating = reader.read_file("repeating.txt").unwrap();
        assert_eq!(repeating.len(), 2000); // 500 * 4
    }

    #[test]
    fn test_engram_with_iam_and_compression() {
        let temp_dir = TempDir::new().unwrap();
        let engram_path = temp_dir.path().join("secured.eng");

        // Create cartridge and files FIRST (before IAM policy)
        let mut cart = Cartridge::new(1000);
        cart.create_file("/public/readme.md", b"# Public README")
            .unwrap();
        cart.create_file("/private/secrets.txt", b"Secret data")
            .unwrap();

        // Now add IAM policy (after files exist)
        let policy = Policy {
            version: "2012-10-17".to_string(),
            statement: vec![
                Statement::new(
                    Effect::Allow,
                    vec![Action::Read],
                    vec!["public/**".to_string()],
                ),
                Statement::new(
                    Effect::Allow,
                    vec![Action::Read, Action::Write],
                    vec!["private/**".to_string()],
                ),
            ],
        };
        cart.set_policy(policy);

        // Freeze with Zstd compression
        let freezer = EngramFreezer::new(
            "secured-cart".to_string(),
            "1.0".to_string(),
            "Security Team".to_string(),
            Some("Cartridge with IAM policies".to_string()),
            engram_rs::CompressionMethod::Zstd,
        );

        freezer.freeze(&mut cart, &engram_path).unwrap();

        // Verify engram and manifest
        let mut reader = ArchiveReader::open(&engram_path).unwrap();
        let manifest = reader.read_manifest().unwrap().expect("Manifest exists");

        // Check IAM capabilities in manifest
        let capabilities = manifest["capabilities"].as_array().unwrap();
        assert!(!capabilities.is_empty());

        // Verify IAM policy file
        let policy_json = reader.read_file("iam_policy.json").unwrap();
        let restored_policy: Policy = serde_json::from_slice(&policy_json).unwrap();
        assert_eq!(restored_policy.statement.len(), 2);
    }

    #[test]
    fn test_compression_utilities() {
        use crate::compression::{compress, compress_if_beneficial, decompress};

        let data = b"Test data for compression ".repeat(50);

        // Test LZ4 compression
        let compressed_lz4 = compress(&data, CompressionMethod::Lz4).unwrap();
        let decompressed_lz4 = decompress(&compressed_lz4, CompressionMethod::Lz4).unwrap();
        assert_eq!(decompressed_lz4, data);
        assert!(compressed_lz4.len() < data.len());

        // Test Zstd compression
        let compressed_zstd = compress(&data, CompressionMethod::Zstd).unwrap();
        let decompressed_zstd = decompress(&compressed_zstd, CompressionMethod::Zstd).unwrap();
        assert_eq!(decompressed_zstd, data);
        assert!(compressed_zstd.len() < data.len());

        // Test beneficial compression detection
        let config = CompressionConfig::lz4();
        let (result, method) = compress_if_beneficial(&data, &config).unwrap();
        assert_eq!(method, CompressionMethod::Lz4);
        assert!(result.len() < data.len());
    }

    #[test]
    fn test_encryption_utilities() {
        use crate::encryption::{decrypt, encrypt, EncryptionConfig};

        let key = EncryptionConfig::generate_key();
        let plaintext = b"Sensitive data that needs encryption";

        // Test encryption/decryption
        let ciphertext = encrypt(plaintext, &key).unwrap();
        let decrypted = decrypt(&ciphertext, &key).unwrap();

        assert_eq!(decrypted.as_slice(), plaintext);
        assert_ne!(&ciphertext[12..], plaintext.as_slice()); // After nonce, should differ
        assert_eq!(ciphertext.len(), plaintext.len() + 28); // Nonce + tag overhead
    }

    #[test]
    fn test_snapshot_pruning() {
        let temp_dir = TempDir::new().unwrap();
        let snapshot_dir = temp_dir.path().join("snapshots");

        let mut snap_mgr = SnapshotManager::new(&snapshot_dir).unwrap();
        let cart = Cartridge::new(100);
        let pages = std::collections::HashMap::new();

        // Create 5 snapshots
        for i in 0..5 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            snap_mgr
                .create_snapshot(
                    format!("v{}", i),
                    format!("Version {}", i),
                    temp_dir.path().to_path_buf(),
                    Header::new(),
                    &pages,
                )
                .unwrap();
        }

        assert_eq!(snap_mgr.list_snapshots().len(), 5);

        // Keep only 2 most recent
        let deleted = snap_mgr.prune_old_snapshots(2).unwrap();
        assert_eq!(deleted.len(), 3);
        assert_eq!(snap_mgr.list_snapshots().len(), 2);
    }

    #[test]
    fn test_engram_file_listing() {
        let temp_dir = TempDir::new().unwrap();
        let engram_path = temp_dir.path().join("files.eng");

        // Create cartridge with nested structure
        let mut cart = Cartridge::new(1000);
        cart.create_file("/root.txt", b"root").unwrap();
        cart.create_dir("/dir1").unwrap();
        cart.create_file("/dir1/file1.txt", b"file1").unwrap();
        cart.create_dir("/dir1/dir2").unwrap();
        cart.create_file("/dir1/dir2/file2.txt", b"file2").unwrap();

        // List all files
        let files = cart.list_all_files().unwrap();
        assert_eq!(files.len(), 3);
        assert!(files.contains(&"/root.txt".to_string()));
        assert!(files.contains(&"/dir1/file1.txt".to_string()));
        assert!(files.contains(&"/dir1/dir2/file2.txt".to_string()));

        // Freeze and verify structure preserved
        let freezer =
            EngramFreezer::new_default("nested".to_string(), "1.0".to_string(), "Test".to_string());

        freezer.freeze(&mut cart, &engram_path).unwrap();

        let mut reader = ArchiveReader::open(&engram_path).unwrap();
        assert_eq!(reader.read_file("root.txt").unwrap(), b"root");
        assert_eq!(reader.read_file("dir1/file1.txt").unwrap(), b"file1");
        assert_eq!(reader.read_file("dir1/dir2/file2.txt").unwrap(), b"file2");
    }

    #[test]
    fn test_encryption_and_compression_combination() {
        use crate::compression::{compress, decompress};
        use crate::encryption::{decrypt, encrypt};

        let plaintext = b"Data to compress then encrypt ".repeat(20);
        let key = EncryptionConfig::generate_key();

        // Compress first, then encrypt
        let compressed = compress(&plaintext, CompressionMethod::Lz4).unwrap();
        let encrypted = encrypt(&compressed, &key).unwrap();

        // Decrypt then decompress
        let decrypted = decrypt(&encrypted, &key).unwrap();
        let decompressed = decompress(&decrypted, CompressionMethod::Lz4).unwrap();

        assert_eq!(decompressed, plaintext);

        // Verify size progression
        assert!(compressed.len() < plaintext.len());
        assert!(encrypted.len() > compressed.len()); // Encryption adds overhead
    }

    #[test]
    fn test_multiple_snapshots_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let snapshot_dir = temp_dir.path().join("snapshots");

        let mut snap_mgr = SnapshotManager::new(&snapshot_dir).unwrap();
        let cart = Cartridge::new(100);
        let mut pages = std::collections::HashMap::new();
        pages.insert(0, vec![1, 2, 3]);

        let snap_id = snap_mgr
            .create_snapshot(
                "test".to_string(),
                "Test snapshot".to_string(),
                temp_dir.path().to_path_buf(),
                Header::new(),
                &pages,
            )
            .unwrap();

        let metadata = snap_mgr.get_snapshot(snap_id).unwrap();
        assert_eq!(metadata.name, "test");
        assert_eq!(metadata.description, "Test snapshot");
        assert_eq!(metadata.size_bytes, 3);
    }

    #[test]
    fn test_engram_manifest_structure() {
        let temp_dir = TempDir::new().unwrap();
        let engram_path = temp_dir.path().join("manifest_test.eng");

        let mut cart = Cartridge::new(500);
        cart.create_file("/test.txt", b"test").unwrap();

        let freezer = EngramFreezer::new(
            "manifest-test".to_string(),
            "2.0.0".to_string(),
            "Integration Test".to_string(),
            Some("Testing manifest structure".to_string()),
            engram_rs::CompressionMethod::None,
        );

        freezer.freeze(&mut cart, &engram_path).unwrap();

        let mut reader = ArchiveReader::open(&engram_path).unwrap();
        let manifest = reader.read_manifest().unwrap().expect("Manifest exists");

        // Verify manifest fields
        assert_eq!(manifest["version"], "2.0.0");
        assert_eq!(manifest["author"], "Integration Test");
        assert_eq!(manifest["description"], "Testing manifest structure");
        assert_eq!(manifest["immutable"], true);
        assert_eq!(manifest["type"], "cartridge");
        assert!(manifest.get("created").is_some());
        assert!(manifest.get("id").is_some());
    }
}
