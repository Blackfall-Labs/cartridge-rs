use crate::error::{CartridgeError, Result};
use serde::{Deserialize, Serialize};
use std::mem::size_of;

pub const MAGIC: [u8; 8] = *b"CART\x00\x01\x00\x00";
pub const VERSION_MAJOR: u16 = 1;
pub const VERSION_MINOR: u16 = 0;
pub const PAGE_SIZE: usize = 4096;

/// Cartridge archive header (Page 0)
///
/// The header occupies the first 4KB page and contains critical metadata
/// for the archive format, including version info, block counts, and
/// pointers to key structures like the B-tree catalog root.
#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Header {
    /// Magic number: "CART\x00\x01\x00\x00"
    pub magic: [u8; 8],

    /// Format version (major)
    pub version_major: u16,

    /// Format version (minor)
    pub version_minor: u16,

    /// Block size in bytes (always 4096)
    pub block_size: u32,

    /// Total number of blocks in archive
    pub total_blocks: u64,

    /// Number of free blocks available
    pub free_blocks: u64,

    /// Page ID of B-tree catalog root
    pub btree_root_page: u64,

    /// Reserved space for future extensions (256 bytes)
    /// Can be used for compression config, encryption params, feature flags
    #[serde(skip, default = "default_reserved")]
    pub reserved: [u8; 256],
}

fn default_reserved() -> [u8; 256] {
    [0u8; 256]
}

/// S3 versioning mode
///
/// Controls how object versioning is handled in S3-compatible operations.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S3VersioningMode {
    /// No versioning support (default)
    None = 0,
    /// Versioning backed by Cartridge snapshots
    SnapshotBacked = 1,
}

impl S3VersioningMode {
    /// Parse versioning mode from a byte value
    ///
    /// Unknown values default to `None` for forward compatibility.
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::SnapshotBacked,
            _ => Self::None, // Default for unknown values
        }
    }
}

/// S3 ACL (Access Control List) mode
///
/// Controls how S3 ACLs are processed.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S3AclMode {
    /// Accept ACL APIs but don't store or enforce (default)
    Ignore = 0,
    /// Store ACLs in metadata but don't enforce
    Record = 1,
    /// Store ACLs and enforce via IAM policy checks
    Enforce = 2,
}

impl S3AclMode {
    /// Parse ACL mode from a byte value
    ///
    /// Unknown values default to `Ignore` for forward compatibility.
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Record,
            2 => Self::Enforce,
            _ => Self::Ignore, // Default for unknown values
        }
    }
}

/// S3 SSE (Server-Side Encryption) mode
///
/// Controls how SSE headers are handled. Note that Cartridge always
/// encrypts data with AES-256-GCM; SSE headers are cosmetic metadata.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S3SseMode {
    /// Discard SSE headers (default)
    Ignore = 0,
    /// Store SSE headers in metadata but don't return them
    Record = 1,
    /// Store SSE headers and return them in responses
    Transparent = 2,
}

impl S3SseMode {
    /// Parse SSE mode from a byte value
    ///
    /// Unknown values default to `Ignore` for forward compatibility.
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Record,
            2 => Self::Transparent,
            _ => Self::Ignore, // Default for unknown values
        }
    }
}

/// S3 feature fuses
///
/// Feature fuses are stored in the first 3 bytes of the reserved header field
/// (bytes 0-2 at offset 40-42 in the header). They control S3-specific behavior
/// while maintaining backward compatibility with v1.0 cartridges.
///
/// # Layout
///
/// ```text
/// Byte 0 (offset 40): S3VersioningMode
/// Byte 1 (offset 41): S3AclMode
/// Byte 2 (offset 42): S3SseMode
/// Bytes 3-255: Reserved for future use
/// ```
///
/// # Default Behavior
///
/// All fuses default to the most permissive/least overhead modes:
/// - Versioning: None
/// - ACL: Ignore
/// - SSE: Ignore
///
/// This ensures old cartridges (with reserved field = all zeros) work
/// correctly without modification.
#[derive(Debug, Clone, Copy)]
pub struct S3FeatureFuses {
    pub versioning_mode: S3VersioningMode,
    pub acl_mode: S3AclMode,
    pub sse_mode: S3SseMode,
}

impl S3FeatureFuses {
    /// Parse fuses from reserved field
    ///
    /// Reads the first 3 bytes of the 256-byte reserved field.
    pub fn from_reserved(reserved: &[u8; 256]) -> Self {
        Self {
            versioning_mode: S3VersioningMode::from_u8(reserved[0]),
            acl_mode: S3AclMode::from_u8(reserved[1]),
            sse_mode: S3SseMode::from_u8(reserved[2]),
        }
    }

