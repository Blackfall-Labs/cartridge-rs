//! File metadata structures

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// File type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileType {
    /// Regular file
    File,
    /// Directory
    Directory,
    /// Symbolic link
    Symlink,
}

/// File metadata stored in the catalog
///
/// Contains all information about a file except its content:
/// - Size, timestamps, permissions
/// - Block allocation (where the content lives)
/// - File type
/// - S3-compatible metadata (content_type, user_metadata)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    /// File type
    pub file_type: FileType,

    /// File size in bytes
    pub size: u64,

    /// Block IDs where content is stored
    pub blocks: Vec<u64>,

    /// Creation timestamp (Unix epoch seconds)
    pub created_at: u64,

    /// Last modified timestamp (Unix epoch seconds)
    pub modified_at: u64,

    /// Unix permissions (e.g., 0o755)
    pub permissions: u32,

    /// File owner (for future IAM integration)
    pub owner: String,

    /// Content hash (SHA-256) for integrity verification
    pub content_hash: Option<[u8; 32]>,

    /// MIME content type (for S3 compatibility)
    /// Examples: "text/plain", "application/json", "image/png"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,

    /// User-defined metadata key-value pairs (for S3 compatibility)
    /// Maps to S3 x-amz-meta-* headers
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub user_metadata: HashMap<String, String>,
}

impl FileMetadata {
    /// Create new file metadata
    pub fn new(file_type: FileType, size: u64, blocks: Vec<u64>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        FileMetadata {
            file_type,
            size,
            blocks,
            created_at: now,
            modified_at: now,
            permissions: match file_type {
                FileType::Directory => 0o755,
                _ => 0o644,
            },
            owner: String::from("default"),
            content_hash: None,
            content_type: None,
            user_metadata: HashMap::new(),
        }
    }

    /// Create a directory metadata entry
    pub fn directory() -> Self {
        Self::new(FileType::Directory, 0, Vec::new())
    }

    /// Update the modification timestamp
    pub fn touch(&mut self) {
        self.modified_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }

    /// Check if this is a directory
    pub fn is_directory(&self) -> bool {
        self.file_type == FileType::Directory
    }

    /// Check if this is a regular file
    pub fn is_file(&self) -> bool {
        self.file_type == FileType::File
    }

    /// Set content type (for S3 compatibility)
    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = Some(content_type.into());
        self
    }

    /// Add user metadata (for S3 compatibility)
    pub fn with_user_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.user_metadata.insert(key.into(), value.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_creation() {
        let meta = FileMetadata::new(FileType::File, 1024, vec![0, 1, 2]);
        assert_eq!(meta.file_type, FileType::File);
        assert_eq!(meta.size, 1024);
        assert_eq!(meta.blocks, vec![0, 1, 2]);
        assert_eq!(meta.permissions, 0o644);
        assert!(meta.content_type.is_none());
        assert!(meta.user_metadata.is_empty());
    }

    #[test]
    fn test_directory_metadata() {
        let dir = FileMetadata::directory();
        assert!(dir.is_directory());
        assert!(!dir.is_file());
        assert_eq!(dir.permissions, 0o755);
        assert_eq!(dir.size, 0);
    }

    #[test]
    fn test_touch() {
        let mut meta = FileMetadata::new(FileType::File, 0, Vec::new());
        let original_modified = meta.modified_at;

        std::thread::sleep(std::time::Duration::from_secs(1));
        meta.touch();

        assert!(meta.modified_at >= original_modified);
    }

    #[test]
    fn test_serialization() {
        let meta = FileMetadata::new(FileType::File, 2048, vec![10, 20, 30]);
        let json = serde_json::to_string(&meta).unwrap();
        let deserialized: FileMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.size, 2048);
        assert_eq!(deserialized.blocks, vec![10, 20, 30]);
    }

    #[test]
    fn test_s3_metadata_fields() {
        let meta = FileMetadata::new(FileType::File, 1024, vec![0, 1])
            .with_content_type("application/json")
            .with_user_metadata("author", "Alice")
            .with_user_metadata("version", "1.0");

        assert_eq!(meta.content_type, Some("application/json".to_string()));
        assert_eq!(meta.user_metadata.get("author"), Some(&"Alice".to_string()));
        assert_eq!(meta.user_metadata.get("version"), Some(&"1.0".to_string()));
    }

    #[test]
    fn test_s3_metadata_serialization() {
        let meta = FileMetadata::new(FileType::File, 512, vec![5])
            .with_content_type("text/plain")
            .with_user_metadata("key1", "value1");

        let json = serde_json::to_string(&meta).unwrap();
        let deserialized: FileMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.content_type, Some("text/plain".to_string()));
        assert_eq!(
            deserialized.user_metadata.get("key1"),
            Some(&"value1".to_string())
        );
    }
}
