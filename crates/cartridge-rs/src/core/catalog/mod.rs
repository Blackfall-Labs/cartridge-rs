//! B-tree catalog for file metadata
//!
//! The catalog maps file paths to their metadata and block locations.
//! Uses a B-tree structure for efficient lookups, inserts, and range queries.

pub mod btree;
pub mod metadata;

pub use btree::{BTree, BTreeNode};
pub use metadata::{FileMetadata, FileType};

use crate::error::Result;

/// Catalog for managing file metadata
///
/// Provides a high-level interface for file operations:
/// - Insert/update file metadata
/// - Lookup files by path
/// - List files in a directory
/// - Delete files
pub struct Catalog {
    /// B-tree root node page ID
    root_page: u64,

    /// B-tree instance
    btree: BTree,
}

impl Catalog {
    /// Create a new catalog with the given root page
    pub fn new(root_page: u64) -> Self {
        Catalog {
            root_page,
            btree: BTree::new(root_page),
        }
    }

    /// Create a catalog from an existing B-tree
    pub fn from_btree(root_page: u64, btree: BTree) -> Self {
        Catalog { root_page, btree }
    }

    /// Get a reference to the internal B-tree
    pub fn btree(&self) -> &BTree {
        &self.btree
    }

    /// Get a mutable reference to the internal B-tree
    pub fn btree_mut(&mut self) -> &mut BTree {
        &mut self.btree
    }

    /// Insert or update file metadata
    pub fn insert(&mut self, path: &str, metadata: FileMetadata) -> Result<()> {
        self.btree.insert(path.to_string(), metadata)
    }

    /// Look up file metadata by path
    pub fn get(&self, path: &str) -> Result<Option<FileMetadata>> {
        self.btree.search(path)
    }

    /// Delete a file from the catalog
    pub fn delete(&mut self, path: &str) -> Result<Option<FileMetadata>> {
        self.btree.delete(path)
    }

    /// List all files with a given prefix (directory listing)
    pub fn list_prefix(&self, prefix: &str) -> Result<Vec<(String, FileMetadata)>> {
        self.btree.range_search(prefix)
    }

    /// Get the root page ID
    pub fn root_page(&self) -> u64 {
        self.root_page
    }
}
