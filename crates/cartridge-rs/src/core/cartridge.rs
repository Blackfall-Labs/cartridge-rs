//! Main Cartridge API
//!
//! Provides high-level file operations for the Cartridge archive format.

use crate::allocator::{hybrid::HybridAllocator, BlockAllocator};
use crate::audit::{AuditLogger, Operation};
use crate::catalog::{btree, Catalog, FileMetadata, FileType};
use crate::error::{CartridgeError, Result};
use crate::header::{Header, PAGE_SIZE};
use crate::iam::{Action, Policy, PolicyEngine};
use crate::io::CartridgeFile;
use crate::manifest::Manifest;
use crate::validation;
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;

// Auto-growth constants
const MIN_BLOCKS: usize = 3; // Minimum: header + catalog + data
const DEFAULT_INITIAL_BLOCKS: usize = 3; // Start minimal by default
const GROW_THRESHOLD: f64 = 0.10; // Grow when <10% free
const GROW_FACTOR: usize = 2; // Double size each time
const DEFAULT_MAX_BLOCKS: usize = 10_000_000; // ~40GB safety limit
const MANIFEST_PATH: &str = "/.cartridge/manifest.json";

/// Cartridge archive
///
/// High-level API for working with cartridge archives.
/// Combines allocation, catalog, and page management.
pub struct Cartridge {
    /// Archive header
    header: Header,

    /// Block allocator
    allocator: HybridAllocator,

    /// File catalog
    catalog: Catalog,

    /// Disk-backed storage (optional) - uses interior mutability for concurrent reads
    file: Option<Mutex<CartridgeFile>>,

    /// In-memory page cache (page_id -> page data) - uses interior mutability for concurrent reads
    pages: Arc<Mutex<std::collections::HashMap<u64, Vec<u8>>>>,

    /// Dirty pages that need to be flushed - uses interior mutability for concurrent reads
    dirty_pages: Arc<Mutex<std::collections::HashSet<u64>>>,

    /// Audit logger (optional)
    audit_logger: Option<Arc<AuditLogger>>,

    /// Session ID for audit logging
    session_id: u32,

    /// IAM policy for access control (optional)
    policy: Option<Policy>,

    /// IAM policy engine for evaluation - uses interior mutability for cache updates
    policy_engine: Option<Arc<Mutex<PolicyEngine>>>,

    /// Enable automatic growth (default: true)
    auto_grow: bool,

    /// Maximum blocks allowed (prevents runaway growth)
    max_blocks: usize,
}

impl Cartridge {
    /// Create a new in-memory cartridge
    pub fn new(total_blocks: usize) -> Self {
        let mut header = Header::new();
        header.total_blocks = total_blocks as u64;
        // Reserve pages 0, 1, 2 for header, catalog, allocator
        header.free_blocks = (total_blocks - 3) as u64;
        header.btree_root_page = 1; // Page 1 is B-tree root

        let mut allocator = HybridAllocator::new(total_blocks);
        // Mark pages 0, 1, 2 as allocated (reserved)
        allocator.allocate(3 * PAGE_SIZE as u64).unwrap();

        let catalog = Catalog::new(1);

        Cartridge {
            header,
            allocator,
            catalog,
            file: None,
            pages: Arc::new(Mutex::new(std::collections::HashMap::new())),
            dirty_pages: Arc::new(Mutex::new(std::collections::HashSet::new())),
            audit_logger: None,
            session_id: 0,
            policy: None,
            policy_engine: None,
            auto_grow: true,
            max_blocks: DEFAULT_MAX_BLOCKS,
        }
    }

    /// Create a new disk-backed cartridge with slug and title
    ///
    /// # Arguments
    ///
    /// * `slug` - Kebab-case identifier (used as filename, without .cart extension)
    /// * `title` - Human-readable display name
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cartridge_core::Cartridge;
    ///
    /// // Creates "my-container.cart" in current directory
    /// let cart = Cartridge::create("my-container", "My Container")?;
    /// ```
    pub fn create(
        slug: &str,
        title: &str,
    ) -> Result<Self> {
        // Validate slug and create path from it
        let slug_validated = validation::ContainerSlug::new(slug)?;
        let path = std::path::PathBuf::from(slug_validated.as_str());
        let normalized_path = validation::normalize_container_path(&path)?;

        // Create with minimal initial blocks (auto-growth enabled by default)
        let total_blocks = DEFAULT_INITIAL_BLOCKS;

        let mut header = Header::new();
        header.total_blocks = total_blocks as u64;
        // Reserve pages 0, 1, 2 for header, catalog, allocator
        header.free_blocks = (total_blocks - 3) as u64;
        header.btree_root_page = 1;

        let file = CartridgeFile::create(normalized_path, &header)?;

        let mut allocator = HybridAllocator::new(total_blocks);
        // Mark pages 0, 1, 2 as allocated (reserved)
        allocator.allocate(3 * PAGE_SIZE as u64)?;

        let catalog = Catalog::new(1);

        let mut cartridge = Cartridge {
            header,
            allocator,
            catalog,
            file: Some(Mutex::new(file)),
            pages: Arc::new(Mutex::new(std::collections::HashMap::new())),
            dirty_pages: Arc::new(Mutex::new(std::collections::HashSet::new())),
            audit_logger: None,
            session_id: 0,
            policy: None,
            policy_engine: None,
            auto_grow: true,
            max_blocks: DEFAULT_MAX_BLOCKS,
        };

        // Create manifest
        let manifest = Manifest::new(slug, title, semver::Version::new(0, 1, 0))?;
        let manifest_json = serde_json::to_vec_pretty(&manifest)?;

        // Ensure /.cartridge directory exists
        cartridge.create_dir("/.cartridge")?;

        // Write manifest to /.cartridge/manifest.json
        cartridge.create_file(MANIFEST_PATH, &manifest_json)?;

        Ok(cartridge)
    }

