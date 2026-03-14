//! Catalog for file metadata
//!
//! Maps file paths to their metadata and block locations.
//! Uses a standard BTreeMap for ordered lookups, inserts, and prefix queries.
//! Serialized with bincode for compact binary storage.

pub mod btree;
pub mod metadata;

pub use metadata::{FileMetadata, FileType};

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Catalog for managing file metadata
///
/// Thin wrapper around BTreeMap<String, FileMetadata> that provides
/// the same interface as the old custom B+tree but backed by Rust's
/// battle-tested stdlib implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    /// Root page ID (kept for header compatibility)
    root_page: u64,

    /// The actual file index
    entries: BTreeMap<String, FileMetadata>,
}

impl Catalog {
    /// Create a new empty catalog
    pub fn new(root_page: u64) -> Self {
        Catalog {
            root_page,
            entries: BTreeMap::new(),
        }
    }

    /// Insert or update file metadata
    pub fn insert(&mut self, path: &str, metadata: FileMetadata) -> Result<()> {
        self.entries.insert(path.to_string(), metadata);
        Ok(())
    }

    /// Look up file metadata by path
    pub fn get(&self, path: &str) -> Result<Option<FileMetadata>> {
        Ok(self.entries.get(path).cloned())
    }

    /// Delete a file from the catalog
    pub fn delete(&mut self, path: &str) -> Result<Option<FileMetadata>> {
        Ok(self.entries.remove(path))
    }

    /// List all files with a given prefix (directory listing)
    pub fn list_prefix(&self, prefix: &str) -> Result<Vec<(String, FileMetadata)>> {
        Ok(self
            .entries
            .range(prefix.to_string()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    /// Get the root page ID
    pub fn root_page(&self) -> u64 {
        self.root_page
    }

    /// Serialize to bincode bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(|e| {
            crate::error::CartridgeError::Corruption(format!("catalog serialize: {e}"))
        })
    }

    /// Deserialize from bincode bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data).map_err(|e| {
            crate::error::CartridgeError::Corruption(format!("catalog deserialize: {e}"))
        })
    }

    /// Number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the catalog is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
