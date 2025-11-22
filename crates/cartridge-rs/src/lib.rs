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
//! // Create a new archive with sensible defaults
//! let mut cart = Cartridge::create("data.cart")?;
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
//!     .path("data.cart")
//!     .blocks(50_000)  // ~200MB
//!     .with_audit_logging()
//!     .build()?;
//!
//! cart.write("data.txt", b"content")?;
//! # Ok(())
//! # }
//! ```

// Re-export core types that users need
pub use cartridge_core::{
    error::{CartridgeError, Result},
    catalog::{FileMetadata, FileType},
    iam::{Action, Policy, PolicyEngine, Statement, Effect},
    snapshot::{SnapshotManager, SnapshotMetadata},
    header::PAGE_SIZE,
};

use cartridge_core::Cartridge as CoreCartridge;
use std::path::Path;
use tracing::{debug, info};

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
/// let mut cart = Cartridge::create("data.cart")?;
/// cart.write("file.txt", b"content")?;
/// let data = cart.read("file.txt")?;
/// # Ok(())
/// # }
/// ```
pub struct Cartridge {
    inner: CoreCartridge,
}

impl Cartridge {
    /// Create a new Cartridge archive with default settings
    ///
    /// Default configuration:
    /// - 10,000 blocks (~40MB initial size)
    /// - No encryption
    /// - No compression
    /// - No audit logging
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use cartridge_rs::Cartridge;
    ///
    /// let mut cart = Cartridge::create("data.cart")?;
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
        info!("Creating cartridge at {:?}", path.as_ref());
        let inner = CoreCartridge::create(path, 10_000)?;
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
    /// # let mut cart = Cartridge::create("data.cart")?;
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
    /// # let cart = Cartridge::open("data.cart")?;
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
    /// # let mut cart = Cartridge::create("data.cart")?;
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
    /// # let cart = Cartridge::open("data.cart")?;
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

    /// Check if a file or directory exists
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use cartridge_rs::Cartridge;
    /// # let cart = Cartridge::open("data.cart")?;
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
    /// # let cart = Cartridge::open("data.cart")?;
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
    /// # let mut cart = Cartridge::create("data.cart")?;
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
    /// # let mut cart = Cartridge::create("data.cart")?;
    /// cart.write("file.txt", b"data")?;
    /// cart.flush()?;  // Ensure changes are persisted
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn flush(&mut self) -> Result<()> {
        debug!("Flushing cartridge to disk");
        self.inner.flush()
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
///     .path("data.cart")
///     .blocks(100_000)  // ~400MB
///     .with_audit_logging()
///     .build()?;
/// # Ok(())
/// # }
/// ```
pub struct CartridgeBuilder {
    path: Option<String>,
    blocks: usize,
    enable_audit: bool,
}

impl CartridgeBuilder {
    /// Create a new CartridgeBuilder with default settings
    pub fn new() -> Self {
        CartridgeBuilder {
            path: None,
            blocks: 10_000,  // ~40MB default
            enable_audit: false,
        }
    }

    /// Set the path for the archive
    pub fn path<P: Into<String>>(mut self, path: P) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Set the number of 4KB blocks (default: 10,000 = ~40MB)
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use cartridge_rs::CartridgeBuilder;
    ///
    /// // Create a 1GB archive
    /// let cart = CartridgeBuilder::new()
    ///     .blocks(250_000)  // 250,000 * 4KB = 1GB
    ///     .path("large.cart")
    ///     .build()?;
    /// # Ok::<(), cartridge_rs::CartridgeError>(())
    /// ```
    pub fn blocks(mut self, blocks: usize) -> Self {
        self.blocks = blocks;
        self
    }

    /// Enable audit logging for all operations
    pub fn with_audit_logging(mut self) -> Self {
        self.enable_audit = true;
        self
    }

    /// Build the Cartridge instance
    pub fn build(self) -> Result<Cartridge> {
        let path = self.path.ok_or_else(|| {
            CartridgeError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path must be set",
            ))
        })?;

        info!("Building cartridge at {} with {} blocks", path, self.blocks);

        let mut inner = CoreCartridge::create(&path, self.blocks)?;

        if self.enable_audit {
            use cartridge_core::audit::AuditLogger;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_write() -> Result<()> {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_str().unwrap();

        let mut cart = Cartridge::create(path)?;
        cart.write("test.txt", b"hello")?;
        cart.flush()?;  // Ensure write is persisted

        let content = cart.read("test.txt")?;
        assert_eq!(content, b"hello");

        Ok(())
    }

    #[test]
    fn test_builder() -> Result<()> {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_str().unwrap().to_string();

        let cart = CartridgeBuilder::new()
            .path(path)
            .blocks(5000)
            .build()?;

        assert_eq!(cart.inner().stats().total_blocks, 5000);

        Ok(())
    }
}