    /// Create a new disk-backed cartridge at a specific path
    ///
    /// Use this when you need to specify a custom directory or path.
    ///
    /// # Arguments
    ///
    /// * `path` - Full path where the cartridge will be created (without .cart extension)
    /// * `slug` - Kebab-case identifier for the container
    /// * `title` - Human-readable display name
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cartridge_core::Cartridge;
    ///
    /// // Creates "/data/my-container.cart"
    /// let cart = Cartridge::create_at("/data/my-container", "my-container", "My Container")?;
    /// ```
    pub fn create_at<P: AsRef<Path>>(
        path: P,
        slug: &str,
        title: &str,
    ) -> Result<Self> {
        // Validate slug
        let _slug_validated = validation::ContainerSlug::new(slug)?;
        let normalized_path = validation::normalize_container_path(path.as_ref())?;

        // Create with minimal initial blocks (auto-growth enabled by default)
        let total_blocks = DEFAULT_INITIAL_BLOCKS;

        let mut header = Header::new();
        header.total_blocks = total_blocks as u64;
        // Reserve pages 0, 1, 2 for header, catalog, allocator
        header.free_blocks = (total_blocks - 3) as u64;
        header.btree_root_page = 1;

        let file = CartridgeFile::create(normalized_path, &header)?;

        let mut allocator = HybridAllocator::new(total_blocks);
        // Mark pages 0, 1, 2 as allocated (reserved)
        allocator.allocate(3 * PAGE_SIZE as u64)?;

        let catalog = Catalog::new(1);

        let mut cartridge = Cartridge {
            header,
            allocator,
            catalog,
            file: Some(Mutex::new(file)),
            pages: Arc::new(Mutex::new(std::collections::HashMap::new())),
            dirty_pages: Arc::new(Mutex::new(std::collections::HashSet::new())),
            audit_logger: None,
            session_id: 0,
            policy: None,
            policy_engine: None,
            auto_grow: true,
            max_blocks: DEFAULT_MAX_BLOCKS,
        };

        // Create manifest
        let manifest = Manifest::new(slug, title, semver::Version::new(0, 1, 0))?;
        let manifest_json = serde_json::to_vec_pretty(&manifest)?;

        // Ensure /.cartridge directory exists
        cartridge.create_dir("/.cartridge")?;

        // Write manifest to /.cartridge/manifest.json
        cartridge.create_file(MANIFEST_PATH, &manifest_json)?;

        Ok(cartridge)
    }

    /// Open an existing disk-backed cartridge
    ///
    /// Loads the manifest if present. For backwards compatibility,
    /// containers without manifests will open successfully with a warning.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        // Normalize path (handles .cart extension)
        let normalized_path = validation::normalize_container_path(path.as_ref())?;

        let mut file = CartridgeFile::open(normalized_path)?;
        let header = file.read_header()?;

        // Load catalog and allocator from disk
        let catalog = Self::load_catalog(&mut file, header.btree_root_page)?;
        let allocator = Self::load_allocator(&mut file, header.total_blocks as usize)?;

        let cartridge = Cartridge {
            header,
            allocator,
            catalog,
            file: Some(Mutex::new(file)),
            pages: Arc::new(Mutex::new(std::collections::HashMap::new())),
            dirty_pages: Arc::new(Mutex::new(std::collections::HashSet::new())),
            audit_logger: None,
            session_id: 0,
            policy: None,
            policy_engine: None,
            auto_grow: true,
            max_blocks: DEFAULT_MAX_BLOCKS,
        };

        // Try to load manifest (optional for backwards compatibility)
        if let Ok(exists) = cartridge.exists(MANIFEST_PATH) {
            if !exists {
                tracing::warn!("Container opened without manifest (legacy container)");
            }
        }

