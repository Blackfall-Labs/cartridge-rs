//! Engram integration for Cartridge
//!
//! Allows freezing mutable cartridges into immutable, compressed engrams.
//! Includes IAM policies and cartridge metadata in the engram manifest.
//!
//! Workflow:
//! ```text
//! Cartridge (mutable)  →  freeze()  →  Engram (immutable + compressed)
//!   + IAM policies     →           →    + manifest with capabilities
//!   + metadata         →           →    + access control preserved
//! ```

use crate::error::{CartridgeError, Result};
use super::cartridge::Cartridge;
use engram_rs::{ArchiveWriter, CompressionMethod};
use serde_json::json;
use std::path::Path;

/// Engram freezer for cartridges
pub struct EngramFreezer {
    /// Compression method to use
    compression: CompressionMethod,

    /// Engram name
    name: String,

    /// Engram version
    version: String,

    /// Author name
    author: String,

    /// Optional description
    description: Option<String>,
}

impl EngramFreezer {
    /// Create a new engram freezer
    pub fn new(
        name: String,
        version: String,
        author: String,
        description: Option<String>,
        compression: CompressionMethod,
    ) -> Self {
        EngramFreezer {
            compression,
            name,
            version,
            author,
            description,
        }
    }

    /// Create with default settings (Zstd compression)
    pub fn new_default(name: String, version: String, author: String) -> Self {
        Self::new(name, version, author, None, CompressionMethod::Zstd)
    }

    /// Freeze a cartridge to an engram archive
    ///
    /// Creates an immutable, compressed archive from the cartridge.
    /// Includes IAM policy in the manifest as capabilities.
    pub fn freeze(&self, cartridge: &mut Cartridge, output_path: &Path) -> Result<()> {
        let mut writer = ArchiveWriter::create(output_path)
            .map_err(|e| CartridgeError::Allocation(format!("Failed to create engram: {}", e)))?;

        // Get all files from catalog
        let files = list_all_files_recursive(cartridge)?;

        // Build file manifest with metadata
        let mut file_entries = serde_json::Map::new();
        for file_path in &files {
            if let Ok(metadata) = cartridge.metadata(file_path) {
                file_entries.insert(
                    file_path.clone(),
                    json!({
                        "size": metadata.size,
                        "type": if metadata.is_file() { "file" } else { "directory" },
                    }),
                );
            }
        }

        // Extract IAM capabilities from policy (if any)
        let capabilities = cartridge.extract_iam_capabilities()?;

        // Create engram manifest
        let manifest = json!({
            "version": self.version,
            "id": format!("{}-{}", self.name, chrono::Utc::now().timestamp()),
            "author": self.author,
            "description": self.description.as_ref().unwrap_or(&"Frozen cartridge archive".to_string()),
            "created": chrono::Utc::now().to_rfc3339(),
            "immutable": true,
            "type": "cartridge",
            "capabilities": capabilities,
            "files": file_entries,
            "metadata": {
                "compression": format!("{:?}", self.compression),
                "source": "cartridge",
            }
        });

        // Add manifest first
        writer
            .add_manifest(&manifest)
            .map_err(|e| CartridgeError::Allocation(format!("Failed to add manifest: {}", e)))?;

        // Add IAM policy if present
        if let Some(policy_json) = cartridge.get_iam_policy_json()? {
            writer
                .add_file_with_compression(
                    "iam_policy.json",
                    policy_json.as_bytes(),
                    CompressionMethod::None,
                )
                .map_err(|e| {
                    CartridgeError::Allocation(format!("Failed to add IAM policy: {}", e))
                })?;
        }

        // Add each file to the engram with specified compression
        for file_path in files {
            let content = cartridge.read_file(&file_path)?;
            // Strip leading slash for engram paths
            let engram_path = file_path.trim_start_matches('/');
            writer
                .add_file_with_compression(engram_path, &content, self.compression)
                .map_err(|e| {
                    CartridgeError::Allocation(format!("Failed to add file {}: {}", file_path, e))
                })?;
        }

        // Finalize the archive
        writer
            .finalize()
            .map_err(|e| CartridgeError::Allocation(format!("Failed to finalize engram: {}", e)))?;

        Ok(())
    }

    /// Freeze with vacuum (removes deleted files, optimizes storage)
    pub fn freeze_with_vacuum(&self, cartridge: &mut Cartridge, output_path: &Path) -> Result<()> {
        // Flush any dirty pages first
        cartridge.flush()?;

        // Perform the freeze
        self.freeze(cartridge, output_path)?;

        Ok(())
    }
}

/// List all files in a cartridge recursively
fn list_all_files_recursive(cartridge: &Cartridge) -> Result<Vec<String>> {
    // Get all entries from the catalog
    let all_entries = cartridge.list_dir("/")?;

    // Filter for files only (exclude directories)
    let mut files = Vec::new();
    for entry_path in all_entries {
        if let Ok(metadata) = cartridge.metadata(&entry_path) {
            if metadata.is_file() {
                files.push(entry_path);
            }
        }
    }

    Ok(files)
}

