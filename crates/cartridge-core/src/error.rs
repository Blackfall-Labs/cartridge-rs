use thiserror::Error;

#[derive(Error, Debug)]
pub enum CartridgeError {
    #[error("Invalid magic number in header")]
    InvalidMagic,

    #[error("Unsupported format version: {major}.{minor}")]
    UnsupportedVersion { major: u16, minor: u16 },

    #[error("Invalid block size: {0}")]
    InvalidBlockSize(u32),

    #[error("Invalid page type: {0}")]
    InvalidPageType(u8),

    #[error("Page checksum verification failed")]
    ChecksumMismatch,

    #[error("Out of space: no free blocks available")]
    OutOfSpace,

    #[error("Invalid block ID: {0}")]
    InvalidBlockId(u64),

    #[error("Block already allocated: {0}")]
    BlockAlreadyAllocated(u64),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("VFS registration failed: {0}")]
    VFSRegistrationFailed(i32),

    #[error("Allocation error: {0}")]
    Allocation(String),

    #[error("Fragmentation score calculation failed")]
    FragmentationError,
}

pub type Result<T> = std::result::Result<T, CartridgeError>;