        Ok(cartridge)
    }

    /// Flush all dirty pages to disk
    pub fn flush(&mut self) -> Result<()> {
        if self.file.is_none() {
            return Ok(());
        }

        let mut file = self.file.as_ref().unwrap().lock();

        // Write header
        file.write_header(&self.header)?;

        // Serialize and write catalog state (uses page 1 for now)
        let catalog_data = serde_json::to_vec(self.catalog.btree())?;
        let mut page_data = vec![0u8; PAGE_SIZE];
        let len = catalog_data.len().min(PAGE_SIZE);
        page_data[..len].copy_from_slice(&catalog_data[..len]);
        file.write_page_data(1, &page_data)?;

        // Serialize and write allocator state (uses page 2 for now)
        let allocator_data = serde_json::to_vec(&self.allocator)?;
        let mut page_data = vec![0u8; PAGE_SIZE];
        let len = allocator_data.len().min(PAGE_SIZE);
        page_data[..len].copy_from_slice(&allocator_data[..len]);
        file.write_page_data(2, &page_data)?;

        // Write dirty pages
        let pages = self.pages.lock();
        let mut dirty_pages = self.dirty_pages.lock();
        for &page_id in dirty_pages.iter() {
            if let Some(data) = pages.get(&page_id) {
                file.write_page_data(page_id, data)?;
            }
        }

        dirty_pages.clear();
        file.sync()?;

        Ok(())
    }

    /// Load catalog state from disk
    fn load_catalog(file: &mut CartridgeFile, root_page: u64) -> Result<Catalog> {
        // Read from page 1
        let page_data = file.read_page_data(1)?;

        // Find the end of JSON data (first null byte or end of page)
        let end = page_data.iter().position(|&b| b == 0).unwrap_or(PAGE_SIZE);

        if end == 0 {
            // Empty catalog
            return Ok(Catalog::new(root_page));
        }

        // Deserialize B-tree
        let btree: btree::BTree = serde_json::from_slice(&page_data[..end])?;

        Ok(Catalog::from_btree(root_page, btree))
    }

    /// Load allocator state from disk
    fn load_allocator(file: &mut CartridgeFile, total_blocks: usize) -> Result<HybridAllocator> {
        // Read from page 2
        let page_data = file.read_page_data(2)?;

        // Find the end of JSON data
        let end = page_data.iter().position(|&b| b == 0).unwrap_or(PAGE_SIZE);

        if end == 0 {
            // Empty allocator
            return Ok(HybridAllocator::new(total_blocks));
        }

        // Deserialize allocator
        let allocator: HybridAllocator = serde_json::from_slice(&page_data[..end])?;

        Ok(allocator)
    }

    /// Close the cartridge, flushing all changes
    pub fn close(mut self) -> Result<()> {
        self.flush()
    }

    /// Enable audit logging with a shared logger
    pub fn set_audit_logger(&mut self, logger: Arc<AuditLogger>) {
        self.audit_logger = Some(logger);
    }

    /// Set session ID for audit logging
    pub fn set_session_id(&mut self, session_id: u32) {
        self.session_id = session_id;
    }

    /// Set IAM policy for access control
    pub fn set_policy(&mut self, policy: Policy) {
        self.policy = Some(policy);
        // Initialize policy engine if not already present
        if self.policy_engine.is_none() {
            self.policy_engine = Some(Arc::new(Mutex::new(PolicyEngine::new_default())));
        }
    }

    /// Check if an action on a resource is allowed by the policy
    ///
    /// Returns `Ok(())` if allowed, `Err` if denied or no policy is set.
    pub fn check_access(&self, action: &Action, path: &str) -> Result<()> {
        if let (Some(policy), Some(engine)) = (&self.policy, &self.policy_engine) {
            let mut engine = engine.lock();
            if engine.evaluate(policy, action, path, None) {
                Ok(())
            } else {
                Err(CartridgeError::Allocation(format!(
                    "Access denied: {:?} on {}",
                    action, path
                )))
            }
        } else {
            // No policy set - allow all operations (permissive by default)
            Ok(())
        }
    }

    /// Clear the IAM policy evaluation cache
    pub fn clear_policy_cache(&mut self) {
        if let Some(engine) = &self.policy_engine {
            engine.lock().clear_cache();
        }
    }

    /// Extract IAM capabilities from the policy for engram manifest
    pub fn extract_iam_capabilities(&self) -> Result<Vec<String>> {
        if let Some(policy) = &self.policy {
            let mut capabilities = Vec::new();

            // Convert policy statements to capabilities (only Allow statements)
            for statement in &policy.statement {
                if matches!(statement.effect, crate::iam::Effect::Allow) {
                    for action in &statement.action {
                        for resource in &statement.resource {
                            let capability = format!(
                                "{}:{}",
                                action_to_string_lower(action),
                                resource.trim_start_matches('/')
                            );
                            capabilities.push(capability);
                        }
                    }
                }
            }

            Ok(capabilities)
        } else {
            Ok(Vec::new())
        }
    }

    /// Get the IAM policy as JSON (if present)
    pub fn get_iam_policy_json(&self) -> Result<Option<String>> {
        if let Some(policy) = &self.policy {
            let json = policy.to_json().map_err(|e| {
                CartridgeError::Allocation(format!("Failed to serialize policy: {}", e))
            })?;
            Ok(Some(json))
        } else {
            Ok(None)
        }
    }

    /// Create a snapshot of the current cartridge state
    ///
    /// Returns the snapshot ID
    pub fn create_snapshot(
        &self,
        name: String,
        description: String,
        snapshot_dir: &std::path::Path,
    ) -> Result<u64> {
        use crate::snapshot::SnapshotManager;

        let mut manager = SnapshotManager::new(snapshot_dir)?;

        // Get cartridge path (if disk-backed)
        let parent_path = self
            .file
            .as_ref()
            .map(|f| f.lock().path().to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("memory"));

        let pages = self.pages.lock();
        let snapshot_id = manager.create_snapshot(
            name,
            description,
            parent_path,
            self.header.clone(),
            &*pages,
        )?;

        Ok(snapshot_id)
    }

    /// Restore from a snapshot
    ///
    /// Replaces current pages with snapshot data
    pub fn restore_snapshot(
        &mut self,
        snapshot_id: u64,
        snapshot_dir: &std::path::Path,
    ) -> Result<()> {
        use crate::snapshot::SnapshotManager;

        let manager = SnapshotManager::new(snapshot_dir)?;

        // Load snapshot metadata
        let metadata = manager.get_snapshot(snapshot_id).ok_or_else(|| {
            CartridgeError::Allocation(format!("Snapshot not found: {}", snapshot_id))
        })?;

        // Restore pages
        let restored_pages = manager.restore_snapshot(snapshot_id)?;

        // Replace current state
        *self.pages.lock() = restored_pages;
        self.header = metadata.header.clone();

        let mut dirty_pages = self.dirty_pages.lock();
        dirty_pages.clear();

        // Mark all pages as dirty for next flush
        let pages = self.pages.lock();
        for &page_id in pages.keys() {
            dirty_pages.insert(page_id);
        }

        Ok(())
    }

    /// Log an audit event (internal helper)
    fn audit_log(&self, operation: Operation, path: &str) {
        if let Some(logger) = &self.audit_logger {
            // Use simple hash of path as file_id for auditing
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            path.hash(&mut hasher);
            let file_id = hasher.finish();
            logger.log_file_op(1, operation, file_id, self.session_id);
        }
    }

    /// Create a file with content
    pub fn create_file(&mut self, path: &str, content: &[u8]) -> Result<()> {
        // Check IAM policy
        self.check_access(&Action::Create, path)?;

        // Check if file already exists
        if self.catalog.get(path)?.is_some() {
            return Err(CartridgeError::Allocation(format!(
                "File already exists: {}",
                path
            )));
        }

        // Ensure capacity before allocating
        if !content.is_empty() {
            self.ensure_capacity(content.len())?;
        }

        // Allocate blocks for content
        let blocks = if content.is_empty() {
            Vec::new()
        } else {
            self.allocator.allocate(content.len() as u64)?
        };

        // Write content to pages
        self.write_content(&blocks, content)?;

        // Create metadata
        let metadata = FileMetadata::new(FileType::File, content.len() as u64, blocks);

        // Add to catalog
        self.catalog.insert(path, metadata)?;

        // Update header
        self.header.free_blocks = self.allocator.free_blocks() as u64;

        // Audit log
        self.audit_log(Operation::Create, path);

        Ok(())
    }

    /// Read a file's content
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        // Check IAM policy
        self.check_access(&Action::Read, path)?;

        // Audit log
        self.audit_log(Operation::Read, path);
        let metadata = self
            .catalog
            .get(path)?
            .ok_or_else(|| CartridgeError::Allocation(format!("File not found: {}", path)))?;

        if !metadata.is_file() {
            return Err(CartridgeError::Allocation(format!("Not a file: {}", path)));
        }

        // Read content from blocks
        self.read_content(&metadata.blocks, metadata.size as usize)
    }

    /// Write content to existing file (replace)
    pub fn write_file(&mut self, path: &str, content: &[u8]) -> Result<()> {
        // Check IAM policy
        self.check_access(&Action::Write, path)?;

        let mut metadata = self
            .catalog
            .get(path)?
            .ok_or_else(|| CartridgeError::Allocation(format!("File not found: {}", path)))?;

        if !metadata.is_file() {
            return Err(CartridgeError::Allocation(format!("Not a file: {}", path)));
        }

        // Ensure capacity before allocating
        if !content.is_empty() {
            self.ensure_capacity(content.len())?;
        }

        // Free old blocks
        if !metadata.blocks.is_empty() {
            self.allocator.free(&metadata.blocks)?;
        }

        // Allocate new blocks
        let new_blocks = if content.is_empty() {
            Vec::new()
        } else {
            self.allocator.allocate(content.len() as u64)?
        };

        // Write new content
        self.write_content(&new_blocks, content)?;

        // Update metadata
        metadata.size = content.len() as u64;
        metadata.blocks = new_blocks;
        metadata.touch();

        // Update catalog
        self.catalog.insert(path, metadata)?;

        // Update header
        self.header.free_blocks = self.allocator.free_blocks() as u64;

        // Audit log
        self.audit_log(Operation::Update, path);

        Ok(())
    }

    /// Append content to existing file
    pub fn append_file(&mut self, path: &str, content: &[u8]) -> Result<()> {
        // Audit log (append is an update operation)
        self.audit_log(Operation::Update, path);
        let mut existing = self.read_file(path)?;
        existing.extend_from_slice(content);
        self.write_file(path, &existing)
    }

    /// Delete a file
    pub fn delete_file(&mut self, path: &str) -> Result<()> {
        // Check IAM policy
        self.check_access(&Action::Delete, path)?;

        let metadata = self
            .catalog
            .delete(path)?
            .ok_or_else(|| CartridgeError::Allocation(format!("File not found: {}", path)))?;

        // Free blocks
        if !metadata.blocks.is_empty() {
            self.allocator.free(&metadata.blocks)?;
        }

        // Update header
        self.header.free_blocks = self.allocator.free_blocks() as u64;

        // Audit log
        self.audit_log(Operation::Delete, path);

        Ok(())
    }

    /// Create a directory
    pub fn create_dir(&mut self, path: &str) -> Result<()> {
        // Check if already exists
        if self.catalog.get(path)?.is_some() {
            return Err(CartridgeError::Allocation(format!(
                "Path already exists: {}",
                path
            )));
        }

        let metadata = FileMetadata::directory();
        self.catalog.insert(path, metadata)?;

        Ok(())
    }

    /// List directory contents
    pub fn list_dir(&self, path: &str) -> Result<Vec<String>> {
        let prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{}/", path)
        };

        let entries = self.catalog.list_prefix(&prefix)?;
        Ok(entries.into_iter().map(|(path, _)| path).collect())
    }

    /// Check if a path exists
    pub fn exists(&self, path: &str) -> Result<bool> {
        Ok(self.catalog.get(path)?.is_some())
    }

    /// Get file metadata
    pub fn metadata(&self, path: &str) -> Result<FileMetadata> {
        self.catalog
            .get(path)?
            .ok_or_else(|| CartridgeError::Allocation(format!("Path not found: {}", path)))
    }

    /// Get a reference to the cartridge header
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Get a mutable reference to the cartridge header
    pub fn header_mut(&mut self) -> &mut Header {
        &mut self.header
    }

    /// Update file user metadata
    ///
    /// Adds or updates a key-value pair in the file's user metadata.
    /// This is useful for storing S3-compatible metadata like ACLs or SSE headers.
    pub fn update_user_metadata(
        &mut self,
        path: &str,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<()> {
        let mut metadata = self.metadata(path)?;
        metadata.user_metadata.insert(key.into(), value.into());
        self.catalog.insert(path, metadata)?;
        Ok(())
    }

    /// Get archive statistics
    pub fn stats(&self) -> CartridgeStats {
        CartridgeStats {
            total_blocks: self.header.total_blocks,
            free_blocks: self.header.free_blocks,
            used_blocks: self.header.total_blocks - self.header.free_blocks,
            fragmentation: self.allocator.fragmentation_score(),
        }
    }

    /// Read container manifest
    ///
    /// Returns an error if the manifest doesn't exist or is invalid.
    pub fn read_manifest(&self) -> Result<Manifest> {
        let manifest_data = self.read_file(MANIFEST_PATH)?;
        let manifest: Manifest = serde_json::from_slice(&manifest_data)?;
        Ok(manifest)
    }

    /// Write/update container manifest
    ///
    /// Overwrites the existing manifest at /.cartridge/manifest.json
    pub fn write_manifest(&mut self, manifest: &Manifest) -> Result<()> {
        let manifest_json = serde_json::to_vec_pretty(manifest)?;

        // Check if manifest file exists
        if self.exists(MANIFEST_PATH)? {
            self.write_file(MANIFEST_PATH, &manifest_json)?;
        } else {
            // Ensure directory exists
            if !self.exists("/.cartridge")? {
                self.create_dir("/.cartridge")?;
            }
            self.create_file(MANIFEST_PATH, &manifest_json)?;
        }

        Ok(())
    }

    /// Get container slug from manifest
    ///
    /// Returns an error if manifest doesn't exist.
    pub fn slug(&self) -> Result<String> {
        let manifest = self.read_manifest()?;
        Ok(manifest.slug.into_string())
    }

    /// Get container title from manifest
    ///
    /// Returns an error if manifest doesn't exist.
    pub fn title(&self) -> Result<String> {
        let manifest = self.read_manifest()?;
        Ok(manifest.title)
    }

    /// Update manifest with a closure
    ///
    /// Loads the manifest, applies the closure, and writes it back.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use cartridge_core::Cartridge;
    /// # fn example(mut cart: Cartridge) -> Result<(), Box<dyn std::error::Error>> {
    /// cart.update_manifest(|manifest| {
    ///     manifest.description = Some("Updated description".to_string());
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn update_manifest<F>(&mut self, f: F) -> Result<()>
    where
        F: FnOnce(&mut Manifest),
    {
        let mut manifest = self.read_manifest()?;
        f(&mut manifest);
        self.write_manifest(&manifest)?;
        Ok(())
    }

    /// Ensure sufficient capacity, growing if needed
    ///
    /// This method is called before allocating space for file operations.
    /// If auto-growth is enabled and free space is insufficient,
    /// the container will automatically grow (potentially multiple times).
    fn ensure_capacity(&mut self, bytes_needed: usize) -> Result<()> {
        if !self.auto_grow {
            return Ok(()); // Manual management
        }

        let blocks_needed = (bytes_needed + PAGE_SIZE - 1) / PAGE_SIZE;

        // Keep growing until we have enough free space
        while (self.header.free_blocks as usize) < blocks_needed {
            self.grow()?;
        }

        Ok(())
    }

    /// Grow container capacity
    ///
    /// Doubles the container size (or grows to max_blocks limit).
    /// Updates header, extends file, and extends allocator capacity.
    fn grow(&mut self) -> Result<()> {
        let current = self.header.total_blocks as usize;
        let new_total = (current * GROW_FACTOR).min(self.max_blocks);

        if new_total == current {
            return Err(CartridgeError::OutOfSpace);
        }

        tracing::info!("Growing container: {} -> {} blocks", current, new_total);

        // Extend file (if disk-backed)
        if let Some(file) = &self.file {
            let mut f = file.lock();
            f.extend(new_total)?;
        }

        // Update header
        let added_blocks = new_total - current;
        self.header.total_blocks = new_total as u64;
        self.header.free_blocks += added_blocks as u64;

        // Extend allocator capacity
        self.allocator.extend_capacity(new_total)?;

        Ok(())
    }

    /// Write content to blocks
    fn write_content(&mut self, blocks: &[u64], content: &[u8]) -> Result<()> {
        let mut offset = 0;
        let mut pages = self.pages.lock();
        let mut dirty_pages = self.dirty_pages.lock();

        for &block_id in blocks {
            let chunk_size = (content.len() - offset).min(PAGE_SIZE);
            let chunk = &content[offset..offset + chunk_size];

            // Create page with content
            let mut page_data = vec![0u8; PAGE_SIZE];
            page_data[..chunk.len()].copy_from_slice(chunk);

            // Store in cache
            pages.insert(block_id, page_data);

            // Mark as dirty for later flush
            dirty_pages.insert(block_id);

            offset += chunk_size;
            if offset >= content.len() {
                break;
            }
        }

        Ok(())
    }

    /// Read content from blocks
    fn read_content(&self, blocks: &[u64], total_size: usize) -> Result<Vec<u8>> {
        let mut content = Vec::with_capacity(total_size);
        let mut remaining = total_size;
        let mut pages = self.pages.lock();

        for &block_id in blocks {
            // Try to get from cache first, otherwise load from disk
            let page_data = if let Some(data) = pages.get(&block_id) {
                data.clone()
            } else if let Some(ref file) = self.file {
                // Load from disk and cache it
                let data = file.lock().read_page_data(block_id)?;
                pages.insert(block_id, data.clone());
                data
            } else {
                return Err(CartridgeError::Allocation(format!(
                    "Block {} not found in memory and no disk backing",
                    block_id
                )));
            };

            let chunk_size = remaining.min(PAGE_SIZE);
            content.extend_from_slice(&page_data[..chunk_size]);

            remaining -= chunk_size;
            if remaining == 0 {
                break;
            }
        }

        Ok(content)
    }
}

