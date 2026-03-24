//! Main Cartridge API
//!
//! Provides high-level file operations for the Cartridge archive format.

use crate::allocator::{hybrid::HybridAllocator, BlockAllocator};
use crate::audit::{AuditLogger, Operation};
use crate::catalog::{Catalog, FileMetadata, FileType};
use crate::encryption::EncryptionConfig;
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
const MANIFEST_PATH: &str = ".cartridge/manifest.json";

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

    /// Encryption configuration (optional)
    encryption_config: Option<EncryptionConfig>,

    /// Enable automatic growth (default: true)
    auto_grow: bool,

    /// Maximum blocks allowed (prevents runaway growth)
    max_blocks: usize,

    /// Pages allocated for catalog overflow (multi-page serialization)
    catalog_overflow_pages: Vec<u64>,

    /// Pages allocated for allocator overflow (multi-page serialization)
    allocator_overflow_pages: Vec<u64>,
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
            encryption_config: None,
            auto_grow: true,
            max_blocks: DEFAULT_MAX_BLOCKS,
            catalog_overflow_pages: Vec::new(),
            allocator_overflow_pages: Vec::new(),
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
    pub fn create(slug: &str, title: &str) -> Result<Self> {
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
            encryption_config: None,
            auto_grow: true,
            max_blocks: DEFAULT_MAX_BLOCKS,
            catalog_overflow_pages: Vec::new(),
            allocator_overflow_pages: Vec::new(),
        };

        // Create manifest
        let manifest = Manifest::new(slug, title, semver::Version::new(0, 1, 0))?;
        let manifest_json = serde_json::to_vec_pretty(&manifest)?;

        // Ensure .cartridge directory exists
        cartridge.create_dir(".cartridge")?;

        // Write manifest to .cartridge/manifest.json
        cartridge.create_file(MANIFEST_PATH, &manifest_json)?;

        // Flush to persist catalog + allocator state to disk
        cartridge.flush()?;

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
    pub fn create_at<P: AsRef<Path>>(path: P, slug: &str, title: &str) -> Result<Self> {
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
            encryption_config: None,
            auto_grow: true,
            max_blocks: DEFAULT_MAX_BLOCKS,
            catalog_overflow_pages: Vec::new(),
            allocator_overflow_pages: Vec::new(),
        };

        // Create manifest
        let manifest = Manifest::new(slug, title, semver::Version::new(0, 1, 0))?;
        let manifest_json = serde_json::to_vec_pretty(&manifest)?;

        // Ensure .cartridge directory exists
        cartridge.create_dir(".cartridge")?;

        // Write manifest to .cartridge/manifest.json
        cartridge.create_file(MANIFEST_PATH, &manifest_json)?;

        // Flush to disk so the catalog and allocator state (including reserved
        // page tracking) are persisted. Without this, reopening the cartridge
        // would find empty catalog/allocator pages and lose the reserved block
        // allocations, causing data corruption.
        cartridge.flush()?;

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
        let mut header = file.read_header()?;

        // Load allocator first (catalog overflow pages are tracked in the allocator)
        let (mut allocator, allocator_overflow_pages) =
            Self::load_allocator_multi(&mut file, header.total_blocks as usize)?;

        // The serialized allocator doesn't know about its own overflow pages
        // (they were allocated after serialization). Mark them as allocated now
        // so future flush() calls don't double-allocate them.
        if !allocator_overflow_pages.is_empty() {
            allocator.mark_pages_allocated(&allocator_overflow_pages)?;
        }

        // Recalibrate all internal free-block counters from the actual bitmap.
        // The canonical `free_blocks` counter can become stale across
        // serialize/deserialize cycles; recalibrating from the bitmap (which is
        // the authoritative record of every allocation) eliminates the
        // desynchronization that causes spurious OutOfSpace errors.
        allocator.recalibrate();

        // Sync header free_blocks from recalibrated allocator.
        header.free_blocks = allocator.free_blocks() as u64;

        // Load catalog (may span multiple pages)
        let (catalog, catalog_overflow_pages) =
            Self::load_catalog_multi(&mut file, header.btree_root_page)?;

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
            encryption_config: None,
            auto_grow: true,
            max_blocks: DEFAULT_MAX_BLOCKS,
            catalog_overflow_pages,
            allocator_overflow_pages,
        };

        // Try to load manifest (optional for backwards compatibility)
        if let Ok(exists) = cartridge.exists(MANIFEST_PATH) {
            if !exists {
                tracing::warn!("Container opened without manifest (legacy container)");
            }
        }

        // Recover from any interrupted vacuum operations
        let mut cartridge = cartridge;
        match cartridge.recover_vacuum_wal() {
            Ok(0) => {}
            Ok(n) => tracing::info!("Recovered {n} interrupted vacuum operations on open"),
            Err(e) => tracing::warn!("WAL recovery failed (non-fatal): {e}"),
        }

        Ok(cartridge)
    }

    /// Flush all dirty pages to disk
    pub fn flush(&mut self) -> Result<()> {
        if self.file.is_none() {
            return Ok(());
        }

        let mut file = self.file.as_ref().unwrap().lock();

        // Write header (updated below after we know overflow state)
        file.write_header(&self.header)?;

        // --- Free ALL old overflow pages before any new allocations ---
        // This prevents a bug where old allocator overflow pages overlap with
        // newly allocated catalog overflow pages: if we freed allocator overflow
        // AFTER catalog allocation, an old allocator overflow page that was just
        // reallocated for catalog overflow would be incorrectly freed.
        let old_overflow_count = self.catalog_overflow_pages.len()
            + self.allocator_overflow_pages.len();
        if !self.catalog_overflow_pages.is_empty() {
            self.allocator.free(&self.catalog_overflow_pages)?;
            self.catalog_overflow_pages.clear();
        }
        if !self.allocator_overflow_pages.is_empty() {
            self.allocator.free(&self.allocator_overflow_pages)?;
            self.allocator_overflow_pages.clear();
        }
        if old_overflow_count > 0 {
            self.header.free_blocks = self.allocator.free_blocks() as u64;
        }

        // --- Catalog: serialize with bincode, write multi-page ---
        let catalog_data = self.catalog.to_bytes()?;
        self.catalog_overflow_pages = Self::write_multi_page_blob(
            &mut file,
            &self.pages,
            1,
            &catalog_data,
            &mut self.allocator,
            &mut self.header,
        )?;

        // --- Allocator: serialize with bincode, write multi-page ---
        let allocator_data = bincode::serialize(&self.allocator)
            .map_err(|e| CartridgeError::Corruption(format!("allocator serialize: {e}")))?;
        self.allocator_overflow_pages = Self::write_multi_page_blob(
            &mut file,
            &self.pages,
            2,
            &allocator_data,
            &mut self.allocator,
            &mut self.header,
        )?;

        // Re-write header (total_blocks / free_blocks may have changed from overflow)
        file.write_header(&self.header)?;

        // Write dirty content pages
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

    // =========================================================================
    // Multi-page blob serialization
    // =========================================================================

    /// Multi-page blob header discriminator.
    /// Old format: page starts with 0x7B (`{`) — raw JSON.
    /// New format: page starts with 0x00 — multi-page header.
    const MULTI_PAGE_MAGIC: u8 = 0x00;

    /// Multi-page header size: 1 (magic) + 4 (data_len) + 2 (num_overflow) = 7 bytes.
    /// Followed by num_overflow * 8 bytes of overflow page IDs (u64 LE each).
    const MULTI_PAGE_HEADER_FIXED: usize = 7;

    /// Write a blob that may span multiple pages.
    ///
    /// If the data fits in one page, writes raw data (backward compatible).
    /// If it doesn't, writes a multi-page header to the primary page and
    /// allocates overflow pages from the allocator for the remaining data.
    ///
    /// Returns the list of overflow page IDs allocated (empty if single-page).
    fn write_multi_page_blob(
        file: &mut CartridgeFile,
        pages_cache: &Mutex<std::collections::HashMap<u64, Vec<u8>>>,
        primary_page: u64,
        data: &[u8],
        allocator: &mut HybridAllocator,
        header: &mut Header,
    ) -> Result<Vec<u64>> {
        // Always write with the multi-page header format so we preserve
        // the exact data length. Bincode data can contain embedded 0x00 bytes,
        // so we can't rely on null-termination for single-page detection.
        //
        // Primary page layout: [1 magic][4 data_len][2 num_overflow][N*8 page_ids][data_chunk]

        if data.len() + Self::MULTI_PAGE_HEADER_FIXED <= PAGE_SIZE {
            // Fits in one page with header — no overflow pages needed
            let mut page = vec![0u8; PAGE_SIZE];
            page[0] = Self::MULTI_PAGE_MAGIC;
            page[1..5].copy_from_slice(&(data.len() as u32).to_le_bytes());
            page[5..7].copy_from_slice(&0u16.to_le_bytes()); // 0 overflow pages
            page[Self::MULTI_PAGE_HEADER_FIXED..Self::MULTI_PAGE_HEADER_FIXED + data.len()]
                .copy_from_slice(data);
            file.write_page_data(primary_page, &page)?;
            pages_cache.lock().insert(primary_page, page);
            return Ok(vec![]);
        }

        // Calculate overflow needed
        let mut num_overflow: u16 = 1;
        loop {
            let header_size = Self::MULTI_PAGE_HEADER_FIXED + (num_overflow as usize) * 8;
            let first_chunk = PAGE_SIZE - header_size;
            let remaining = data.len().saturating_sub(first_chunk);
            let needed = (remaining + PAGE_SIZE - 1) / PAGE_SIZE;
            if needed <= num_overflow as usize {
                break;
            }
            num_overflow = needed as u16;
        }

        // Allocate overflow pages
        let overflow_size = (num_overflow as usize) * PAGE_SIZE;
        // Ensure capacity (auto-grow if needed)
        while allocator.free_blocks() < num_overflow as usize {
            // Grow the container
            let current = header.total_blocks as usize;
            let new_total = (current * GROW_FACTOR).min(DEFAULT_MAX_BLOCKS);
            if new_total == current {
                return Err(CartridgeError::OutOfSpace);
            }
            file.extend(new_total)?;
            header.total_blocks = new_total as u64;
            allocator.extend_capacity(new_total)?;
            header.free_blocks = allocator.free_blocks() as u64;
        }

        let overflow_page_ids = allocator.allocate(overflow_size as u64)?;
        header.free_blocks = allocator.free_blocks() as u64;

        // Build primary page
        let header_size = Self::MULTI_PAGE_HEADER_FIXED + overflow_page_ids.len() * 8;
        let first_chunk_size = (PAGE_SIZE - header_size).min(data.len());
        let mut page = vec![0u8; PAGE_SIZE];

        // Header: magic + data_len + num_overflow + page_ids
        page[0] = Self::MULTI_PAGE_MAGIC;
        page[1..5].copy_from_slice(&(data.len() as u32).to_le_bytes());
        page[5..7].copy_from_slice(&(overflow_page_ids.len() as u16).to_le_bytes());
        for (i, &pid) in overflow_page_ids.iter().enumerate() {
            let off = 7 + i * 8;
            page[off..off + 8].copy_from_slice(&pid.to_le_bytes());
        }

        // First data chunk
        page[header_size..header_size + first_chunk_size]
            .copy_from_slice(&data[..first_chunk_size]);
        file.write_page_data(primary_page, &page)?;
        pages_cache.lock().insert(primary_page, page);

        // Write overflow pages
        let mut offset = first_chunk_size;
        for &pid in &overflow_page_ids {
            let mut opage = vec![0u8; PAGE_SIZE];
            let chunk = PAGE_SIZE.min(data.len() - offset);
            opage[..chunk].copy_from_slice(&data[offset..offset + chunk]);
            file.write_page_data(pid, &opage)?;
            pages_cache.lock().insert(pid, opage);
            offset += chunk;
        }

        Ok(overflow_page_ids)
    }

    /// Read a multi-page blob from disk.
    ///
    /// Detects old single-page format (starts with `{`) vs new multi-page
    /// format (starts with 0x00). Returns the reassembled data and overflow
    /// page IDs (empty for single-page).
    fn read_multi_page_blob(
        file: &mut CartridgeFile,
        primary_page: u64,
    ) -> Result<(Vec<u8>, Vec<u64>)> {
        let page_data = file.read_page_data(primary_page)?;

        if page_data[0] == Self::MULTI_PAGE_MAGIC && page_data.len() >= Self::MULTI_PAGE_HEADER_FIXED {
            // New multi-page format
            let data_len = u32::from_le_bytes([
                page_data[1], page_data[2], page_data[3], page_data[4],
            ]) as usize;
            let num_overflow = u16::from_le_bytes([
                page_data[5], page_data[6],
            ]) as usize;

            // Read overflow page IDs
            let mut overflow_pages = Vec::with_capacity(num_overflow);
            for i in 0..num_overflow {
                let off = 7 + i * 8;
                let pid = u64::from_le_bytes([
                    page_data[off], page_data[off + 1], page_data[off + 2], page_data[off + 3],
                    page_data[off + 4], page_data[off + 5], page_data[off + 6], page_data[off + 7],
                ]);
                overflow_pages.push(pid);
            }

            let header_size = Self::MULTI_PAGE_HEADER_FIXED + num_overflow * 8;
            let first_chunk_size = (PAGE_SIZE - header_size).min(data_len);

            let mut data = Vec::with_capacity(data_len);
            data.extend_from_slice(&page_data[header_size..header_size + first_chunk_size]);

            // Read overflow pages
            for &pid in &overflow_pages {
                let opage = file.read_page_data(pid)?;
                let remaining = data_len - data.len();
                let chunk = PAGE_SIZE.min(remaining);
                data.extend_from_slice(&opage[..chunk]);
            }

            Ok((data, overflow_pages))
        } else {
            // Old single-page format: raw JSON terminated by null or end of page
            let end = page_data.iter().position(|&b| b == 0).unwrap_or(PAGE_SIZE);
            Ok((page_data[..end].to_vec(), vec![]))
        }
    }

    /// Load catalog state from disk (supports multi-page, bincode + legacy JSON)
    fn load_catalog_multi(
        file: &mut CartridgeFile,
        root_page: u64,
    ) -> Result<(Catalog, Vec<u64>)> {
        let (data, overflow_pages) = Self::read_multi_page_blob(file, 1)?;

        if data.is_empty() {
            return Ok((Catalog::new(root_page), vec![]));
        }

        // Try bincode first (new format), fall back to legacy JSON
        let catalog = if data.first() == Some(&b'{') {
            // Legacy JSON format (old custom BTree)
            use crate::catalog::btree;
            let btree: btree::BTree = serde_json::from_slice(&data)
                .map_err(|e| CartridgeError::Corruption(
                    format!("Corrupted legacy catalog: {}", e)
                ))?;
            btree.into_catalog(root_page)
        } else {
            Catalog::from_bytes(&data)?
        };

        Ok((catalog, overflow_pages))
    }

    /// Load allocator state from disk (supports multi-page, bincode + legacy JSON)
    fn load_allocator_multi(
        file: &mut CartridgeFile,
        total_blocks: usize,
    ) -> Result<(HybridAllocator, Vec<u64>)> {
        let (data, overflow_pages) = Self::read_multi_page_blob(file, 2)?;

        if data.is_empty() {
            return Ok((HybridAllocator::new(total_blocks), vec![]));
        }

        // Try bincode first (new format), fall back to legacy JSON
        let allocator = if data.first() == Some(&b'{') {
            serde_json::from_slice(&data)
                .map_err(|e| CartridgeError::Corruption(
                    format!("Corrupted legacy allocator: {}", e)
                ))?
        } else {
            bincode::deserialize(&data)
                .map_err(|e| CartridgeError::Corruption(
                    format!("Corrupted allocator: {}", e)
                ))?
        };

        Ok((allocator, overflow_pages))
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

    /// Enable encryption with the provided key
    ///
    /// # Arguments
    ///
    /// * `key` - 32-byte AES-256 encryption key
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cartridge_core::Cartridge;
    /// use cartridge_core::encryption::EncryptionConfig;
    ///
    /// let mut cart = Cartridge::create("data", "My Data")?;
    /// let key = EncryptionConfig::generate_key();
    /// cart.enable_encryption(&key)?;
    /// ```
    pub fn enable_encryption(&mut self, key: &[u8; 32]) -> Result<()> {
        self.encryption_config = Some(EncryptionConfig::new(*key));
        Ok(())
    }

    /// Disable encryption
    ///
    /// Note: This does not decrypt existing encrypted files.
    /// New files written after disabling encryption will not be encrypted.
    pub fn disable_encryption(&mut self) -> Result<()> {
        self.encryption_config = None;
        Ok(())
    }

    /// Check if encryption is enabled
    pub fn is_encrypted(&self) -> bool {
        self.encryption_config
            .as_ref()
            .map(|c| c.is_enabled())
            .unwrap_or(false)
    }

    /// Get the encryption configuration (if set)
    pub(crate) fn encryption_config(&self) -> Option<&EncryptionConfig> {
        self.encryption_config.as_ref()
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

        let mut manager = SnapshotManager::new(snapshot_dir)?;

        // Load snapshot metadata from disk
        let metadata = manager.load_snapshot(snapshot_id)?;

        // Restore pages
        let restored_pages = manager.restore_snapshot(snapshot_id)?;

        // Replace current state
        *self.pages.lock() = restored_pages.clone();
        self.header = metadata.header.clone();

        // Reload catalog and allocator from restored pages (supports multi-page)
        // We need to read from disk since overflow pages may not be in the map
        if let Some(ref file_mutex) = self.file {
            let mut file = file_mutex.lock();
            let (catalog, cat_overflow) =
                Self::load_catalog_multi(&mut file, self.header.btree_root_page)?;
            self.catalog = catalog;
            self.catalog_overflow_pages = cat_overflow;

            let (mut allocator, alloc_overflow) =
                Self::load_allocator_multi(&mut file, self.header.total_blocks as usize)?;
            if !alloc_overflow.is_empty() {
                let _ = allocator.mark_pages_allocated(&alloc_overflow);
            }
            self.allocator = allocator;
            self.allocator_overflow_pages = alloc_overflow;
        } else if let Some(catalog_page) = restored_pages.get(&1) {
            // In-memory only: parse from page data directly
            let end = catalog_page.iter().position(|&b| b == 0).unwrap_or(PAGE_SIZE);
            if end > 0 {
                let data = &catalog_page[..end];
                self.catalog = if data.first() == Some(&b'{') {
                    // Legacy JSON
                    use crate::catalog::btree;
                    let btree: btree::BTree = serde_json::from_slice(data)
                        .map_err(|e| CartridgeError::Corruption(
                            format!("Corrupted legacy catalog in snapshot: {}", e)
                        ))?;
                    btree.into_catalog(1)
                } else {
                    Catalog::from_bytes(data)?
                };
            }
            if let Some(alloc_page) = restored_pages.get(&2) {
                let end = alloc_page.iter().position(|&b| b == 0).unwrap_or(PAGE_SIZE);
                if end > 0 {
                    let data = &alloc_page[..end];
                    self.allocator = if data.first() == Some(&b'{') {
                        serde_json::from_slice(data)
                            .map_err(|e| CartridgeError::Corruption(
                                format!("Corrupted legacy allocator in snapshot: {}", e)
                            ))?
                    } else {
                        bincode::deserialize(data)
                            .map_err(|e| CartridgeError::Corruption(
                                format!("Corrupted allocator in snapshot: {}", e)
                            ))?
                    };
                }
            }
        }

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
        self.check_access(&Action::Create, &path)?;

        // Check if file already exists
        if self.catalog.get(&path)?.is_some() {
            return Err(CartridgeError::Allocation(format!(
                "File already exists: {}",
                path
            )));
        }

        // Encrypt content if encryption is enabled
        let (final_content, was_encrypted) = if let Some(config) = &self.encryption_config {
            use crate::encryption::encrypt_if_enabled;
            encrypt_if_enabled(content, config)?
        } else {
            (content.to_vec(), false)
        };

        // Ensure capacity before allocating (using final content size after encryption)
        if !final_content.is_empty() {
            self.ensure_capacity(final_content.len())?;
        }

        // Allocate blocks for content
        let blocks = if final_content.is_empty() {
            Vec::new()
        } else {
            self.allocator.allocate(final_content.len() as u64)?
        };

        // Write content to pages (encrypted if enabled)
        self.write_content(&blocks, &final_content)?;

        // Create metadata (store original size and encryption flag)
        let mut metadata = FileMetadata::new(FileType::File, content.len() as u64, blocks);
        if was_encrypted {
            // Store encryption flag and encrypted size in user metadata
            metadata.user_metadata.insert("encrypted".to_string(), "true".to_string());
            metadata.user_metadata.insert("encrypted_size".to_string(), final_content.len().to_string());
        }

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

        // Check if file was encrypted
        let was_encrypted = metadata.user_metadata.get("encrypted").map(|v| v == "true").unwrap_or(false);

        // If encrypted, we need to read the encrypted size, not the original size
        // If not encrypted, use the metadata size
        let read_size = if was_encrypted {
            // For encrypted files, read the encrypted size stored in metadata
            metadata.user_metadata
                .get("encrypted_size")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(metadata.size as usize) // Fallback to original size if not set
        } else {
            // For unencrypted files, use the original size
            metadata.size as usize
        };

        // Read content from blocks (this reads the raw data, encrypted or not)
        let raw_content = self.read_content(&metadata.blocks, read_size)?;

        // Decrypt if needed
        if was_encrypted {
            if let Some(config) = &self.encryption_config {
                use crate::encryption::decrypt_if_encrypted;
                decrypt_if_encrypted(&raw_content, config, true)
            } else {
                Err(CartridgeError::Allocation(
                    "File is encrypted but no encryption key is set".to_string()
                ))
            }
        } else {
            Ok(raw_content)
        }
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

        // Encrypt content if encryption is enabled
        let (final_content, was_encrypted) = if let Some(config) = &self.encryption_config {
            use crate::encryption::encrypt_if_enabled;
            encrypt_if_enabled(content, config)?
        } else {
            (content.to_vec(), false)
        };

        // Ensure capacity before allocating (using final content size after encryption)
        if !final_content.is_empty() {
            self.ensure_capacity(final_content.len())?;
        }

        // Free old blocks
        if !metadata.blocks.is_empty() {
            self.allocator.free(&metadata.blocks)?;
        }

        // Allocate new blocks
        let new_blocks = if final_content.is_empty() {
            Vec::new()
        } else {
            self.allocator.allocate(final_content.len() as u64)?
        };

        // Write new content (encrypted if enabled)
        self.write_content(&new_blocks, &final_content)?;

        // Update metadata (store original size and encryption flag)
        metadata.size = content.len() as u64;
        metadata.blocks = new_blocks;
        metadata.touch();
        if was_encrypted {
            metadata.user_metadata.insert("encrypted".to_string(), "true".to_string());
            metadata.user_metadata.insert("encrypted_size".to_string(), final_content.len().to_string());
        } else {
            metadata.user_metadata.remove("encrypted");
            metadata.user_metadata.remove("encrypted_size");
        }

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
        // Empty path means list all files (no prefix filter)
        let prefix = if path.is_empty() {
            String::new()
        } else if path.ends_with('/') {
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
        let (path, file_size_bytes) = if let Some(file) = &self.file {
            let f = file.lock();
            let p = f.path().to_path_buf();
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            (Some(p), size)
        } else {
            (None, 0)
        };
        CartridgeStats {
            total_blocks: self.header.total_blocks,
            free_blocks: self.header.free_blocks,
            used_blocks: self.header.total_blocks - self.header.free_blocks,
            fragmentation: self.allocator.fragmentation_score(),
            path,
            file_size_bytes,
        }
    }

    /// Copy all live files to a new cartridge at `dest`, producing a compact
    /// copy with no free (unallocated) blocks.
    ///
    /// The original cartridge is left unchanged. After this call the caller
    /// can atomically replace the original file with `dest` to reclaim space.
    pub fn vacuum_into(&self, dest: &Path) -> Result<()> {
        if dest.exists() {
            std::fs::remove_file(dest)?;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut new_cart = Cartridge::create_at(dest, "vacuum", "vacuum")?;

        for path in self.list_dir("")? {
            // Skip internal container entries — new_cart creates its own manifest.
            if path == ".cartridge" || path.starts_with(".cartridge/") {
                continue;
            }
            let data = self.read_file(&path)?;
            if new_cart.exists(&path)? {
                new_cart.write_file(&path, &data)?;
            } else {
                new_cart.create_file(&path, &data)?;
            }
        }

        new_cart.flush()?;
        Ok(())
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
    /// Overwrites the existing manifest at .cartridge/manifest.json
    pub fn write_manifest(&mut self, manifest: &Manifest) -> Result<()> {
        let manifest_json = serde_json::to_vec_pretty(manifest)?;

        // Check if manifest file exists
        if self.exists(MANIFEST_PATH)? {
            self.write_file(MANIFEST_PATH, &manifest_json)?;
        } else {
            // Ensure directory exists
            if !self.exists(".cartridge")? {
                self.create_dir(".cartridge")?;
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

        // Update header total_blocks
        self.header.total_blocks = new_total as u64;

        // Extend allocator capacity (this updates allocator's free_blocks)
        self.allocator.extend_capacity(new_total)?;

        // Sync header free_blocks from allocator
        self.header.free_blocks = self.allocator.free_blocks() as u64;

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

    // =====================================================================
    // WAL + Incremental Vacuum
    // =====================================================================

    /// Path of the vacuum WAL file inside the VFS.
    const VACUUM_WAL_PATH: &'static str = "wal/vacuum/wal.log";
    /// Directory containing WAL files.
    const WAL_DIR: &'static str = "wal";
    /// Subdirectory for vacuum WAL.
    const VACUUM_WAL_DIR: &'static str = "wal/vacuum";

    /// Check whether this cartridge has enough wasted space to justify vacuum.
    ///
    /// Returns `true` if more than 50% of pages are dead OR more than 10 MB
    /// of dead space exists.
    pub fn needs_vacuum(&self) -> bool {
        let total = self.header.total_blocks;
        let free = self.header.free_blocks;
        if total <= MIN_BLOCKS as u64 {
            return false;
        }

        // Free blocks are reclaimable by truncation.
        let waste = free;
        let waste_bytes = waste * PAGE_SIZE as u64;
        let waste_ratio = waste as f64 / total as f64;

        waste_ratio > 0.5 || waste_bytes > 10 * 1024 * 1024
    }

    /// Ensure the vacuum WAL file exists in the VFS with pre-allocated pages.
    ///
    /// If the file already exists, loads and returns it.
    /// If it doesn't, creates it with `DEFAULT_WAL_PAGES` pages.
    fn ensure_vacuum_wal(&mut self) -> Result<crate::wal::WalFile> {
        use crate::wal;

        if self.exists(Self::VACUUM_WAL_PATH)? {
            // Load existing WAL
            let meta = self.metadata(Self::VACUUM_WAL_PATH)?;
            let mut page_data = Vec::new();
            for &page_id in &meta.blocks {
                let data = self.read_page_data_raw(page_id)?;
                page_data.push((page_id, data));
            }
            return wal::WalFile::load(page_data);
        }

        // Create WAL directory structure
        if !self.exists(Self::WAL_DIR)? {
            self.create_dir(Self::WAL_DIR)?;
        }
        if !self.exists(Self::VACUUM_WAL_DIR)? {
            self.create_dir(Self::VACUUM_WAL_DIR)?;
        }

        // Pre-allocate pages for the WAL file
        let wal_size = wal::DEFAULT_WAL_PAGES * PAGE_SIZE;
        let zero_content = vec![0u8; wal_size];
        self.create_file(Self::VACUUM_WAL_PATH, &zero_content)?;

        // Read back the allocated page IDs
        let meta = self.metadata(Self::VACUUM_WAL_PATH)?;
        let wal = wal::WalFile::new(meta.blocks)?;

        // Write initial header
        let header_write = wal.header_write();
        self.apply_wal_write(&header_write)?;

        Ok(wal)
    }

    /// Apply a WAL write descriptor to both the backing file and the page cache.
    ///
    /// WAL writes are partial-page overwrites. We must update the page cache
    /// so that a subsequent `flush()` doesn't overwrite our WAL data with
    /// stale cached page content.
    fn apply_wal_write(&self, write: &crate::wal::WalWrite) -> Result<()> {
        // Update page cache — ensures flush() won't clobber WAL data
        {
            let mut pages = self.pages.lock();
            let mut dirty = self.dirty_pages.lock();
            let page = pages.entry(write.page_id).or_insert_with(|| {
                if let Some(file) = &self.file {
                    file.lock().read_page_data(write.page_id).unwrap_or_else(|_| vec![0u8; PAGE_SIZE])
                } else {
                    vec![0u8; PAGE_SIZE]
                }
            });
            let end = write.offset_in_page + write.data.len();
            page[write.offset_in_page..end].copy_from_slice(&write.data);
            dirty.insert(write.page_id);
        }

        // Also write directly to disk for immediate durability
        if let Some(file) = &self.file {
            file.lock().write_at(write.page_id, write.offset_in_page, &write.data)?;
        }
        Ok(())
    }

    /// Fsync the backing file.
    fn sync_file(&self) -> Result<()> {
        if let Some(file) = &self.file {
            file.lock().sync()?;
        }
        Ok(())
    }

    /// Read raw page data (bypass page cache, direct from disk or cache).
    fn read_page_data_raw(&self, page_id: u64) -> Result<Vec<u8>> {
        let pages = self.pages.lock();
        if let Some(data) = pages.get(&page_id) {
            return Ok(data.clone());
        }
        drop(pages);

        if let Some(file) = &self.file {
            return file.lock().read_page_data(page_id);
        }

        Err(CartridgeError::Allocation(format!(
            "Page {} not found and no disk backing",
            page_id
        )))
    }

    /// Build a reverse map: page_id → (catalog_path, index_in_blocks_vec).
    ///
    /// This tells us which catalog entry owns each content page so we can
    /// update the `blocks` vec after relocating.
    fn build_page_owner_map(&self) -> Result<std::collections::HashMap<u64, (String, usize)>> {
        let mut map = std::collections::HashMap::new();

        for (path, meta) in self.catalog.list_prefix("")? {
            // Skip WAL files — they must never be relocated
            if path.starts_with(crate::wal::WAL_PREFIX) {
                continue;
            }
            for (idx, &page_id) in meta.blocks.iter().enumerate() {
                map.insert(page_id, (path.clone(), idx));
            }
        }

        Ok(map)
    }

    /// Run one incremental vacuum step, relocating up to `batch_size` pages.
    ///
    /// Returns progress information. Call repeatedly until `done` is true,
    /// then call `vacuum_finish()` to truncate the file.
    ///
    /// Each page relocation is WAL-journaled. Crash at any point is safe.
    pub fn vacuum_step(&mut self, batch_size: usize) -> Result<VacuumProgress> {
        use crate::wal::{WalOp, WalState, fnv1a_hash};

        if self.file.is_none() {
            return Ok(VacuumProgress {
                pages_relocated: 0,
                pages_remaining: 0,
                bytes_reclaimable: 0,
                done: true,
            });
        }

        let mut wal = self.ensure_vacuum_wal()?;

        // Build the live page set and owner map
        let owner_map = self.build_page_owner_map()?;

        // Collect all live page IDs (content + infrastructure).
        // Infrastructure pages that must not move: 0 (header), 1 (catalog root), 2 (allocator root)
        // Overflow pages and WAL pages are live but also shouldn't be relocated.
        let mut live_pages: std::collections::HashSet<u64> = std::collections::HashSet::new();
        live_pages.insert(0);
        live_pages.insert(1);
        live_pages.insert(2);
        for &p in &self.catalog_overflow_pages {
            live_pages.insert(p);
        }
        for &p in &self.allocator_overflow_pages {
            live_pages.insert(p);
        }
        // WAL pages
        for &p in wal.page_ids() {
            live_pages.insert(p);
        }
        // Content pages (everything tracked by catalog, including WAL files in VFS)
        for (_, meta) in self.catalog.list_prefix("")? {
            for &p in &meta.blocks {
                live_pages.insert(p);
            }
        }

        // Find the compact boundary: the smallest total_blocks where all live
        // pages fit. That's max(live_page_id) + 1.
        let high_water = live_pages.iter().copied().max().unwrap_or(2) + 1;
        let current_total = self.header.total_blocks;

        if high_water >= current_total {
            // Nothing to reclaim — all pages are packed
            let writes = wal.clear();
            for w in &writes {
                self.apply_wal_write(w)?;
            }
            self.sync_file()?;
            return Ok(VacuumProgress {
                pages_relocated: 0,
                pages_remaining: 0,
                bytes_reclaimable: 0,
                done: true,
            });
        }

        // Find pages that need to relocate: live content pages above the
        // target boundary. We want to pack everything below high_water,
        // but first we need free slots below high_water to move things into.
        //
        // Strategy: find the highest live content pages and move them into
        // the lowest free slots.

        // Relocatable pages: live content pages (not infrastructure, not WAL)
        let mut relocatable: Vec<u64> = owner_map.keys()
            .copied()
            .collect();
        relocatable.sort_unstable();
        relocatable.reverse(); // highest first

        // Free slots below current high_water, lowest first
        let mut free_slots: Vec<u64> = Vec::new();
        for page_id in 3..high_water {
            if !live_pages.contains(&page_id) {
                free_slots.push(page_id);
            }
        }
        // We only need to relocate pages that are above all free slots.
        // The goal: move high pages into low free slots.

        // Find pages that ARE above the minimum viable boundary.
        // The minimum viable boundary = total live pages (they could all fit
        // in pages 0..live_count if perfectly packed).
        let min_boundary = live_pages.len() as u64;

        // Pages to relocate: high live pages that are above where they'd be
        // in a compacted layout, paired with free slots below them.
        let mut moves_planned = 0usize;
        let mut free_idx = 0;

        for &high_page in &relocatable {
            if moves_planned >= batch_size {
                break;
            }
            // Only relocate if this page is above the minimum boundary
            // AND there's a free slot below it
            if high_page < min_boundary {
                continue;
            }
            // Find a free slot below this page
            while free_idx < free_slots.len() && free_slots[free_idx] >= high_page {
                free_idx += 1;
            }
            if free_idx >= free_slots.len() {
                break; // No more free slots below
            }
            // Skip if the free slot isn't actually lower
            if free_slots[free_idx] >= high_page {
                continue;
            }

            let dest = free_slots[free_idx];
            free_idx += 1;

            let (ref path, block_index) = owner_map[&high_page];
            let path_hash = fnv1a_hash(path);

            // 1. Write WAL intent
            let (entry, intent_write) = wal.append(
                WalOp::VacuumRelocate,
                WalState::Intent,
                high_page,
                dest,
                path_hash,
                block_index as u32,
            )?;
            self.apply_wal_write(&intent_write)?;
            let hdr_write = wal.header_write();
            self.apply_wal_write(&hdr_write)?;
            self.sync_file()?;

            // 2. Copy page content
            let page_content = self.read_page_data_raw(high_page)?;
            {
                let mut pages = self.pages.lock();
                let mut dirty = self.dirty_pages.lock();
                pages.insert(dest, page_content);
                dirty.insert(dest);
                // Write dest page to disk immediately
            }
            if let Some(file) = &self.file {
                let pages = self.pages.lock();
                if let Some(data) = pages.get(&dest) {
                    file.lock().write_page_data(dest, data)?;
                }
            }

            // Update WAL: written
            let written_write = wal.update_state(entry.sequence, WalState::Written)?;
            self.apply_wal_write(&written_write)?;
            self.sync_file()?;

            // 3. Update catalog: point file's block from high_page to dest
            if let Some(mut meta) = self.catalog.get(path)? {
                meta.blocks[block_index] = dest;
                self.catalog.insert(path, meta)?;
            }

            // Free the old page in the allocator
            self.allocator.free(&[high_page])?;
            self.header.free_blocks = self.allocator.free_blocks() as u64;

            // Remove old page from cache
            {
                let mut pages = self.pages.lock();
                pages.remove(&high_page);
            }

            // Update WAL: committed
            let committed_write = wal.update_state(entry.sequence, WalState::Committed)?;
            self.apply_wal_write(&committed_write)?;
            self.sync_file()?;

            moves_planned += 1;
        }

        // Calculate remaining work
        let remaining = relocatable.iter()
            .filter(|&&p| p >= min_boundary)
            .count()
            .saturating_sub(moves_planned);

        let reclaimable = (current_total - min_boundary) * PAGE_SIZE as u64;

        // If no more work to do, clear the WAL
        let done = remaining == 0 && moves_planned == 0;
        if done || remaining == 0 {
            let writes = wal.clear();
            for w in &writes {
                self.apply_wal_write(w)?;
            }
            self.sync_file()?;
        }

        Ok(VacuumProgress {
            pages_relocated: moves_planned,
            pages_remaining: remaining,
            bytes_reclaimable: reclaimable,
            done: remaining == 0,
        })
    }

    /// Finish vacuum: truncate the file to reclaim disk space.
    ///
    /// Call this after `vacuum_step()` returns `done: true`.
    /// Shrinks the allocator, truncates the backing file, and flushes.
    pub fn vacuum_finish(&mut self) -> Result<u64> {
        // Recompute the high water mark
        let mut max_live: u64 = 2; // minimum: pages 0, 1, 2
        for &p in &self.catalog_overflow_pages {
            max_live = max_live.max(p);
        }
        for &p in &self.allocator_overflow_pages {
            max_live = max_live.max(p);
        }
        for (_, meta) in self.catalog.list_prefix("")? {
            for &p in &meta.blocks {
                max_live = max_live.max(p);
            }
        }

        let new_total = (max_live + 1) as usize;
        let old_total = self.header.total_blocks as usize;

        if new_total >= old_total {
            return Ok(0); // Nothing to truncate
        }

        let bytes_freed = ((old_total - new_total) * PAGE_SIZE) as u64;

        tracing::info!(
            "Vacuum truncate: {} -> {} blocks ({} bytes reclaimed)",
            old_total,
            new_total,
            bytes_freed
        );

        // Shrink allocator
        self.allocator.shrink_capacity(new_total)?;
        self.header.total_blocks = new_total as u64;
        self.header.free_blocks = self.allocator.free_blocks() as u64;

        // Flush catalog + allocator + header to their (now lower) pages
        self.flush()?;

        // Truncate the backing file
        if let Some(file) = &self.file {
            file.lock().shrink(new_total)?;
        }

        Ok(bytes_freed)
    }

    /// Recover from a crashed vacuum by replaying or discarding WAL entries.
    ///
    /// Called automatically by `open()` if a dirty WAL is found.
    pub fn recover_vacuum_wal(&mut self) -> Result<usize> {
        use crate::wal::WalState;

        if !self.exists(Self::VACUUM_WAL_PATH)? {
            return Ok(0);
        }

        let meta = self.metadata(Self::VACUUM_WAL_PATH)?;
        let mut page_data = Vec::new();
        for &page_id in &meta.blocks {
            let data = self.read_page_data_raw(page_id)?;
            page_data.push((page_id, data));
        }

        let mut wal = crate::wal::WalFile::load(page_data)?;
        if !wal.is_dirty() {
            return Ok(0);
        }

        let pending: Vec<crate::wal::WalEntry> = wal.pending_entries()
            .into_iter()
            .copied()
            .collect();
        let mut recovered = 0;

        for entry in &pending {
            match entry.state {
                WalState::Intent => {
                    // Nothing happened. Discard.
                    tracing::info!(
                        "WAL recovery: discarding intent seq={} (page {} → {})",
                        entry.sequence, entry.source_page, entry.dest_page
                    );
                }
                WalState::Written => {
                    // Data was copied to dest but catalog not updated.
                    // Check if source is still the canonical location (catalog).
                    // If so, the move didn't complete — just discard dest.
                    // The source page is still valid.
                    tracing::info!(
                        "WAL recovery: discarding incomplete move seq={} (page {} → {}), source intact",
                        entry.sequence, entry.source_page, entry.dest_page
                    );
                    // Free the dest page if it was allocated
                    if self.allocator.is_allocated(entry.dest_page) {
                        // Only free if it's not used by something else
                        let owner_map = self.build_page_owner_map()?;
                        if !owner_map.contains_key(&entry.dest_page) {
                            self.allocator.free(&[entry.dest_page])?;
                            self.header.free_blocks = self.allocator.free_blocks() as u64;
                        }
                    }
                    recovered += 1;
                }
                WalState::Committed => {
                    // Already done. Just clear.
                }
            }
        }

        // Clear the WAL
        let writes = wal.clear();
        for w in &writes {
            self.apply_wal_write(w)?;
        }
        self.sync_file()?;

        if recovered > 0 {
            tracing::info!("WAL recovery: cleaned up {} incomplete operations", recovered);
        }

        Ok(recovered)
    }
}

/// Progress of an incremental vacuum operation.
#[derive(Debug, Clone)]
pub struct VacuumProgress {
    /// Pages relocated in this step.
    pub pages_relocated: usize,
    /// Pages still needing relocation.
    pub pages_remaining: usize,
    /// Bytes that can be reclaimed once relocation is complete.
    pub bytes_reclaimable: u64,
    /// True when all relocation is done and `vacuum_finish()` can be called.
    pub done: bool,
}

impl Drop for Cartridge {
    fn drop(&mut self) {
        // Automatically flush on drop to prevent data loss
        if self.file.is_some() {
            if let Err(e) = self.flush() {
                tracing::warn!("Failed to flush cartridge on drop: {}", e);
            }
        }
    }
}

/// Cartridge statistics
#[derive(Debug, Clone)]
pub struct CartridgeStats {
    pub total_blocks: u64,
    pub free_blocks: u64,
    pub used_blocks: u64,
    pub fragmentation: f64,
    /// Disk path of the backing file, or `None` for in-memory cartridges.
    pub path: Option<std::path::PathBuf>,
    /// Size of the backing file in bytes, or 0 for in-memory cartridges.
    pub file_size_bytes: u64,
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
        cart.create_dir(".cartridge").unwrap();
        cart.write_manifest(&manifest).unwrap();

        // Update using the closure API
        cart.update_manifest(|m| {
            m.description = Some("Updated description".to_string());
        })
        .unwrap();

        // Verify update persisted
        let updated = cart.read_manifest().unwrap();
        assert_eq!(updated.description, Some("Updated description".to_string()));
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

    #[test]
    fn test_multi_page_catalog_flush_and_reload() {
        // Create a disk-backed cartridge with many files to overflow the catalog
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large-catalog");

        let mut cart = Cartridge::create_at(&path, "large-catalog", "Large Catalog Test").unwrap();

        // Insert enough files to make the catalog exceed PAGE_SIZE (4096 bytes)
        // Each entry is roughly ~150 bytes in JSON, so ~30 entries should overflow
        for i in 0..200 {
            let filename = format!("dir/subdir/file-with-a-long-name-{:04}.dat", i);
            let content = format!("content-{}", i);
            cart.create_file(&filename, content.as_bytes()).unwrap();
        }

        // Verify catalog bincode would exceed single page
        let catalog_data = cart.catalog.to_bytes().unwrap();
        assert!(
            catalog_data.len() > PAGE_SIZE,
            "Catalog should exceed single page: {} bytes",
            catalog_data.len()
        );

        // Flush should succeed (was previously an error)
        cart.flush().unwrap();

        // Overflow pages should have been allocated
        assert!(
            !cart.catalog_overflow_pages.is_empty(),
            "Should have catalog overflow pages"
        );

        // Close and reopen
        drop(cart);
        let cart2 = Cartridge::open(&path).unwrap();

        // Verify all files are readable after reload
        for i in 0..200 {
            let filename = format!("dir/subdir/file-with-a-long-name-{:04}.dat", i);
            let content = cart2.read_file(&filename).unwrap();
            assert_eq!(content, format!("content-{}", i).as_bytes());
        }
    }

    #[test]
    fn test_backward_compatible_single_page_catalog() {
        // Small cartridge should still use single-page format (backward compatible)
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("small-catalog");

        let mut cart = Cartridge::create_at(&path, "small-catalog", "Small Catalog Test").unwrap();
        cart.create_file("hello.txt", b"world").unwrap();
        cart.flush().unwrap();

        // Should have no overflow pages
        assert!(cart.catalog_overflow_pages.is_empty());
        assert!(cart.allocator_overflow_pages.is_empty());

        // Reopen and verify
        drop(cart);
        let cart2 = Cartridge::open(&path).unwrap();
        assert_eq!(cart2.read_file("hello.txt").unwrap(), b"world");
    }

    #[test]
    fn test_multi_page_catalog_repeated_flush() {
        // Verify that overflow pages are properly freed and reallocated on each flush
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi-flush");

        let mut cart = Cartridge::create_at(&path, "multi-flush", "Multi Flush Test").unwrap();

        // Create enough files to trigger overflow
        for i in 0..150 {
            let filename = format!("f{:04}.dat", i);
            cart.create_file(&filename, b"x").unwrap();
        }
        cart.flush().unwrap();
        let first_overflow = cart.catalog_overflow_pages.clone();
        assert!(!first_overflow.is_empty());

        // Add more files and flush again
        for i in 150..200 {
            let filename = format!("f{:04}.dat", i);
            cart.create_file(&filename, b"y").unwrap();
        }
        cart.flush().unwrap();

        // Overflow pages may differ (old ones freed, new ones allocated)
        // But all files should still be accessible
        drop(cart);
        let cart2 = Cartridge::open(&path).unwrap();
        for i in 0..200 {
            let filename = format!("f{:04}.dat", i);
            assert!(cart2.exists(&filename).unwrap(), "File {} should exist", filename);
        }
    }

    #[test]
    fn test_multi_page_no_overflow_overlap() {
        // Regression test: on the second flush, old allocator overflow pages
        // could overlap with newly allocated catalog overflow pages, causing
        // the allocator's write to corrupt the catalog data.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("overlap-test");

        let mut cart = Cartridge::create_at(&path, "overlap", "Overlap Test").unwrap();

        // Create enough files to trigger multi-page catalog overflow
        for i in 0..200 {
            let filename = format!("project/data/file-{:04}.dat", i);
            let content = format!("content-for-file-{}", i);
            cart.create_file(&filename, content.as_bytes()).unwrap();
        }
        cart.flush().unwrap();

        assert!(!cart.catalog_overflow_pages.is_empty(), "Should have catalog overflow");

        // Verify no overlap between catalog and allocator overflow pages
        let cat_set: std::collections::HashSet<u64> =
            cart.catalog_overflow_pages.iter().copied().collect();
        let alloc_set: std::collections::HashSet<u64> =
            cart.allocator_overflow_pages.iter().copied().collect();
        let overlap: Vec<u64> = cat_set.intersection(&alloc_set).copied().collect();
        assert!(overlap.is_empty(), "Overflow pages must not overlap: {:?}", overlap);

        // Close and reopen (this tests the allocator overflow marking on load)
        drop(cart);
        let mut cart = Cartridge::open(&path).unwrap();

        // Add more files (grows catalog, requires new overflow allocation)
        for i in 200..350 {
            let filename = format!("project/data/file-{:04}.dat", i);
            let content = format!("new-content-{}", i);
            cart.create_file(&filename, content.as_bytes()).unwrap();
        }

        // Second flush — this is where the overlap bug would manifest
        cart.flush().unwrap();

        // Verify no overlap after second flush
        let cat_set2: std::collections::HashSet<u64> =
            cart.catalog_overflow_pages.iter().copied().collect();
        let alloc_set2: std::collections::HashSet<u64> =
            cart.allocator_overflow_pages.iter().copied().collect();
        let overlap2: Vec<u64> = cat_set2.intersection(&alloc_set2).copied().collect();
        assert!(overlap2.is_empty(), "Overflow pages must not overlap after re-flush: {:?}", overlap2);

        // Close and reopen — verify all data is intact
        drop(cart);
        let cart = Cartridge::open(&path).unwrap();
        let mut missing = Vec::new();
        for i in 0..350 {
            let filename = format!("project/data/file-{:04}.dat", i);
            if !cart.exists(&filename).unwrap() {
                missing.push(i);
            }
        }
        assert!(missing.is_empty(), "Missing {} files after reopen: {:?}",
            missing.len(), &missing[..missing.len().min(20)]);
    }

    #[test]
    fn test_350_files_single_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big-single");

        let mut cart = Cartridge::create_at(&path, "big", "Big Test").unwrap();
        for i in 0..350 {
            let filename = format!("d/f-{:04}.dat", i);
            cart.create_file(&filename, format!("c-{}", i).as_bytes()).unwrap();
        }

        // Verify catalog has all entries before flush
        let mut pre_missing = Vec::new();
        for i in 0..350 {
            let filename = format!("d/f-{:04}.dat", i);
            if !cart.exists(&filename).unwrap() {
                pre_missing.push(i);
            }
        }
        if !pre_missing.is_empty() {
            // BTree has a bug losing entries
            panic!("Pre-flush: missing {} files: {:?}",
                pre_missing.len(), &pre_missing[..pre_missing.len().min(30)]);
        }

        // Check catalog bincode size
        let catalog_data = cart.catalog.to_bytes().unwrap();
        eprintln!("Catalog bincode size: {} bytes", catalog_data.len());

        cart.flush().unwrap();

        // After flush, check catalog overflow
        eprintln!("Catalog overflow pages: {:?}", cart.catalog_overflow_pages);
        eprintln!("Allocator overflow pages: {:?}", cart.allocator_overflow_pages);

        // Verify still accessible before reopen
        for i in 0..350 {
            let filename = format!("d/f-{:04}.dat", i);
            assert!(cart.exists(&filename).unwrap(), "Post-flush: missing {}", filename);
        }

        drop(cart);
        let cart = Cartridge::open(&path).unwrap();
        let mut missing = Vec::new();
        for i in 0..350 {
            let filename = format!("d/f-{:04}.dat", i);
            if !cart.exists(&filename).unwrap() {
                missing.push(i);
            }
        }
        assert!(missing.is_empty(), "Missing {} files after reopen: {:?}",
            missing.len(), &missing[..missing.len().min(20)]);
    }

    #[test]
    fn test_multi_page_with_delete_and_reopen() {
        // Test that delete+create+flush+reopen preserves all data
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("delete-reopen-test");

        let mut cart = Cartridge::create_at(&path, "delreopen", "Delete Reopen Test").unwrap();

        for i in 0..200 {
            let filename = format!("d/f-{:04}.dat", i);
            cart.create_file(&filename, format!("c-{}", i).as_bytes()).unwrap();
        }
        cart.flush().unwrap();

        drop(cart);
        let mut cart = Cartridge::open(&path).unwrap();

        // Delete some, create others
        for i in 0..50 {
            let filename = format!("d/f-{:04}.dat", i);
            cart.delete_file(&filename).unwrap();
        }
        for i in 200..350 {
            let filename = format!("d/f-{:04}.dat", i);
            cart.create_file(&filename, format!("n-{}", i).as_bytes()).unwrap();
        }
        cart.flush().unwrap();

        drop(cart);
        let cart = Cartridge::open(&path).unwrap();
        let mut missing = Vec::new();
        for i in 50..350 {
            let filename = format!("d/f-{:04}.dat", i);
            if !cart.exists(&filename).unwrap() {
                missing.push(i);
            }
        }
        assert!(missing.is_empty(), "Missing {} files: {:?}",
            missing.len(), &missing[..missing.len().min(20)]);
    }

    // ===================================================================
    // Vacuum tests
    // ===================================================================

    #[test]
    fn test_needs_vacuum_empty_cart() {
        let cart = Cartridge::new(1000);
        // 1000 blocks, 997 free (3 reserved). That's 99.7% waste.
        assert!(cart.needs_vacuum());
    }

    #[test]
    fn test_needs_vacuum_full_cart() {
        let mut cart = Cartridge::new(100);
        // Fill it up with files to reduce free space
        for i in 0..90 {
            let name = format!("f{}.dat", i);
            let data = vec![0xABu8; 2048]; // half a page each
            cart.create_file(&name, &data).unwrap();
        }
        // With 90 files consuming ~90 pages out of 100 (after auto-grow),
        // waste should be low
        let stats = cart.stats();
        let waste_ratio = stats.free_blocks as f64 / stats.total_blocks as f64;
        if waste_ratio < 0.5 {
            assert!(!cart.needs_vacuum());
        }
    }

    #[test]
    fn test_vacuum_step_in_memory() {
        // In-memory carts return done immediately (no file to truncate)
        let mut cart = Cartridge::new(100);
        let progress = cart.vacuum_step(4).unwrap();
        assert!(progress.done);
    }

    #[test]
    fn test_vacuum_incremental_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vacuum-test.cart");

        // Create cart and fill with 50 files (~50 pages of content)
        {
            let mut cart = Cartridge::create_at(&path, "vacuum-test", "Vacuum Test").unwrap();
            for i in 0..50 {
                let name = format!("file-{:03}.dat", i);
                let data = vec![(i as u8); PAGE_SIZE]; // exactly 1 page each
                cart.create_file(&name, &data).unwrap();
            }
            cart.flush().unwrap();
        }

        let size_before_delete = std::fs::metadata(&path).unwrap().len();

        // Reopen and delete 40 of the 50 files, leaving 10
        {
            let mut cart = Cartridge::open(&path).unwrap();
            for i in 0..40 {
                let name = format!("file-{:03}.dat", i);
                cart.delete_file(&name).unwrap();
            }
            cart.flush().unwrap();

            assert!(cart.needs_vacuum(), "Should need vacuum after deleting 80% of files");

            // Run vacuum incrementally
            let mut total_relocated = 0;
            for _ in 0..100 {
                let progress = cart.vacuum_step(4).unwrap();
                total_relocated += progress.pages_relocated;
                if progress.done {
                    break;
                }
            }

            // Finish vacuum — truncate
            let bytes_freed = cart.vacuum_finish().unwrap();
            assert!(bytes_freed > 0, "Should have freed some bytes");
        }

        let size_after_vacuum = std::fs::metadata(&path).unwrap().len();
        assert!(
            size_after_vacuum < size_before_delete,
            "File should be smaller after vacuum: {} vs {}",
            size_after_vacuum,
            size_before_delete
        );

        // Verify surviving files are intact
        {
            let cart = Cartridge::open(&path).unwrap();
            for i in 40..50 {
                let name = format!("file-{:03}.dat", i);
                let data = cart.read_file(&name).unwrap();
                assert_eq!(data.len(), PAGE_SIZE);
                assert!(data.iter().all(|&b| b == i as u8),
                    "File {} content corrupted after vacuum", name);
            }
        }
    }

    #[test]
    fn test_vacuum_crash_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crash-test.cart");

        // Create cart with files
        {
            let mut cart = Cartridge::create_at(&path, "crash-test", "Crash Test").unwrap();
            for i in 0..20 {
                let name = format!("data-{:03}.dat", i);
                let data = vec![(i as u8); PAGE_SIZE];
                cart.create_file(&name, &data).unwrap();
            }
            cart.flush().unwrap();
        }

        // Delete files and create WAL but DON'T finish vacuum
        {
            let mut cart = Cartridge::open(&path).unwrap();
            for i in 0..15 {
                let name = format!("data-{:03}.dat", i);
                cart.delete_file(&name).unwrap();
            }
            cart.flush().unwrap();

            // Start vacuum — do one step to create WAL entries
            let progress = cart.vacuum_step(2).unwrap();
            assert!(progress.pages_relocated > 0 || progress.done);

            // Flush to persist WAL and catalog changes, but don't finish
            cart.flush().unwrap();
            // Drop without vacuum_finish — simulates crash
        }

        // Reopen — recovery should handle the dirty WAL
        {
            let cart = Cartridge::open(&path).unwrap();

            // Verify surviving files are intact
            for i in 15..20 {
                let name = format!("data-{:03}.dat", i);
                let data = cart.read_file(&name).unwrap();
                assert_eq!(data.len(), PAGE_SIZE);
                assert!(data.iter().all(|&b| b == i as u8),
                    "File {} corrupted after crash recovery", name);
            }
        }
    }

    #[test]
    fn test_vacuum_no_work_needed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-work.cart");

        // Create a small cart — pages are packed at front
        {
            let mut cart = Cartridge::create_at(&path, "no-work", "No Work").unwrap();
            cart.create_file("keep.txt", b"hello").unwrap();
            cart.flush().unwrap();
        }

        {
            let mut cart = Cartridge::open(&path).unwrap();
            let progress = cart.vacuum_step(10).unwrap();
            // Should be done immediately — nothing to relocate
            assert!(progress.done);
            assert_eq!(progress.pages_relocated, 0);
        }
    }

    #[test]
    fn test_shrink_capacity_allocators() {
        use crate::allocator::BlockAllocator;

        // Bitmap
        let mut bitmap = crate::allocator::bitmap::BitmapAllocator::new(1000);
        bitmap.allocate_blocks(10).unwrap(); // blocks 0-9
        bitmap.shrink_capacity(100).unwrap();
        assert_eq!(bitmap.total_blocks(), 100);
        assert_eq!(bitmap.free_blocks(), 90);

        // Cannot shrink below allocated blocks
        let mut bitmap2 = crate::allocator::bitmap::BitmapAllocator::new(100);
        bitmap2.allocate_blocks(100).unwrap();
        bitmap2.free_allocated_blocks(&[0, 1, 2, 3, 4]).unwrap();
        // Blocks 5-99 are allocated — can't shrink to 50
        assert!(bitmap2.shrink_capacity(50).is_err());

        // Extent
        let mut extent = crate::allocator::extent::ExtentAllocator::new(1000);
        extent.allocate_contiguous(10).unwrap(); // blocks 0-9
        extent.shrink_capacity(100).unwrap();
        assert_eq!(extent.total_blocks(), 100);
        assert_eq!(extent.free_blocks(), 90);

        // Hybrid
        let mut hybrid = crate::allocator::hybrid::HybridAllocator::new(1000);
        hybrid.allocate(10 * PAGE_SIZE as u64).unwrap();
        hybrid.shrink_capacity(100).unwrap();
        assert_eq!(hybrid.total_blocks(), 100);
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
