//! # Cartridge - High-Performance Mutable Archive Format
//!
//! `cartridge-rs` provides a high-level, easy-to-use API for working with Cartridge archives.
//! Cartridge is a mutable archive format optimized for embedded systems with features like:
//!
//! - **Mutable archives** with in-place modifications
//! - **SQLite VFS integration** for running databases directly inside archives
//! - **Advanced features**: compression, encryption, snapshots, IAM policies
//! - **Engram integration**: freeze to immutable, signed archives
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use cartridge_rs::{Cartridge, Result};
//!
//! # fn main() -> Result<()> {
//! // Create a new archive - auto-grows from 12KB as needed!
//! let mut cart = Cartridge::create("my-data", "My Data Container")?;
//!
//! // Write files
//! cart.write("documents/report.txt", b"Hello, World!")?;
//!
//! // Read files
//! let content = cart.read("documents/report.txt")?;
//!
//! // List directory
//! let files = cart.list("documents")?;
//!
//! // Automatic cleanup on drop
//! # Ok(())
//! # }
//! ```
//!
//! ## Advanced Usage
//!
//! ```rust,no_run
//! use cartridge_rs::{CartridgeBuilder, Result};
//!
//! # fn main() -> Result<()> {
//! // Use builder for custom configuration
//! let mut cart = CartridgeBuilder::new()
//!     .slug("my-data")
//!     .title("My Data Container")
//!     .path("/data/my-container")  // Custom path
//!     .with_audit_logging()
//!     .build()?;
//!
//! cart.write("data.txt", b"content")?;
//! # Ok(())
//! # }
//! ```

// Core implementation (merged from cartridge-core)
pub mod core;

// Re-export core modules internally so crate:: paths in core still work
#[allow(unused_imports)]
pub(crate) use core::{
    allocator, audit, buffer_pool, catalog, compression, encryption, engram_integration, error,
    header, iam, io, manifest, page, snapshot, validation, vfs,
};

// Re-export core types that users need
pub use crate::core::{
    catalog::{FileMetadata, FileType},
    error::{CartridgeError, Result},
    header::{S3AclMode, S3FeatureFuses, S3SseMode, S3VersioningMode, PAGE_SIZE},
    iam::{Action, Effect, Policy, PolicyEngine, Statement},
    manifest::Manifest,
    snapshot::{SnapshotManager, SnapshotMetadata},
    validation::ContainerSlug,
};

use crate::core::Cartridge as CoreCartridge;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use tracing::{debug, info};

/// Rich metadata about a file or directory in the archive
///
/// Entry provides a convenient view of files and directories with parsed metadata,
/// eliminating the need for consumers to manually parse paths and build hierarchies.
///
/// # Examples
///
/// ```rust,no_run
/// use cartridge_rs::Cartridge;
///
/// # fn main() -> cartridge_rs::Result<()> {
/// let cart = Cartridge::open("data.cart")?;
/// let entries = cart.list_entries("documents")?;
///
/// for entry in entries {
///     if entry.is_dir {
///         println!("📁 {} ({})", entry.name, entry.path);
///     } else {
///         println!("📄 {} ({} bytes)", entry.name, entry.size.unwrap_or(0));
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    /// Full path in the archive (e.g., "research/notes/overview.cml")
    pub path: String,

    /// Just the name (e.g., "overview.cml" or "notes")
    pub name: String,

    /// Parent directory path (e.g., "research/notes")
    /// Empty string for root-level entries
    pub parent: String,

    /// True if this is a directory (has children under this prefix)
    pub is_dir: bool,

    /// File size in bytes (None for directories or if unavailable)
    pub size: Option<u64>,

    /// Creation timestamp as Unix epoch seconds (None if unavailable)
    pub created: Option<u64>,

    /// Last modification timestamp as Unix epoch seconds (None if unavailable)
    pub modified: Option<u64>,

    /// MIME type or content type (None if unavailable)
    pub content_type: Option<String>,

    /// File type (File, Directory, or Symlink)
    pub file_type: FileType,

    /// Compressed size on disk in bytes (None for directories or if unavailable)
    /// This is the actual space used in the container, which may be less than
    /// `size` when compression is enabled.
    pub compressed_size: Option<u64>,
}