    /// Serialize fuses to reserved field
    ///
    /// Writes fuse values to the first 3 bytes, leaving the rest as zeros.
    pub fn to_reserved(&self) -> [u8; 256] {
        let mut reserved = [0u8; 256];
        reserved[0] = self.versioning_mode as u8;
        reserved[1] = self.acl_mode as u8;
        reserved[2] = self.sse_mode as u8;
        reserved
    }
}

impl Default for S3FeatureFuses {
    fn default() -> Self {
        Self {
            versioning_mode: S3VersioningMode::None,
            acl_mode: S3AclMode::Ignore,
            sse_mode: S3SseMode::Ignore,
        }
    }
}

impl Header {
    /// Create a new header with default values
    pub fn new() -> Self {
        Header {
            magic: MAGIC,
            version_major: VERSION_MAJOR,
            version_minor: VERSION_MINOR,
            block_size: PAGE_SIZE as u32,
            total_blocks: 0,
            free_blocks: 0,
            btree_root_page: 0,
            reserved: [0; 256],
        }
    }

    /// Validate the header magic and version
    pub fn validate(&self) -> Result<()> {
        // Check magic number
        if self.magic != MAGIC {
            return Err(CartridgeError::InvalidMagic);
        }

        // Check version compatibility (exact match for now)
        if self.version_major != VERSION_MAJOR || self.version_minor != VERSION_MINOR {
            return Err(CartridgeError::UnsupportedVersion {
                major: self.version_major,
                minor: self.version_minor,
            });
        }

        // Check block size
        if self.block_size != PAGE_SIZE as u32 {
            return Err(CartridgeError::InvalidBlockSize(self.block_size));
        }

        // Sanity check: free_blocks <= total_blocks
        if self.free_blocks > self.total_blocks {
            return Err(CartridgeError::Allocation(format!(
                "Free blocks ({}) exceeds total blocks ({})",
                self.free_blocks, self.total_blocks
            )));
        }

        Ok(())
    }

    /// Get S3 feature fuses from reserved field
    ///
    /// # Examples
    ///
    /// ```
    /// use cartridge::header::Header;
    ///
    /// let header = Header::new();
    /// let fuses = header.get_s3_fuses();
    /// // Default fuses have all modes set to most permissive
    /// ```
    pub fn get_s3_fuses(&self) -> S3FeatureFuses {
        S3FeatureFuses::from_reserved(&self.reserved)
    }

    /// Set S3 feature fuses in reserved field
    ///
    /// Note: This should only be called during cartridge creation.
    /// Fuses are intended to be immutable after creation.
    ///
    /// # Examples
    ///
    /// ```
    /// use cartridge::header::{Header, S3FeatureFuses, S3VersioningMode, S3AclMode, S3SseMode};
    ///
    /// let mut header = Header::new();
    /// let fuses = S3FeatureFuses {
    ///     versioning_mode: S3VersioningMode::SnapshotBacked,
    ///     acl_mode: S3AclMode::Record,
    ///     sse_mode: S3SseMode::Transparent,
    /// };
    /// header.set_s3_fuses(fuses);
    /// ```
    pub fn set_s3_fuses(&mut self, fuses: S3FeatureFuses) {
        self.reserved = fuses.to_reserved();
    }

    /// Serialize header to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(PAGE_SIZE);

        bytes.extend_from_slice(&self.magic);
        bytes.extend_from_slice(&self.version_major.to_le_bytes());
        bytes.extend_from_slice(&self.version_minor.to_le_bytes());
        bytes.extend_from_slice(&self.block_size.to_le_bytes());
        bytes.extend_from_slice(&self.total_blocks.to_le_bytes());
        bytes.extend_from_slice(&self.free_blocks.to_le_bytes());
        bytes.extend_from_slice(&self.btree_root_page.to_le_bytes());
        bytes.extend_from_slice(&self.reserved);

        // Pad to PAGE_SIZE
        bytes.resize(PAGE_SIZE, 0);