/// Cartridge statistics
#[derive(Debug, Clone)]
pub struct CartridgeStats {
    pub total_blocks: u64,
    pub free_blocks: u64,
    pub used_blocks: u64,
    pub fragmentation: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_read_file() {
        let mut cart = Cartridge::new(1000);

        let content = b"Hello, Cartridge!";
        cart.create_file("test.txt", content).unwrap();

        let read = cart.read_file("test.txt").unwrap();
        assert_eq!(read, content);
    }

    #[test]
    fn test_write_file() {
        let mut cart = Cartridge::new(1000);

        cart.create_file("test.txt", b"original").unwrap();
        cart.write_file("test.txt", b"updated content").unwrap();

        let read = cart.read_file("test.txt").unwrap();
        assert_eq!(read, b"updated content");
    }

    #[test]
    fn test_append_file() {
        let mut cart = Cartridge::new(1000);

        cart.create_file("test.txt", b"Hello").unwrap();
        cart.append_file("test.txt", b", World!").unwrap();

        let read = cart.read_file("test.txt").unwrap();
        assert_eq!(read, b"Hello, World!");
    }

    #[test]
    fn test_delete_file() {
        let mut cart = Cartridge::new(1000);

        cart.create_file("test.txt", b"data").unwrap();
        assert!(cart.exists("test.txt").unwrap());

        cart.delete_file("test.txt").unwrap();
        assert!(!cart.exists("test.txt").unwrap());
    }