/// Convert flat paths to Entry objects with rich metadata
///
/// This helper parses paths, infers directory structure, and fetches metadata
/// from the cartridge for each entry. Internal .cartridge/ files are filtered out.
fn paths_to_entries(cart: &CoreCartridge, paths: &[String], _prefix: &str) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut seen_dirs: HashSet<String> = HashSet::new();

    // Process each file path
    for path in paths {
        // Skip internal .cartridge directory (with or without leading slash)
        if path.starts_with(".cartridge/")
            || path == ".cartridge"
            || path.starts_with("/.cartridge")
        {
            continue;
        }
        // Extract name and parent from path
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        let parent = if let Some(idx) = path.rfind('/') {
            if idx == 0 {
                // Root level: "/file.txt" -> parent is "/"
                "/".to_string()
            } else {
                path[..idx].to_string()
            }
        } else {
            String::new()
        };

        // Fetch metadata for the file
        let metadata = cart.metadata(path).ok();

        // Create entry for the file
        entries.push(Entry {
            path: path.clone(),
            name,
            parent: parent.clone(),
            is_dir: false,
            size: metadata.as_ref().map(|m| m.size),
            created: metadata.as_ref().map(|m| m.created_at),
            modified: metadata.as_ref().map(|m| m.modified_at),
            content_type: metadata.as_ref().and_then(|m| m.content_type.clone()),
            file_type: metadata
                .as_ref()
                .map(|m| m.file_type)
                .unwrap_or(FileType::File),
            compressed_size: metadata
                .as_ref()
                .map(|m| (m.blocks.len() as u64) * PAGE_SIZE as u64),
        });

        // Add parent directories (if not already seen)
        let mut current_parent = parent.as_str();
        while !current_parent.is_empty() && current_parent != "/" {
            if seen_dirs.insert(current_parent.to_string()) {
                let parent_name = current_parent
                    .rsplit('/')
                    .next()
                    .unwrap_or(current_parent)
                    .to_string();
                let grandparent = if let Some(idx) = current_parent.rfind('/') {
                    if idx == 0 {
                        "/".to_string()
                    } else {
                        current_parent[..idx].to_string()
                    }
                } else {
                    String::new()
                };

                entries.push(Entry {
                    path: current_parent.to_string(),
                    name: parent_name,
                    parent: grandparent,
                    is_dir: true,
                    size: None,
                    created: None,
                    modified: None,
                    content_type: None,
                    file_type: FileType::Directory,
                    compressed_size: None,
                });
            }

            // Move up the tree
            if let Some(idx) = current_parent.rfind('/') {
                current_parent = &current_parent[..idx];
            } else {
                break;
            }
        }
    }

    // Sort: directories first, then alphabetically by name
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });

    Ok(entries)
}

/// High-level Cartridge archive API
///
/// This is a wrapper around `cartridge_core::Cartridge` that provides:
/// - Sensible defaults
/// - Simpler method names
/// - Automatic resource management
/// - Better error messages
///
/// # Examples
///
/// ```rust,no_run
/// use cartridge_rs::{Cartridge, Result};
///
/// # fn main() -> Result<()> {
/// let mut cart = Cartridge::create("my-data", "My Data")?;
/// cart.write("file.txt", b"content")?;
/// let data = cart.read("file.txt")?;
/// # Ok(())
/// # }
/// ```
pub struct Cartridge {
    inner: CoreCartridge,
}

impl Cartridge {
    /// Create a new Cartridge archive with auto-growth
    ///
    /// Creates a container with the given slug and title.
    /// - Starts at 12KB and grows automatically as needed
    /// - Slug is used as the filename (kebab-case, becomes `{slug}.cart`)
    /// - Title is the human-readable display name
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use cartridge_rs::Cartridge;
    ///
    /// // Creates "my-data.cart" in current directory
    /// let mut cart = Cartridge::create("my-data", "My Data Container")?;
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn create(slug: &str, title: &str) -> Result<Self> {
        info!("Creating cartridge with slug '{}', title '{}'", slug, title);
        let inner = CoreCartridge::create(slug, title)?;
        Ok(Cartridge { inner })
    }

    /// Create a new Cartridge archive at a specific path
    ///
    /// Use this when you need to specify a custom directory or path.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use cartridge_rs::Cartridge;
    ///
    /// // Creates "/data/my-container.cart"
    /// let mut cart = Cartridge::create_at("/data/my-container", "my-container", "My Container")?;
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn create_at<P: AsRef<Path>>(path: P, slug: &str, title: &str) -> Result<Self> {
        info!(
            "Creating cartridge at {:?} with slug '{}', title '{}'",
            path.as_ref(),
            slug,
            title
        );
        let inner = CoreCartridge::create_at(path, slug, title)?;
        Ok(Cartridge { inner })
    }

