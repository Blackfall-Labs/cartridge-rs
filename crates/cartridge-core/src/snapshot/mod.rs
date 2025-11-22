//! Snapshot system for point-in-time cartridge copies
//!
//! Provides copy-on-write (COW) snapshots for:
//! - Backup and versioning
//! - Rollback to previous states
//! - Concurrent read access to stable versions
//!
//! Snapshots are lightweight and share unchanged pages with the parent.

use crate::error::{CartridgeError, Result};
use crate::header::Header;
use crate::page::Page;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Snapshot metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    /// Snapshot ID (timestamp-based)
    pub id: u64,

    /// Human-readable name
    pub name: String,

    /// Description
    pub description: String,

    /// Creation timestamp (Unix microseconds)
    pub created_at: u64,

    /// Parent cartridge path
    pub parent_path: PathBuf,

    /// Snapshot header (at time of snapshot)
    pub header: Header,

    /// Modified pages since snapshot (for COW)
    pub modified_pages: HashSet<u64>,

    /// Snapshot size in bytes
    pub size_bytes: u64,
}

impl SnapshotMetadata {
    /// Create new snapshot metadata
    pub fn new(name: String, description: String, parent_path: PathBuf, header: Header) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        SnapshotMetadata {
            id: now,
            name,
            description,
            created_at: now,
            parent_path,
            header,
            modified_pages: HashSet::new(),
            size_bytes: 0,
        }
    }

    /// Get snapshot age in seconds
    pub fn age_seconds(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        (now - self.created_at) / 1_000_000
    }
}

/// Snapshot manager
pub struct SnapshotManager {
    /// Active snapshots by ID
    snapshots: HashMap<u64, SnapshotMetadata>,

    /// Snapshot data directory
    snapshot_dir: PathBuf,
}

impl SnapshotManager {
    /// Create a new snapshot manager
    pub fn new<P: AsRef<Path>>(snapshot_dir: P) -> Result<Self> {
        let snapshot_dir = snapshot_dir.as_ref().to_path_buf();

        // Create snapshot directory if it doesn't exist
        if !snapshot_dir.exists() {
            std::fs::create_dir_all(&snapshot_dir).map_err(|e| {
                CartridgeError::Allocation(format!("Failed to create snapshot dir: {}", e))
            })?;
        }

        Ok(SnapshotManager {
            snapshots: HashMap::new(),
            snapshot_dir,
        })
    }

    /// Create a new snapshot
    pub fn create_snapshot(
        &mut self,
        name: String,
        description: String,
        parent_path: PathBuf,
        header: Header,
        pages: &HashMap<u64, Vec<u8>>,
    ) -> Result<u64> {
        let mut metadata = SnapshotMetadata::new(name, description, parent_path, header);

        // Calculate snapshot size
        let mut total_size = 0;
        for page_data in pages.values() {
            total_size += page_data.len() as u64;
        }
        metadata.size_bytes = total_size;

        // Write snapshot data to disk
        self.write_snapshot(&metadata, pages)?;

        let snapshot_id = metadata.id;
        self.snapshots.insert(snapshot_id, metadata);

        Ok(snapshot_id)
    }

    /// Write snapshot to disk
    fn write_snapshot(
        &self,
        metadata: &SnapshotMetadata,
        pages: &HashMap<u64, Vec<u8>>,
    ) -> Result<()> {
        // Create snapshot directory
        let snapshot_path = self.snapshot_dir.join(format!("snapshot_{}", metadata.id));
        std::fs::create_dir_all(&snapshot_path).map_err(|e| {
            CartridgeError::Allocation(format!("Failed to create snapshot dir: {}", e))
        })?;

        // Write metadata
        let metadata_path = snapshot_path.join("metadata.json");
        let metadata_json = serde_json::to_string_pretty(metadata).map_err(|e| {
            CartridgeError::Allocation(format!("Failed to serialize metadata: {}", e))
        })?;
        std::fs::write(&metadata_path, metadata_json)
            .map_err(|e| CartridgeError::Allocation(format!("Failed to write metadata: {}", e)))?;

        // Write pages
        let pages_path = snapshot_path.join("pages.bin");
        let mut pages_data = Vec::new();

        // Write page count
        pages_data.extend_from_slice(&(pages.len() as u64).to_le_bytes());

        // Write each page
        for (&page_id, page_data) in pages.iter() {
            pages_data.extend_from_slice(&page_id.to_le_bytes());
            pages_data.extend_from_slice(&(page_data.len() as u64).to_le_bytes());
            pages_data.extend_from_slice(page_data);
        }

        std::fs::write(&pages_path, pages_data)
            .map_err(|e| CartridgeError::Allocation(format!("Failed to write pages: {}", e)))?;

        Ok(())
    }