    #[test]
    fn test_create_directory() {
        let mut cart = Cartridge::new(1000);

        cart.create_dir("/home/user").unwrap();

        let meta = cart.metadata("/home/user").unwrap();
        assert!(meta.is_directory());
    }

    #[test]
    fn test_list_directory() {
        let mut cart = Cartridge::new(1000);

        cart.create_file("/home/file1.txt", b"1").unwrap();
        cart.create_file("/home/file2.txt", b"2").unwrap();
        cart.create_file("/other/file3.txt", b"3").unwrap();

        let home_files = cart.list_dir("/home").unwrap();
        assert_eq!(home_files.len(), 2);

        let other_files = cart.list_dir("/other").unwrap();
        assert_eq!(other_files.len(), 1);
    }

    #[test]
    fn test_large_file() {
        let mut cart = Cartridge::new(1000);

        // Create 100KB file (spans multiple blocks)
        let large_content = vec![42u8; 100 * 1024];
        cart.create_file("large.bin", &large_content).unwrap();

        let read = cart.read_file("large.bin").unwrap();
        assert_eq!(read.len(), 100 * 1024);
        assert_eq!(read, large_content);
    }

    #[test]
    fn test_stats() {
        let mut cart = Cartridge::new(1000);

        let stats = cart.stats();
        assert_eq!(stats.total_blocks, 1000);
        // 3 blocks reserved for header, catalog, allocator
        assert_eq!(stats.free_blocks, 997);
        assert_eq!(stats.used_blocks, 3);

        cart.create_file("test.txt", b"Hello").unwrap();

        let stats = cart.stats();
        assert!(stats.used_blocks > 3);
        assert!(stats.free_blocks < 997);
    }