    /// Open an existing Cartridge archive
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use cartridge_rs::Cartridge;
    ///
    /// let mut cart = Cartridge::open("existing.cart")?;
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        info!("Opening cartridge at {:?}", path.as_ref());
        let inner = CoreCartridge::open(path)?;
        Ok(Cartridge { inner })
    }

    /// Write data to a file in the archive
    ///
    /// Creates the file if it doesn't exist, updates it if it does.
    /// Automatically creates parent directories.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let mut cart = Cartridge::create("my-data", "My Data")?;
    /// cart.write("documents/report.txt", b"Hello, World!")?;
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn write<P: AsRef<str>>(&mut self, path: P, content: &[u8]) -> Result<()> {
        let path = path.as_ref();
        debug!("Writing {} bytes to {}", content.len(), path);

        // Check if file exists, create or update accordingly
        if self.inner.exists(path)? {
            self.inner.write_file(path, content)
        } else {
            self.inner.create_file(path, content)
        }
    }

    /// Read data from a file in the archive
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let cart = Cartridge::create("my-data", "My Data")?;
    /// let content = cart.read("documents/report.txt")?;
    /// println!("Content: {}", String::from_utf8_lossy(&content));
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn read<P: AsRef<str>>(&self, path: P) -> Result<Vec<u8>> {
        let path = path.as_ref();
        debug!("Reading {}", path);
        self.inner.read_file(path)
    }

    /// Delete a file from the archive
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let mut cart = Cartridge::create("my-data", "My Data")?;
    /// cart.delete("old_file.txt")?;
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn delete<P: AsRef<str>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref();
        debug!("Deleting {}", path);
        self.inner.delete_file(path)
    }

    /// List all entries in a directory
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let cart = Cartridge::create("my-data", "My Data")?;
    /// let files = cart.list("documents")?;
    /// for file in files {
    ///     println!("Found: {}", file);
    /// }
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn list<P: AsRef<str>>(&self, path: P) -> Result<Vec<String>> {
        let path = path.as_ref();
        debug!("Listing directory {}", path);
        self.inner.list_dir(path)
    }

    /// List all entries with rich metadata under a given prefix
    ///
    /// Returns Entry objects with parsed path components, file metadata,
    /// and inferred directory information. This eliminates the need to
    /// manually parse paths and build hierarchies.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let cart = Cartridge::create("my-data", "My Data")?;
    /// let entries = cart.list_entries("documents")?;
    /// for entry in entries {
    ///     if entry.is_dir {
    ///         println!("📁 {}", entry.name);
    ///     } else {
    ///         println!("📄 {} ({} bytes)", entry.name, entry.size.unwrap_or(0));
    ///     }
    /// }
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn list_entries<P: AsRef<str>>(&self, prefix: P) -> Result<Vec<Entry>> {
        let prefix = prefix.as_ref();
        debug!("Listing entries under prefix {}", prefix);
        let paths = self.inner.list_dir(prefix)?;
        paths_to_entries(&self.inner, &paths, prefix)
    }

    /// List immediate children of a directory
    ///
    /// Like `list_entries()` but filters to only direct children,
    /// providing a traditional directory-style listing.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let cart = Cartridge::create("my-data", "My Data")?;
    /// // List only immediate children of "documents"
    /// let children = cart.list_children("documents")?;
    /// for child in children {
    ///     println!("{} - parent: {}", child.name, child.parent);
    /// }
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn list_children<P: AsRef<str>>(&self, parent: P) -> Result<Vec<Entry>> {
        let parent = parent.as_ref();
        debug!("Listing immediate children of {}", parent);
        let all_entries = self.list_entries(parent)?;

        // Filter to immediate children only
        Ok(all_entries
            .into_iter()
            .filter(|e| e.parent == parent)
            .collect())
    }

    /// Check if a path is a directory
    ///
    /// Returns true if the path has children (i.e., is a directory),
    /// false otherwise.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let cart = Cartridge::create("my-data", "My Data")?;
    /// if cart.is_dir("documents")? {
    ///     println!("documents is a directory");
    /// }
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn is_dir<P: AsRef<str>>(&self, path: P) -> Result<bool> {
        let path = path.as_ref();
        debug!("Checking if {} is a directory", path);

        // A path is a directory if it has children
        let prefix = if path.is_empty() {
            String::new()
        } else {
            format!("{}/", path)
        };

        let paths = self.inner.list_dir(&prefix)?;
        Ok(!paths.is_empty())
    }

    /// Check if a file or directory exists
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let cart = Cartridge::create("my-data", "My Data")?;
    /// if cart.exists("config.json")? {
    ///     println!("Config file found!");
    /// }
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn exists<P: AsRef<str>>(&self, path: P) -> Result<bool> {
        self.inner.exists(path.as_ref())
    }

    /// Get metadata for a file or directory
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let cart = Cartridge::create("my-data", "My Data")?;
    /// let meta = cart.metadata("file.txt")?;
    /// println!("Size: {} bytes", meta.size);
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn metadata<P: AsRef<str>>(&self, path: P) -> Result<FileMetadata> {
        self.inner.metadata(path.as_ref())
    }

    /// Create a directory
    ///
    /// Automatically creates parent directories if needed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let mut cart = Cartridge::create("my-data", "My Data")?;
    /// cart.create_dir("documents/reports/2025")?;
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn create_dir<P: AsRef<str>>(&mut self, path: P) -> Result<()> {
        self.inner.create_dir(path.as_ref())
    }

    /// Flush all pending changes to disk
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let mut cart = Cartridge::create("my-data", "My Data")?;
    /// cart.write("file.txt", b"data")?;
    /// cart.flush()?;  // Ensure changes are persisted
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn flush(&mut self) -> Result<()> {
        debug!("Flushing cartridge to disk");
        self.inner.flush()
    }

    /// Get the container slug
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let cart = Cartridge::create("my-data", "My Data")?;
    /// assert_eq!(cart.slug()?, "my-data");
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn slug(&self) -> Result<String> {
        self.inner.slug()
    }

    /// Get the container title
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let cart = Cartridge::create("my-data", "My Container")?;
    /// assert_eq!(cart.title()?, "My Container");
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn title(&self) -> Result<String> {
        self.inner.title()
    }

    /// Read the container manifest
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let cart = Cartridge::create("my-data", "My Container")?;
    /// let manifest = cart.read_manifest()?;
    /// println!("Version: {}", manifest.version);
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn read_manifest(&self) -> Result<Manifest> {
        self.inner.read_manifest()
    }

    /// Update the container manifest
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let mut cart = Cartridge::create("my-data", "My Container")?;
    /// cart.update_manifest(|manifest| {
    ///     manifest.description = Some("Updated description".to_string());
    /// })?;
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn update_manifest<F>(&mut self, f: F) -> Result<()>
    where
        F: FnOnce(&mut Manifest),
    {
        self.inner.update_manifest(f)
    }

    /// Get access to the underlying core Cartridge for advanced operations
    ///
    /// Use this when you need features not exposed by the high-level API:
    /// - IAM policies
    /// - Snapshots
    /// - Audit logging
    /// - Custom allocator settings
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use cartridge_rs::{Cartridge, Policy};
    ///
    /// let mut cart = Cartridge::create("data.cart")?;
    ///
    /// // Access advanced features
    /// let policy = Policy::new("my-policy", vec![]);
    /// cart.inner_mut().set_policy(policy);
    /// ```
    pub fn inner(&self) -> &CoreCartridge {
        &self.inner
    }

    /// Get mutable access to the underlying core Cartridge
    pub fn inner_mut(&mut self) -> &mut CoreCartridge {
        &mut self.inner
    }
}