    /// Load snapshot from disk
    pub fn load_snapshot(&mut self, snapshot_id: u64) -> Result<SnapshotMetadata> {
        let snapshot_path = self.snapshot_dir.join(format!("snapshot_{}", snapshot_id));

        // Read metadata
        let metadata_path = snapshot_path.join("metadata.json");
        let metadata_json = std::fs::read_to_string(&metadata_path)
            .map_err(|e| CartridgeError::Allocation(format!("Failed to read metadata: {}", e)))?;

        let metadata: SnapshotMetadata = serde_json::from_str(&metadata_json)
            .map_err(|e| CartridgeError::Allocation(format!("Failed to parse metadata: {}", e)))?;

        self.snapshots.insert(snapshot_id, metadata.clone());

        Ok(metadata)
    }

    /// List all snapshots
    pub fn list_snapshots(&self) -> Vec<&SnapshotMetadata> {
        let mut snapshots: Vec<_> = self.snapshots.values().collect();
        snapshots.sort_by_key(|s| s.created_at);
        snapshots
    }

    /// Get snapshot metadata
    pub fn get_snapshot(&self, snapshot_id: u64) -> Option<&SnapshotMetadata> {
        self.snapshots.get(&snapshot_id)
    }

    /// Delete snapshot
    pub fn delete_snapshot(&mut self, snapshot_id: u64) -> Result<()> {
        // Remove from memory
        self.snapshots.remove(&snapshot_id);

        // Remove from disk
        let snapshot_path = self.snapshot_dir.join(format!("snapshot_{}", snapshot_id));
        if snapshot_path.exists() {
            std::fs::remove_dir_all(&snapshot_path).map_err(|e| {
                CartridgeError::Allocation(format!("Failed to delete snapshot: {}", e))
            })?;
        }

        Ok(())
    }

    /// Restore snapshot (returns pages)
    pub fn restore_snapshot(&self, snapshot_id: u64) -> Result<HashMap<u64, Vec<u8>>> {
        let snapshot_path = self.snapshot_dir.join(format!("snapshot_{}", snapshot_id));
        let pages_path = snapshot_path.join("pages.bin");

        // Read pages
        let pages_data = std::fs::read(&pages_path)
            .map_err(|e| CartridgeError::Allocation(format!("Failed to read pages: {}", e)))?;

        let mut offset = 0;
        let mut pages = HashMap::new();

        // Read page count
        if pages_data.len() < 8 {
            return Err(CartridgeError::Allocation("Invalid pages data".to_string()));
        }
        let page_count = u64::from_le_bytes(pages_data[0..8].try_into().unwrap());
        offset += 8;

        // Read each page
        for _ in 0..page_count {
            if offset + 16 > pages_data.len() {
                return Err(CartridgeError::Allocation(
                    "Truncated pages data".to_string(),
                ));
            }

            let page_id = u64::from_le_bytes(pages_data[offset..offset + 8].try_into().unwrap());
            offset += 8;

            let page_len =
                u64::from_le_bytes(pages_data[offset..offset + 8].try_into().unwrap()) as usize;
            offset += 8;

            if offset + page_len > pages_data.len() {
                return Err(CartridgeError::Allocation(
                    "Truncated page data".to_string(),
                ));
            }

            let page_data = pages_data[offset..offset + page_len].to_vec();
            offset += page_len;

            pages.insert(page_id, page_data);
        }

        Ok(pages)
    }

    /// Get total snapshot storage size
    pub fn total_size_bytes(&self) -> u64 {
        self.snapshots.values().map(|s| s.size_bytes).sum()
    }