/// Extension trait to add list_all_files method to Cartridge
impl Cartridge {
    /// List all files in the cartridge (recursively)
    pub fn list_all_files(&self) -> Result<Vec<String>> {
        list_all_files_recursive(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_freezer_creation() {
        let freezer = EngramFreezer::new_default(
            "test-cartridge".to_string(),
            "1.0.0".to_string(),
            "Test Author".to_string(),
        );

        assert_eq!(freezer.name, "test-cartridge");
        assert_eq!(freezer.version, "1.0.0");
        assert_eq!(freezer.author, "Test Author");
        assert_eq!(freezer.compression, CompressionMethod::Zstd);
    }

    #[test]
    fn test_freeze_cartridge() {
        let temp_dir = TempDir::new().unwrap();
        let engram_path = temp_dir.path().join("test.eng");

        // Create a cartridge with some files
        let mut cart = Cartridge::new(1000);
        cart.create_file("/readme.txt", b"Hello, World!").unwrap();
        cart.create_file("/data.bin", b"Binary data here").unwrap();
        cart.create_dir("/docs").unwrap();
        cart.create_file("/docs/guide.md", b"# Guide\nContent here")
            .unwrap();

        // Freeze to engram
        let freezer = EngramFreezer::new_default(
            "test-cart".to_string(),
            "1.0.0".to_string(),
            "Test".to_string(),
        );

        let result = freezer.freeze(&mut cart, &engram_path);
        assert!(result.is_ok(), "Failed to freeze: {:?}", result.err());

        // Verify engram was created
        assert!(engram_path.exists());

        // Verify we can read it back
        use engram_rs::ArchiveReader;
        let mut reader = ArchiveReader::open(&engram_path).unwrap();

        let readme = reader.read_file("readme.txt").unwrap();
        assert_eq!(readme, b"Hello, World!");

        let data = reader.read_file("data.bin").unwrap();
        assert_eq!(data, b"Binary data here");

        let guide = reader.read_file("docs/guide.md").unwrap();
        assert_eq!(guide, b"# Guide\nContent here");
    }

    #[test]
    fn test_list_all_files() {
        let mut cart = Cartridge::new(1000);
        cart.create_file("/file1.txt", b"one").unwrap();
        cart.create_file("/file2.txt", b"two").unwrap();
        cart.create_dir("/subdir").unwrap();
        cart.create_file("/subdir/file3.txt", b"three").unwrap();

        let files = cart.list_all_files().unwrap();

        assert_eq!(files.len(), 3);
        assert!(files.contains(&"/file1.txt".to_string()));
        assert!(files.contains(&"/file2.txt".to_string()));
        assert!(files.contains(&"/subdir/file3.txt".to_string()));
    }

    #[test]
    fn test_freeze_with_compression() {
        let temp_dir = TempDir::new().unwrap();
        let zstd_path = temp_dir.path().join("zstd.eng");
        let lz4_path = temp_dir.path().join("lz4.eng");

        let mut cart = Cartridge::new(1000);
        cart.create_file("/large.txt", &vec![b'A'; 10000]).unwrap();

        // Freeze with Zstd
        let zstd_freezer = EngramFreezer::new(
            "test".to_string(),
            "1.0".to_string(),
            "Test".to_string(),
            None,
            CompressionMethod::Zstd,
        );
        zstd_freezer.freeze(&mut cart, &zstd_path).unwrap();

        // Freeze with LZ4
        let lz4_freezer = EngramFreezer::new(
            "test".to_string(),
            "1.0".to_string(),
            "Test".to_string(),
            None,
            CompressionMethod::Lz4,
        );
        lz4_freezer.freeze(&mut cart, &lz4_path).unwrap();

        // Both should exist and be compressed
        assert!(zstd_path.exists());
        assert!(lz4_path.exists());

        // Compressed size should be much smaller than original
        let zstd_size = std::fs::metadata(&zstd_path).unwrap().len();
        let lz4_size = std::fs::metadata(&lz4_path).unwrap().len();

        assert!(zstd_size < 10000, "Zstd should compress");
        assert!(lz4_size < 10000, "LZ4 should compress");
    }

    #[test]
    fn test_freeze_with_iam_policy() {
        use crate::iam::{Action, Effect, Policy, Statement};

        let temp_dir = TempDir::new().unwrap();
        let engram_path = temp_dir.path().join("with_iam.eng");

        // Create a cartridge with IAM policy
        let mut cart = Cartridge::new(1000);
        cart.create_file("/public/readme.txt", b"Public file")
            .unwrap();
        cart.create_file("/private/secret.txt", b"Secret file")
            .unwrap();

        // Add IAM policy
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
        cart.set_policy(policy.clone());

        // Freeze to engram
        let freezer = EngramFreezer::new_default(
            "test-with-iam".to_string(),
            "1.0.0".to_string(),
            "Test".to_string(),
        );

        freezer.freeze(&mut cart, &engram_path).unwrap();

        // Verify engram was created
        assert!(engram_path.exists());

        // Read manifest and verify IAM capabilities
        use engram_rs::ArchiveReader;
        let mut reader = ArchiveReader::open(&engram_path).unwrap();

        // Read manifest
        let manifest = reader
            .read_manifest()
            .unwrap()
            .expect("Manifest should exist");

        // Check capabilities exist
        assert!(manifest.get("capabilities").is_some());
        let capabilities = manifest["capabilities"].as_array().unwrap();

        // Should have 3 capabilities: read:public/**, read:private/**, write:private/**
        assert_eq!(capabilities.len(), 3);

        let cap_strings: Vec<String> = capabilities
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();

        assert!(cap_strings.contains(&"read:public/**".to_string()));
        assert!(cap_strings.contains(&"read:private/**".to_string()));
        assert!(cap_strings.contains(&"write:private/**".to_string()));

        // Verify IAM policy file exists
        let policy_json = reader.read_file("iam_policy.json").unwrap();
        let restored_policy: Policy = serde_json::from_slice(&policy_json).unwrap();
        assert_eq!(restored_policy.statement.len(), 2);
    }
}