/// Builder for customizing Cartridge creation
///
/// Provides a fluent API for configuring advanced options.
///
/// # Examples
///
/// ```rust,no_run
/// use cartridge_rs::CartridgeBuilder;
///
/// # fn main() -> cartridge_rs::Result<()> {
/// let cart = CartridgeBuilder::new()
///     .slug("my-data")
///     .title("My Data Container")
///     .path("/data/my-container")  // Optional: custom path
///     .with_audit_logging()
///     .build()?;
/// # Ok(())
/// # }
/// ```
pub struct CartridgeBuilder {
    path: Option<String>,
    slug: Option<String>,
    title: Option<String>,
    enable_audit: bool,
}

impl CartridgeBuilder {
    /// Create a new CartridgeBuilder with default settings
    pub fn new() -> Self {
        CartridgeBuilder {
            path: None,
            slug: None,
            title: None,
            enable_audit: false,
        }
    }

    /// Set the slug (kebab-case identifier)
    pub fn slug<S: Into<String>>(mut self, slug: S) -> Self {
        self.slug = Some(slug.into());
        self
    }

    /// Set the title (human-readable display name)
    pub fn title<S: Into<String>>(mut self, title: S) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set a custom path (optional, defaults to slug in current directory)
    pub fn path<P: Into<String>>(mut self, path: P) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Enable audit logging for all operations
    pub fn with_audit_logging(mut self) -> Self {
        self.enable_audit = true;
        self
    }