        bytes
    }

    /// Deserialize header from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < size_of::<Header>() {
            return Err(CartridgeError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Insufficient bytes for header",
            )));
        }

        let mut header = Header::new();
        let mut offset = 0;

        header.magic.copy_from_slice(&bytes[offset..offset + 8]);
        offset += 8;

        header.version_major = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        offset += 2;

        header.version_minor = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        offset += 2;

        header.block_size = u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]);
        offset += 4;

        header.total_blocks = u64::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]);
        offset += 8;

        header.free_blocks = u64::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]);
        offset += 8;

        header.btree_root_page = u64::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]);
        offset += 8;

        header
            .reserved
            .copy_from_slice(&bytes[offset..offset + 256]);

        header.validate()?;

        Ok(header)
    }
}

impl Default for Header {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_creation() {
        let header = Header::new();
        assert_eq!(header.magic, MAGIC);
        assert_eq!(header.version_major, VERSION_MAJOR);
        assert_eq!(header.version_minor, VERSION_MINOR);
        assert_eq!(header.block_size, PAGE_SIZE as u32);
    }

    #[test]
    fn test_header_validation() {
        let header = Header::new();
        assert!(header.validate().is_ok());
    }

    #[test]
    fn test_invalid_magic() {
        let mut header = Header::new();
        header.magic = *b"INVALID!";
        assert!(matches!(
            header.validate(),
            Err(CartridgeError::InvalidMagic)
        ));
    }

