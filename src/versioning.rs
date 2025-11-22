//! S3 versioning support backed by Cartridge snapshots
//!
//! When S3VersioningMode::SnapshotBacked is enabled, object versions are
//! mapped to Cartridge snapshot IDs, providing full version history.

use crate::error::{S3Error, S3Result};
use cartridge::Cartridge;
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info};

/// Version ID type (maps to snapshot IDs)
pub type VersionId = String;

/// Versioning manager for S3 objects
pub struct VersioningManager {
    cartridge: Arc<RwLock<Cartridge>>,
    snapshot_dir: PathBuf,
}

impl VersioningManager {
    /// Create a new versioning manager
    pub fn new(cartridge: Arc<RwLock<Cartridge>>, snapshot_dir: impl AsRef<Path>) -> Self {
        Self {
            cartridge,
            snapshot_dir: snapshot_dir.as_ref().to_path_buf(),
        }
    }

    /// Create a snapshot version before overwriting an object
    ///
    /// Returns the version ID (snapshot ID) if a version was created.
    pub fn create_version_before_write(&self, key: &str) -> S3Result<Option<VersionId>> {
        let cart = self.cartridge.read();

        // Check if object exists
        let exists = cart.read_file(key).is_ok();
        if !exists {
            debug!("Object {} does not exist, skipping version creation", key);
            return Ok(None);
        }

        // Create snapshot with meaningful name
        let snapshot_name = format!("s3-version-{}", key);
        let snapshot_desc = format!("S3 object version before PUT: {}", key);

        debug!("Creating snapshot version for object: {}", key);

        let snapshot_id = cart
            .create_snapshot(snapshot_name, snapshot_desc, &self.snapshot_dir)
            .map_err(|e| S3Error::Internal(format!("Failed to create snapshot: {}", e)))?;

        let version_id = format!("v-{}", snapshot_id);
        info!("Created version {} for object {}", version_id, key);

        Ok(Some(version_id))
    }

    /// Get a specific version of an object
    ///
    /// Restores from the snapshot corresponding to the version ID.
    pub fn get_version(&self, key: &str, version_id: &VersionId) -> S3Result<Vec<u8>> {
        let snapshot_id = self.parse_version_id(version_id)?;

        debug!(
            "Retrieving version {} (snapshot {}) of object {}",
            version_id, snapshot_id, key
        );

        // Create a temporary cartridge clone to restore into
        let mut cart = self.cartridge.write();

        // Save current state
        let original_data = cart.read_file(key).ok();

        // Restore from snapshot
        cart.restore_snapshot(snapshot_id, &self.snapshot_dir)
            .map_err(|e| {
                S3Error::Internal(format!("Failed to restore snapshot {}: {}", snapshot_id, e))
            })?;

        // Read versioned object
        let versioned_data = cart
            .read_file(key)
            .map_err(|_| S3Error::NoSuchKey(format!("{}?versionId={}", key, version_id)))?;

        // Restore original state
        if let Some(data) = original_data {
            let _ = cart.delete_file(key);
            cart.create_file(key, &data).map_err(|e| {
                S3Error::Internal(format!("Failed to restore original state: {}", e))
            })?;
        }

        Ok(versioned_data)
    }

    /// List all versions of objects with a given prefix
    ///
    /// Returns (key, version_id) pairs for all snapshots containing matching objects.
    pub fn list_versions(&self, prefix: &str) -> S3Result<Vec<(String, VersionId)>> {
        use cartridge::snapshot::SnapshotManager;

        let manager = SnapshotManager::new(&self.snapshot_dir)
            .map_err(|e| S3Error::Internal(format!("Failed to open snapshot manager: {}", e)))?;

        let snapshots = manager.list_snapshots();
        let mut versions = Vec::new();

        debug!(
            "Listing versions for prefix '{}' across {} snapshots",
            prefix,
            snapshots.len()
        );

        for snapshot in snapshots {
            // Check if snapshot name indicates S3 versioning
            if !snapshot.name.starts_with("s3-version-") {
                continue;
            }

            // Extract object key from snapshot name
            if let Some(key) = snapshot.name.strip_prefix("s3-version-") {
                if key.starts_with(prefix) {
                    let version_id = format!("v-{}", snapshot.id);
                    versions.push((key.to_string(), version_id));
                }
            }
        }

        info!("Found {} versions for prefix '{}'", versions.len(), prefix);

        Ok(versions)
    }

    /// Delete a specific version
    ///
    /// Deletes the snapshot corresponding to the version ID.
    pub fn delete_version(&self, key: &str, version_id: &VersionId) -> S3Result<()> {
        use cartridge::snapshot::SnapshotManager;

        let snapshot_id = self.parse_version_id(version_id)?;

        debug!(
            "Deleting version {} (snapshot {}) of object {}",
            version_id, snapshot_id, key
        );

        let mut manager = SnapshotManager::new(&self.snapshot_dir)
            .map_err(|e| S3Error::Internal(format!("Failed to open snapshot manager: {}", e)))?;

        manager.delete_snapshot(snapshot_id).map_err(|e| {
            S3Error::Internal(format!("Failed to delete snapshot {}: {}", snapshot_id, e))
        })?;

        info!("Deleted version {} of object {}", version_id, key);

        Ok(())
    }