    /// Build the Cartridge instance
    pub fn build(self) -> Result<Cartridge> {
        let slug = self.slug.ok_or_else(|| {
            CartridgeError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "slug must be set",
            ))
        })?;

        let title = self.title.ok_or_else(|| {
            CartridgeError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "title must be set",
            ))
        })?;

        info!("Building cartridge with slug '{}', title '{}'", slug, title);

        let mut inner = if let Some(path) = self.path {
            CoreCartridge::create_at(&path, &slug, &title)?
        } else {
            CoreCartridge::create(&slug, &title)?
        };

        if self.enable_audit {
            use crate::core::audit::AuditLogger;
            use std::sync::Arc;
            use std::time::Duration;

            let logger = Arc::new(AuditLogger::new(1000, Duration::from_secs(60)));
            inner.set_audit_logger(logger);
            debug!("Audit logging enabled");
        }

        Ok(Cartridge { inner })
    }
}

impl Default for CartridgeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Virtual Filesystem trait for unified storage interface
///
/// Provides a common interface that can be implemented by different storage backends:
/// - Cartridge (mutable containers)
/// - Engram (immutable archives)
/// - ZipVfs, TarVfs (other archive formats)
/// - S3Vfs, LocalVfs (remote/local filesystems)
///
/// This allows applications to work with any storage backend using the same API.
///
/// # Examples
///
/// ```rust,no_run
/// use cartridge_rs::{Cartridge, Vfs};
///
/// fn process_storage<V: Vfs>(vfs: &V, path: &str) -> Result<(), Box<dyn std::error::Error>> {
///     // Works with any VFS implementation
///     let entries = vfs.list_entries(path)?;
///     for entry in entries {
///         if !entry.is_dir {
///             let content = vfs.read(&entry.path)?;
///             println!("File: {} ({} bytes)", entry.name, content.len());
///         }
///     }
///     Ok(())
/// }
///
/// // Use with Cartridge
/// let cart = Cartridge::create("my-data", "My Data")?;
/// process_storage(&cart, "documents")?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub trait Vfs {
    /// List all entries under a given prefix with rich metadata
    ///
    /// Returns Entry objects with parsed path components, file metadata,
    /// and inferred directory information.
    fn list_entries(&self, prefix: &str) -> Result<Vec<Entry>>;

    /// List immediate children of a directory (non-recursive)
    ///
    /// Returns only entries whose parent matches the given path,
    /// providing a directory-style listing view.
    fn list_children(&self, parent: &str) -> Result<Vec<Entry>>;

    /// Read the contents of a file
    ///
    /// Returns the full file contents as a byte vector.
    fn read(&self, path: &str) -> Result<Vec<u8>>;

    /// Write or update a file
    ///
    /// Creates the file if it doesn't exist, updates it if it does.
    /// Automatically creates parent directories as needed.
    fn write(&mut self, path: &str, data: &[u8]) -> Result<()>;

    /// Delete a file or directory
    ///
    /// For directories, this typically deletes all contents recursively.
    fn delete(&mut self, path: &str) -> Result<()>;

    /// Check if a path exists
    fn exists(&self, path: &str) -> Result<bool>;

    /// Check if a path is a directory
    fn is_dir(&self, path: &str) -> Result<bool>;

    /// Get metadata for a path
    fn metadata(&self, path: &str) -> Result<FileMetadata>;
}

/// Implement VFS trait for Cartridge
impl Vfs for Cartridge {
    fn list_entries(&self, prefix: &str) -> Result<Vec<Entry>> {
        self.list_entries(prefix)
    }