    /// Prune old snapshots (keep only N most recent)
    pub fn prune_old_snapshots(&mut self, keep_count: usize) -> Result<Vec<u64>> {
        let mut snapshots: Vec<_> = self.snapshots.values().cloned().collect();
        snapshots.sort_by_key(|s| std::cmp::Reverse(s.created_at));

        let mut deleted = Vec::new();

        for snapshot in snapshots.iter().skip(keep_count) {
            self.delete_snapshot(snapshot.id)?;
            deleted.push(snapshot.id);
        }

        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_snapshot_creation() {
        let temp_dir = TempDir::new().unwrap();
        let snapshot_dir = temp_dir.path().join("snapshots");

        let mut manager = SnapshotManager::new(&snapshot_dir).unwrap();

        let header = Header::new();
        let mut pages = HashMap::new();
        pages.insert(0, vec![1, 2, 3, 4]);
        pages.insert(1, vec![5, 6, 7, 8]);

        let snapshot_id = manager
            .create_snapshot(
                "test_snapshot".to_string(),
                "Test description".to_string(),
                PathBuf::from("/test/path"),
                header,
                &pages,
            )
            .unwrap();

        assert!(manager.get_snapshot(snapshot_id).is_some());
        assert_eq!(manager.list_snapshots().len(), 1);
    }

    #[test]
    fn test_snapshot_restore() {
        let temp_dir = TempDir::new().unwrap();
        let snapshot_dir = temp_dir.path().join("snapshots");

        let mut manager = SnapshotManager::new(&snapshot_dir).unwrap();

        let header = Header::new();
        let mut pages = HashMap::new();
        pages.insert(0, vec![1, 2, 3, 4]);
        pages.insert(1, vec![5, 6, 7, 8]);

        let snapshot_id = manager
            .create_snapshot(
                "test".to_string(),
                "desc".to_string(),
                PathBuf::from("/test"),
                header,
                &pages,
            )
            .unwrap();

        let restored_pages = manager.restore_snapshot(snapshot_id).unwrap();

        assert_eq!(restored_pages.len(), 2);
        assert_eq!(restored_pages.get(&0).unwrap(), &vec![1, 2, 3, 4]);
        assert_eq!(restored_pages.get(&1).unwrap(), &vec![5, 6, 7, 8]);
    }

    #[test]
    fn test_snapshot_deletion() {
        let temp_dir = TempDir::new().unwrap();
        let snapshot_dir = temp_dir.path().join("snapshots");

        let mut manager = SnapshotManager::new(&snapshot_dir).unwrap();

        let header = Header::new();
        let pages = HashMap::new();

        let snapshot_id = manager
            .create_snapshot(
                "test".to_string(),
                "desc".to_string(),
                PathBuf::from("/test"),
                header,
                &pages,
            )
            .unwrap();

        assert_eq!(manager.list_snapshots().len(), 1);

        manager.delete_snapshot(snapshot_id).unwrap();

        assert_eq!(manager.list_snapshots().len(), 0);
    }

    #[test]
    fn test_snapshot_pruning() {
        let temp_dir = TempDir::new().unwrap();
        let snapshot_dir = temp_dir.path().join("snapshots");

        let mut manager = SnapshotManager::new(&snapshot_dir).unwrap();

        let header = Header::new();
        let pages = HashMap::new();

        // Create 5 snapshots
        for i in 0..5 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            manager
                .create_snapshot(
                    format!("snapshot_{}", i),
                    format!("desc_{}", i),
                    PathBuf::from("/test"),
                    header.clone(),
                    &pages,
                )
                .unwrap();
        }

        assert_eq!(manager.list_snapshots().len(), 5);

        // Keep only 2 most recent
        let deleted = manager.prune_old_snapshots(2).unwrap();

        assert_eq!(deleted.len(), 3);
        assert_eq!(manager.list_snapshots().len(), 2);
    }

    #[test]
    fn test_snapshot_size_tracking() {
        let temp_dir = TempDir::new().unwrap();
        let snapshot_dir = temp_dir.path().join("snapshots");

        let mut manager = SnapshotManager::new(&snapshot_dir).unwrap();

        let header = Header::new();
        let mut pages = HashMap::new();
        pages.insert(0, vec![1; 1000]);
        pages.insert(1, vec![2; 2000]);

        manager
            .create_snapshot(
                "test".to_string(),
                "desc".to_string(),
                PathBuf::from("/test"),
                header,
                &pages,
            )
            .unwrap();

        assert_eq!(manager.total_size_bytes(), 3000);
    }
}