    #[test]
    fn test_file_not_found() {
        let mut cart = Cartridge::new(1000);
        let result = cart.read_file("nonexistent.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_file() {
        let mut cart = Cartridge::new(1000);

        cart.create_file("test.txt", b"data").unwrap();
        let result = cart.create_file("test.txt", b"duplicate");
        assert!(result.is_err());
    }

    // Phase 5: Disk I/O Tests
    #[test]
    fn test_disk_backed_create_and_close() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test.cart");

        {
            let mut cart = Cartridge::create_at(&path, "test", "Test Container").unwrap();
            cart.create_file("test.txt", b"Hello, Disk!").unwrap();
            cart.close().unwrap();
        }

        // File should exist
        assert!(path.exists());

        // Try to reopen
        {
            let mut cart = Cartridge::open(&path).unwrap();
            let content = cart.read_file("test.txt").unwrap();
            assert_eq!(content, b"Hello, Disk!");
        }
    }

    #[test]
    fn test_disk_backed_round_trip() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test.cart");

        // Create and write
        {
            let mut cart = Cartridge::create_at(&path, "test", "Test Container").unwrap();
            cart.create_file("test.txt", b"Hello, World!").unwrap();
            cart.create_file("data.bin", &vec![42u8; 1024]).unwrap();
            cart.create_dir("/home/user").unwrap();
            cart.close().unwrap();
        }