    fn list_children(&self, parent: &str) -> Result<Vec<Entry>> {
        self.list_children(parent)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>> {
        self.read(path)
    }

    fn write(&mut self, path: &str, data: &[u8]) -> Result<()> {
        self.write(path, data)
    }

    fn delete(&mut self, path: &str) -> Result<()> {
        self.delete(path)
    }

    fn exists(&self, path: &str) -> Result<bool> {
        self.exists(path)
    }

    fn is_dir(&self, path: &str) -> Result<bool> {
        self.is_dir(path)
    }

    fn metadata(&self, path: &str) -> Result<FileMetadata> {
        self.metadata(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_write() -> Result<()> {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("test-cart");

        let mut cart = Cartridge::create_at(&path, "test-cart", "Test Cartridge")?;
        cart.write("test.txt", b"hello")?;
        cart.flush()?; // Ensure write is persisted

        let content = cart.read("test.txt")?;
        assert_eq!(content, b"hello");

        Ok(())
    }

    #[test]
    fn test_builder() -> Result<()> {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("builder-cart");

        let cart = CartridgeBuilder::new()
            .slug("builder-cart")
            .title("Builder Cartridge")
            .path(path.to_str().unwrap())
            .build()?;

        // Starts small with auto-growth
        assert!(cart.inner().stats().total_blocks >= 3);

        Ok(())
    }

    #[test]
    fn test_list_entries_flat_structure() -> Result<()> {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("flat-cart");

        let mut cart = Cartridge::create_at(&path, "flat-cart", "Flat Cartridge")?;

        // Create flat structure (no nesting)
        cart.write("/file1.txt", b"content1")?;
        cart.write("/file2.txt", b"content2")?;
        cart.write("/file3.txt", b"content3")?;
        cart.flush()?;

        let entries = cart.list_entries("/")?;

        // Should have 3 files
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().all(|e| !e.is_dir));
        assert!(entries.iter().all(|e| e.parent == "/"));

        Ok(())
    }

    #[test]
    fn test_list_entries_nested_structure() -> Result<()> {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("nested-cart");

        let mut cart = Cartridge::create_at(&path, "nested-cart", "Nested Cartridge")?;

        // Create nested structure (3+ levels)
        cart.write("/docs/guides/getting-started.md", b"# Getting Started")?;
        cart.write("/docs/guides/advanced.md", b"# Advanced")?;
        cart.write("/docs/api/reference.md", b"# API Reference")?;
        cart.write("/src/main.rs", b"fn main() {}")?;
        cart.flush()?;

        let entries = cart.list_entries("/")?;

        // Should have: /docs, /docs/guides, /docs/api, /src (4 dirs) + 4 files = 8 entries
        assert_eq!(entries.len(), 8);

        // Count directories and files
        let dirs: Vec<_> = entries.iter().filter(|e| e.is_dir).collect();
        let files: Vec<_> = entries.iter().filter(|e| !e.is_dir).collect();

        assert_eq!(dirs.len(), 4);
        assert_eq!(files.len(), 4);

        // Verify directory names
        let dir_names: Vec<_> = dirs.iter().map(|e| e.name.as_str()).collect();
        assert!(dir_names.contains(&"docs"));
        assert!(dir_names.contains(&"guides"));
        assert!(dir_names.contains(&"api"));
        assert!(dir_names.contains(&"src"));

        Ok(())
    }

    #[test]
    fn test_list_entries_empty() -> Result<()> {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("empty-cart");

        let cart = Cartridge::create_at(&path, "empty-cart", "Empty Cartridge")?;

        let entries = cart.list_entries("/")?;

        // Empty cartridge should return empty list (excluding internal .cartridge/)
        assert_eq!(entries.len(), 0);

        Ok(())
    }

    #[test]
    fn test_list_children_root_level() -> Result<()> {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("children-cart");

        let mut cart = Cartridge::create_at(&path, "children-cart", "Children Cartridge")?;

        // Create structure with nested files
        cart.write("/root1.txt", b"root file 1")?;
        cart.write("/root2.txt", b"root file 2")?;
        cart.write("/docs/nested.md", b"nested file")?;
        cart.write("/docs/deep/very-nested.md", b"very nested")?;
        cart.flush()?;

        let children = cart.list_children("/")?;

        // Should only have root-level entries: root1.txt, root2.txt, docs/
        assert_eq!(children.len(), 3);
        assert!(children.iter().all(|e| e.parent == "/"));

        // Count root files and directories
        let files: Vec<_> = children.iter().filter(|e| !e.is_dir).collect();
        let dirs: Vec<_> = children.iter().filter(|e| e.is_dir).collect();

        assert_eq!(files.len(), 2);
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].name, "docs");

        Ok(())
    }

