//! Error types for S3 operations

use thiserror::Error;

/// S3 operation result type
pub type S3Result<T> = Result<T, S3Error>;

/// S3 operation errors
#[derive(Error, Debug)]
pub enum S3Error {
    /// Cartridge operation failed
    #[error("Cartridge error: {0}")]
    Cartridge(#[from] cartridge_core::error::CartridgeError),

    /// Bucket does not exist
    #[error("Bucket does not exist: {0}")]
    NoSuchBucket(String),

    /// Object (file) does not exist
    #[error("Object does not exist: {0}")]
    NoSuchKey(String),

    /// Bucket already exists
    #[error("Bucket already exists: {0}")]
    BucketAlreadyExists(String),

    /// Invalid bucket name
    #[error("Invalid bucket name: {0}")]
    InvalidBucketName(String),

    /// Invalid object key
    #[error("Invalid object key: {0}")]
    InvalidKey(String),

    /// Bucket is not empty (cannot delete)
    #[error("Bucket not empty: {0}")]
    BucketNotEmpty(String),

    /// Internal server error
    #[error("Internal error: {0}")]
    Internal(String),

    /// Invalid request parameter
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