        // Reopen and verify
        {
            let mut cart = Cartridge::open(&path).unwrap();

            let content = cart.read_file("test.txt").unwrap();
            assert_eq!(content, b"Hello, World!");

            let data = cart.read_file("data.bin").unwrap();
            assert_eq!(data.len(), 1024);
            assert!(data.iter().all(|&b| b == 42));

            assert!(cart.exists("/home/user").unwrap());
            let meta = cart.metadata("/home/user").unwrap();
            assert!(meta.is_directory());
        }
    }

    #[test]
    fn test_disk_backed_large_file() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("large.cart");

        let large_content = vec![123u8; 100 * 1024]; // 100KB

        // Create and write
        {
            let mut cart = Cartridge::create_at(&path, "test", "Test Container").unwrap();
            cart.create_file("large.bin", &large_content).unwrap();
            cart.close().unwrap();
        }

        // Reopen and verify
        {
            let mut cart = Cartridge::open(&path).unwrap();
            let read_content = cart.read_file("large.bin").unwrap();
            assert_eq!(read_content.len(), 100 * 1024);
            assert_eq!(read_content, large_content);
        }
    }

    #[test]
    fn test_disk_backed_write_and_reopen() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("write.cart");

        // Create with original content
        {
            let mut cart = Cartridge::create_at(&path, "test", "Test Container").unwrap();
            cart.create_file("test.txt", b"original").unwrap();
            cart.close().unwrap();
        }

        // Reopen, modify, close
        {
            let mut cart = Cartridge::open(&path).unwrap();
            cart.write_file("test.txt", b"modified content").unwrap();
            cart.close().unwrap();
        }

        // Verify modification persisted
        {
            let mut cart = Cartridge::open(&path).unwrap();
            let content = cart.read_file("test.txt").unwrap();
            assert_eq!(content, b"modified content");
        }
    }

    #[test]
    fn test_disk_backed_delete_and_reopen() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("delete.cart");

        // Create multiple files
        {
            let mut cart = Cartridge::create_at(&path, "test", "Test Container").unwrap();
            cart.create_file("file1.txt", b"data1").unwrap();
            cart.create_file("file2.txt", b"data2").unwrap();
            cart.create_file("file3.txt", b"data3").unwrap();
            cart.close().unwrap();
        }

        // Delete one file
        {
            let mut cart = Cartridge::open(&path).unwrap();
            cart.delete_file("file2.txt").unwrap();
            cart.close().unwrap();
        }

        // Verify deletion persisted
        {
            let mut cart = Cartridge::open(&path).unwrap();
            assert!(cart.exists("file1.txt").unwrap());
            assert!(!cart.exists("file2.txt").unwrap());
            assert!(cart.exists("file3.txt").unwrap());
        }
    }

    #[test]
    fn test_disk_backed_flush() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("flush.cart");

        let mut cart = Cartridge::create_at(&path, "test", "Test Container").unwrap();
        cart.create_file("test.txt", b"data").unwrap();

        // Explicit flush without closing
        cart.flush().unwrap();

        // Should be able to read back immediately
        let content = cart.read_file("test.txt").unwrap();
        assert_eq!(content, b"data");

        cart.close().unwrap();
    }

    #[test]
    fn test_disk_backed_stats_persistence() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("stats.cart");

        let initial_total;
        let initial_used;

        // Create with some files
        {
            let mut cart = Cartridge::create_at(&path, "test", "Test Container").unwrap();
            cart.create_file("test.txt", b"Hello").unwrap();
            let stats = cart.stats();
            initial_total = stats.total_blocks;
            initial_used = stats.used_blocks;
            assert!(initial_used > 0);
            cart.close().unwrap();
        }

        // Reopen and verify stats persist correctly
        {
            let cart = Cartridge::open(&path).unwrap();
            let stats = cart.stats();
            assert_eq!(stats.total_blocks, initial_total);
            assert_eq!(stats.used_blocks, initial_used);
            assert_eq!(stats.free_blocks + stats.used_blocks, initial_total);
        }
    }

    #[test]
    fn test_iam_policy_enforcement() {
        use crate::iam::{Effect, Statement};

        let mut cart = Cartridge::new(1000);

        // Create a policy
        let mut policy = Policy::new();

        // Allow read and create on /public/**
        policy.add_statement(Statement::new(
            Effect::Allow,
            vec![Action::Read, Action::List, Action::Create],
            vec!["/public/**".to_string()],
        ));

        // Allow write and create on /data/** (but not read)
        policy.add_statement(Statement::new(
            Effect::Allow,
            vec![Action::Write, Action::Create],
            vec!["/data/**".to_string()],
        ));

        // Deny all access to /secret/**
        policy.add_statement(Statement::new(
            Effect::Deny,
            vec![Action::All],
            vec!["/secret/**".to_string()],
        ));

        cart.set_policy(policy);

        // Test allowed operations
        cart.create_file("/data/file.txt", b"test").unwrap();
        cart.create_file("/public/readme.md", b"public").unwrap();
        cart.read_file("/public/readme.md").unwrap();

        // Test denied operations
        let result = cart.create_file("/secret/key.pem", b"secret");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Access denied"));

        // Test read denial on non-public paths
        cart.create_file("/data/private.txt", b"private").unwrap();
        let result = cart.read_file("/data/private.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_auto_growth() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("growth.cart");

        let mut cart = Cartridge::create_at(&path, "test-growth", "Test Growth").unwrap();

        // Container starts small (manifest directory/file may have already caused growth)
        let initial_stats = cart.stats();
        assert!(initial_stats.total_blocks >= 3); // At least 3 blocks

        // Add content that requires significant growth (100KB needs ~25 blocks)
        let large_data = vec![0u8; 100_000];
        cart.create_file("large.bin", &large_data).unwrap();

        // Should have grown automatically to accommodate the data
        let after_stats = cart.stats();
        assert!(after_stats.total_blocks >= 25); // At least enough for the data
        assert!(after_stats.total_blocks > initial_stats.total_blocks); // Grew from initial

        // Verify we can read the file back
        let read_data = cart.read_file("large.bin").unwrap();
        assert_eq!(read_data.len(), 100_000);

        cart.close().unwrap();
    }

    #[test]
    fn test_manifest_creation_and_read() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("manifest-test.cart");

        // Create with slug and title
        {
            let mut cart = Cartridge::create_at(&path, "us-const", "U.S. Constitution").unwrap();

            // Read manifest
            let manifest = cart.read_manifest().unwrap();
            assert_eq!(manifest.slug.as_str(), "us-const");
            assert_eq!(manifest.title, "U.S. Constitution");
            assert_eq!(manifest.version, semver::Version::new(0, 1, 0));

            // Test convenience methods
            assert_eq!(cart.slug().unwrap(), "us-const");
            assert_eq!(cart.title().unwrap(), "U.S. Constitution");

            cart.close().unwrap();
        }

        // Reopen and verify manifest persists
        {
            let cart = Cartridge::open(&path).unwrap();
            let manifest = cart.read_manifest().unwrap();
            assert_eq!(manifest.slug.as_str(), "us-const");
            assert_eq!(manifest.title, "U.S. Constitution");
        }
    }

    #[test]
    fn test_manifest_update() {
        let mut cart = Cartridge::new(100);

        // Manually create a manifest for in-memory cartridge
        let manifest = Manifest::new("test", "Test", semver::Version::new(1, 0, 0)).unwrap();
        cart.create_dir("/.cartridge").unwrap();
        cart.write_manifest(&manifest).unwrap();

        // Update using the closure API
        cart.update_manifest(|m| {
            m.description = Some("Updated description".to_string());
        })
        .unwrap();

        // Verify update persisted
        let updated = cart.read_manifest().unwrap();
        assert_eq!(
            updated.description,
            Some("Updated description".to_string())
        );
    }

    #[test]
    fn test_iam_cache_usage() {
        use crate::iam::{Effect, Statement};

        let mut cart = Cartridge::new(1000);

        let mut policy = Policy::new();
        policy.add_statement(Statement::new(
            Effect::Allow,
            vec![Action::Read, Action::Create],
            vec!["/**".to_string()],
        ));

        cart.set_policy(policy);

        // Create file
        cart.create_file("/test.txt", b"test").unwrap();

        // Multiple reads should use cache
        for _ in 0..10 {
            cart.read_file("/test.txt").unwrap();
        }

        // Clear cache
        cart.clear_policy_cache();

        // Should still work after cache clear
        cart.read_file("/test.txt").unwrap();
    }
}

/// Helper function to convert Action enum to lowercase string for capabilities
fn action_to_string_lower(action: &crate::iam::Action) -> &'static str {
    match action {
        crate::iam::Action::Read => "read",
        crate::iam::Action::Write => "write",
        crate::iam::Action::Delete => "delete",
        crate::iam::Action::List => "list",
        crate::iam::Action::Create => "create",
        crate::iam::Action::All => "*",
    }
}