    #[test]
    fn test_list_children_nested_directory() -> Result<()> {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("nested-children-cart");

        let mut cart = Cartridge::create_at(&path, "nested-children-cart", "Nested Children")?;

        cart.write("/docs/readme.md", b"readme")?;
        cart.write("/docs/guides/tutorial.md", b"tutorial")?;
        cart.write("/docs/api/reference.md", b"reference")?;
        cart.flush()?;

        let children = cart.list_children("/docs")?;

        // Should have: readme.md, guides/, api/ (3 immediate children)
        assert_eq!(children.len(), 3);
        assert!(children.iter().all(|e| e.parent == "/docs"));

        Ok(())
    }

    #[test]
    fn test_list_children_only_subdirectories() -> Result<()> {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("subdirs-cart");

        let mut cart = Cartridge::create_at(&path, "subdirs-cart", "Subdirs")?;

        // Create directories with no files at this level
        cart.write("/parent/child1/file.txt", b"file1")?;
        cart.write("/parent/child2/file.txt", b"file2")?;
        cart.flush()?;

        let children = cart.list_children("/parent")?;

        // Should only have child1/ and child2/ directories
        assert_eq!(children.len(), 2);
        assert!(children.iter().all(|e| e.is_dir));
        assert!(children.iter().all(|e| e.parent == "/parent"));

        Ok(())
    }

    #[test]
    fn test_list_children_only_files() -> Result<()> {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("files-cart");

        let mut cart = Cartridge::create_at(&path, "files-cart", "Files")?;

        // Create directory with only files
        cart.write("/data/file1.dat", b"data1")?;
        cart.write("/data/file2.dat", b"data2")?;
        cart.write("/data/file3.dat", b"data3")?;
        cart.flush()?;

        let children = cart.list_children("/data")?;

        // Should only have 3 files
        assert_eq!(children.len(), 3);
        assert!(children.iter().all(|e| !e.is_dir));
        assert!(children.iter().all(|e| e.parent == "/data"));

        Ok(())
    }

    #[test]
    fn test_is_dir_known_directory() -> Result<()> {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("isdir-cart");

        let mut cart = Cartridge::create_at(&path, "isdir-cart", "IsDir")?;

        cart.write("/documents/report.txt", b"report")?;
        cart.flush()?;

        // "/documents" should be a directory
        assert!(cart.is_dir("/documents")?);

        Ok(())
    }

    #[test]
    fn test_is_dir_known_file() -> Result<()> {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("isfile-cart");

        let mut cart = Cartridge::create_at(&path, "isfile-cart", "IsFile")?;

        cart.write("/documents/report.txt", b"report")?;
        cart.flush()?;

        // "/documents/report.txt" should NOT be a directory
        assert!(!cart.is_dir("/documents/report.txt")?);

        Ok(())
    }

    #[test]
    fn test_is_dir_nonexistent() -> Result<()> {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("nonexistent-cart");

        let cart = Cartridge::create_at(&path, "nonexistent-cart", "NonExistent")?;

        // Non-existent path should not be a directory
        assert!(!cart.is_dir("/does-not-exist")?);

        Ok(())
    }

    #[test]
    fn test_entry_metadata_fields() -> Result<()> {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path = temp_dir.path().join("metadata-cart");

        let mut cart = Cartridge::create_at(&path, "metadata-cart", "Metadata")?;

        cart.write("/test.txt", b"hello world")?;
        cart.flush()?;

        let entries = cart.list_entries("/")?;

        assert_eq!(entries.len(), 1);
        let entry = &entries[0];

        // Verify basic fields
        assert_eq!(entry.path, "/test.txt");
        assert_eq!(entry.name, "test.txt");
        assert_eq!(entry.parent, "/");
        assert!(!entry.is_dir);

        // Verify metadata fields are populated
        assert!(entry.size.is_some());
        assert_eq!(entry.size.unwrap(), 11); // "hello world" is 11 bytes
        assert!(entry.created.is_some());
        assert!(entry.modified.is_some());

        Ok(())
    }
}