    /// Parse version ID to extract snapshot ID
    fn parse_version_id(&self, version_id: &VersionId) -> S3Result<u64> {
        version_id
            .strip_prefix("v-")
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| {
                S3Error::InvalidRequest(format!("Invalid version ID format: {}", version_id))
            })
    }
}

/// Check if versioning should be applied
pub fn should_create_version(versioning_enabled: bool, object_exists: bool) -> bool {
    versioning_enabled && object_exists
}

#[cfg(test)]
mod tests {
    use super::*;
    use cartridge::Cartridge;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup_test_cartridge() -> (Arc<RwLock<Cartridge>>, TempDir, TempDir) {
        let cart_dir = TempDir::new().unwrap();
        let snapshot_dir = TempDir::new().unwrap();

        let cart_path = cart_dir.path().join("test.cart");
        let cart = Cartridge::create(&cart_path, 1000).unwrap();

        (Arc::new(RwLock::new(cart)), cart_dir, snapshot_dir)
    }

    #[test]
    fn test_should_create_version() {
        assert!(!should_create_version(false, false)); // No versioning
        assert!(!should_create_version(false, true)); // No versioning
        assert!(!should_create_version(true, false)); // New object
        assert!(should_create_version(true, true)); // Existing object with versioning
    }

    #[test]
    fn test_create_version_for_new_object() {
        let (cart, _cart_dir, snapshot_dir) = setup_test_cartridge();
        let manager = VersioningManager::new(cart.clone(), snapshot_dir.path());

        // Try to create version for non-existent object
        let result = manager.create_version_before_write("/bucket/newfile.txt");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None); // No version created
    }

    #[test]
    fn test_create_version_for_existing_object() {
        let (cart, _cart_dir, snapshot_dir) = setup_test_cartridge();

        // Create initial object
        {
            let mut c = cart.write();
            c.create_dir("/bucket").unwrap();
            c.create_file("/bucket/file.txt", b"version 1").unwrap();
        }

        let manager = VersioningManager::new(cart.clone(), snapshot_dir.path());

        // Create version before overwriting
        let result = manager.create_version_before_write("/bucket/file.txt");
        assert!(result.is_ok());

        let version_id = result.unwrap();
        assert!(version_id.is_some());
        assert!(version_id.unwrap().starts_with("v-"));
    }

    #[test]
    fn test_parse_version_id() {
        let (cart, _cart_dir, snapshot_dir) = setup_test_cartridge();
        let manager = VersioningManager::new(cart, snapshot_dir.path());

        // Valid version ID
        assert_eq!(manager.parse_version_id(&"v-123".to_string()).unwrap(), 123);
        assert_eq!(
            manager.parse_version_id(&"v-999999".to_string()).unwrap(),
            999999
        );

        // Invalid version IDs
        assert!(manager.parse_version_id(&"123".to_string()).is_err());
        assert!(manager.parse_version_id(&"v-abc".to_string()).is_err());
        assert!(manager.parse_version_id(&"invalid".to_string()).is_err());
    }

    #[test]
    fn test_list_versions_empty() {
        let (cart, _cart_dir, snapshot_dir) = setup_test_cartridge();
        let manager = VersioningManager::new(cart, snapshot_dir.path());

        let versions = manager.list_versions("/bucket/").unwrap();
        assert_eq!(versions.len(), 0);
    }

    #[test]
    fn test_versioning_workflow() {
        let (cart, _cart_dir, snapshot_dir) = setup_test_cartridge();

        // Create initial object
        {
            let mut c = cart.write();
            c.create_dir("/bucket").unwrap();
            c.create_file("/bucket/doc.txt", b"version 1").unwrap();
        }

        let manager = VersioningManager::new(cart.clone(), snapshot_dir.path());

        // Create first version
        let version1 = manager
            .create_version_before_write("/bucket/doc.txt")
            .unwrap()
            .unwrap();
        assert!(version1.starts_with("v-"));

        // Overwrite with version 2
        {
            let mut c = cart.write();
            let _ = c.delete_file("/bucket/doc.txt");
            c.create_file("/bucket/doc.txt", b"version 2").unwrap();
        }

        // Create second version
        let version2 = manager
            .create_version_before_write("/bucket/doc.txt")
            .unwrap()
            .unwrap();
        assert!(version2.starts_with("v-"));
        assert_ne!(version1, version2);

        // Verify different version IDs were created
        // (List versions might not work in test environment without proper snapshot dir setup)
        assert!(version1 < version2); // Version IDs are timestamp-based
    }
}