    #[test]
    fn test_invalid_version() {
        let mut header = Header::new();
        header.version_major = 99;
        assert!(matches!(
            header.validate(),
            Err(CartridgeError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn test_invalid_block_size() {
        let mut header = Header::new();
        header.block_size = 8192;
        assert!(matches!(
            header.validate(),
            Err(CartridgeError::InvalidBlockSize(_))
        ));
    }

    #[test]
    fn test_header_serialization() {
        let mut header = Header::new();
        header.total_blocks = 1000;
        header.free_blocks = 500;
        header.btree_root_page = 42;

        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), PAGE_SIZE);

        let deserialized = Header::from_bytes(&bytes).unwrap();
        assert_eq!(deserialized.total_blocks, 1000);
        assert_eq!(deserialized.free_blocks, 500);
        assert_eq!(deserialized.btree_root_page, 42);
    }

    #[test]
    fn test_free_blocks_exceeds_total() {
        let mut header = Header::new();
        header.total_blocks = 100;
        header.free_blocks = 200; // Invalid!

        assert!(matches!(
            header.validate(),
            Err(CartridgeError::Allocation(_))
        ));
    }

    #[test]
    fn test_s3_fuses_default() {
        let fuses = S3FeatureFuses::default();
        assert_eq!(fuses.versioning_mode, S3VersioningMode::None);
        assert_eq!(fuses.acl_mode, S3AclMode::Ignore);
        assert_eq!(fuses.sse_mode, S3SseMode::Ignore);
    }

    #[test]
    fn test_s3_fuses_serialization_round_trip() {
        let fuses = S3FeatureFuses {
            versioning_mode: S3VersioningMode::SnapshotBacked,
            acl_mode: S3AclMode::Enforce,
            sse_mode: S3SseMode::Transparent,
        };

        let reserved = fuses.to_reserved();
        let deserialized = S3FeatureFuses::from_reserved(&reserved);

        assert_eq!(
            deserialized.versioning_mode,
            S3VersioningMode::SnapshotBacked
        );
        assert_eq!(deserialized.acl_mode, S3AclMode::Enforce);
        assert_eq!(deserialized.sse_mode, S3SseMode::Transparent);
    }

    #[test]
    fn test_s3_fuses_zeros_produce_defaults() {
        let reserved = [0u8; 256];
        let fuses = S3FeatureFuses::from_reserved(&reserved);

        assert_eq!(fuses.versioning_mode, S3VersioningMode::None);
        assert_eq!(fuses.acl_mode, S3AclMode::Ignore);
        assert_eq!(fuses.sse_mode, S3SseMode::Ignore);
    }

    #[test]
    fn test_s3_fuses_invalid_values_fallback() {
        let mut reserved = [0u8; 256];
        reserved[0] = 255; // Invalid versioning mode
        reserved[1] = 255; // Invalid ACL mode
        reserved[2] = 255; // Invalid SSE mode

        let fuses = S3FeatureFuses::from_reserved(&reserved);

        // All should fall back to defaults
        assert_eq!(fuses.versioning_mode, S3VersioningMode::None);
        assert_eq!(fuses.acl_mode, S3AclMode::Ignore);
        assert_eq!(fuses.sse_mode, S3SseMode::Ignore);
    }

    #[test]
    fn test_s3_fuses_partial_values() {
        let fuses = S3FeatureFuses {
            versioning_mode: S3VersioningMode::SnapshotBacked,
            acl_mode: S3AclMode::Record,
            sse_mode: S3SseMode::Ignore,
        };

        let reserved = fuses.to_reserved();

        // Check that only first 3 bytes are set
        assert_eq!(reserved[0], 1); // SnapshotBacked
        assert_eq!(reserved[1], 1); // Record
        assert_eq!(reserved[2], 0); // Ignore
                                    // Rest should be zeros
        for i in 3..256 {
            assert_eq!(reserved[i], 0);
        }
    }

    #[test]
    fn test_header_get_set_s3_fuses() {
        let mut header = Header::new();

        // Default fuses
        let default_fuses = header.get_s3_fuses();
        assert_eq!(default_fuses.versioning_mode, S3VersioningMode::None);

        // Set custom fuses
        let custom_fuses = S3FeatureFuses {
            versioning_mode: S3VersioningMode::SnapshotBacked,
            acl_mode: S3AclMode::Enforce,
            sse_mode: S3SseMode::Transparent,
        };
        header.set_s3_fuses(custom_fuses);

        // Retrieve and verify
        let retrieved_fuses = header.get_s3_fuses();
        assert_eq!(
            retrieved_fuses.versioning_mode,
            S3VersioningMode::SnapshotBacked
        );
        assert_eq!(retrieved_fuses.acl_mode, S3AclMode::Enforce);
        assert_eq!(retrieved_fuses.sse_mode, S3SseMode::Transparent);
    }

    #[test]
    fn test_header_fuses_persist_through_serialization() {
        let mut header = Header::new();
        header.total_blocks = 1000;
        header.free_blocks = 500;

        // Set fuses
        let fuses = S3FeatureFuses {
            versioning_mode: S3VersioningMode::SnapshotBacked,
            acl_mode: S3AclMode::Record,
            sse_mode: S3SseMode::Transparent,
        };
        header.set_s3_fuses(fuses);

        // Serialize and deserialize
        let bytes = header.to_bytes();
        let deserialized = Header::from_bytes(&bytes).unwrap();

        // Verify fuses persisted
        let retrieved_fuses = deserialized.get_s3_fuses();
        assert_eq!(
            retrieved_fuses.versioning_mode,
            S3VersioningMode::SnapshotBacked
        );
        assert_eq!(retrieved_fuses.acl_mode, S3AclMode::Record);
        assert_eq!(retrieved_fuses.sse_mode, S3SseMode::Transparent);

        // Verify other header fields also persisted
        assert_eq!(deserialized.total_blocks, 1000);
        assert_eq!(deserialized.free_blocks, 500);
    }

    #[test]
    fn test_backward_compatibility_old_cartridge() {
        // Simulate an old cartridge with reserved field = all zeros
        let mut header = Header::new();
        header.total_blocks = 1000;
        header.free_blocks = 800;
        // reserved field is already [0; 256] from Header::new()

        // Reading fuses should give defaults
        let fuses = header.get_s3_fuses();
        assert_eq!(fuses.versioning_mode, S3VersioningMode::None);
        assert_eq!(fuses.acl_mode, S3AclMode::Ignore);
        assert_eq!(fuses.sse_mode, S3SseMode::Ignore);

        // Header should still validate
        assert!(header.validate().is_ok());
    }

    #[test]
    fn test_s3_versioning_mode_from_u8() {
        assert_eq!(S3VersioningMode::from_u8(0), S3VersioningMode::None);
        assert_eq!(
            S3VersioningMode::from_u8(1),
            S3VersioningMode::SnapshotBacked
        );
        assert_eq!(S3VersioningMode::from_u8(255), S3VersioningMode::None); // Unknown falls back
    }

    #[test]
    fn test_s3_acl_mode_from_u8() {
        assert_eq!(S3AclMode::from_u8(0), S3AclMode::Ignore);
        assert_eq!(S3AclMode::from_u8(1), S3AclMode::Record);
        assert_eq!(S3AclMode::from_u8(2), S3AclMode::Enforce);
        assert_eq!(S3AclMode::from_u8(255), S3AclMode::Ignore); // Unknown falls back
    }

    #[test]
    fn test_s3_sse_mode_from_u8() {
        assert_eq!(S3SseMode::from_u8(0), S3SseMode::Ignore);
        assert_eq!(S3SseMode::from_u8(1), S3SseMode::Record);
        assert_eq!(S3SseMode::from_u8(2), S3SseMode::Transparent);
        assert_eq!(S3SseMode::from_u8(255), S3SseMode::Ignore); // Unknown falls back
    }
}
