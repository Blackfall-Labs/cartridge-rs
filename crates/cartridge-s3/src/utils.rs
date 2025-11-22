//! Utility functions for S3 operations

use crate::error::{S3Error, S3Result};
use sha2::{Digest, Sha256};

/// Validate S3 bucket name according to AWS rules
///
/// Rules:
/// - 3-63 characters
/// - Lowercase letters, numbers, dots, hyphens
/// - Must start and end with letter or number
/// - No consecutive dots
pub fn validate_bucket_name(name: &str) -> S3Result<()> {
    if name.len() < 3 || name.len() > 63 {
        return Err(S3Error::InvalidBucketName(format!(
            "Bucket name must be 3-63 characters, got {}",
            name.len()
        )));
    }

    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
    {
        return Err(S3Error::InvalidBucketName(
            "Bucket name must contain only lowercase letters, numbers, dots, and hyphens"
                .to_string(),
        ));
    }

    let first = name.chars().next().unwrap();
    let last = name.chars().last().unwrap();
    if !(first.is_ascii_alphanumeric() && last.is_ascii_alphanumeric()) {
        return Err(S3Error::InvalidBucketName(
            "Bucket name must start and end with letter or number".to_string(),
        ));
    }

    if name.contains("..") {
        return Err(S3Error::InvalidBucketName(
            "Bucket name cannot contain consecutive dots".to_string(),
        ));
    }

    Ok(())
}

/// Validate S3 object key
///
/// Keys can be any UTF-8 string up to 1024 bytes
pub fn validate_key(key: &str) -> S3Result<()> {
    if key.is_empty() {
        return Err(S3Error::InvalidKey("Key cannot be empty".to_string()));
    }

    if key.len() > 1024 {
        return Err(S3Error::InvalidKey(format!(
            "Key too long: {} bytes (max 1024)",
            key.len()
        )));
    }

    Ok(())
}

/// Convert bucket name to Cartridge path
///
/// Example: "mybucket" → "/mybucket"
pub fn bucket_to_path(bucket: &str) -> String {
    format!("/{}", bucket)
}

/// Convert S3 key to Cartridge path
///
/// Example: "mybucket", "file.txt" → "/mybucket/file.txt"
pub fn key_to_path(bucket: &str, key: &str) -> String {
    format!("/{}/{}", bucket, key)
}

/// Parse Cartridge path into bucket and key
///
/// Example: "/mybucket/file.txt" → ("mybucket", "file.txt")
pub fn path_to_bucket_key(path: &str) -> Option<(String, String)> {
    let path = path.strip_prefix('/')?;
    let parts: Vec<&str> = path.splitn(2, '/').collect();

    if parts.len() == 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

/// Generate ETag from content hash
///
/// S3 ETags are typically MD5 hashes, but we use SHA-256 for consistency
/// with Cartridge's existing hashing. Format as hex string.
pub fn generate_etag(content_hash: &[u8; 32]) -> String {
    hex::encode(content_hash)
}

/// Compute SHA-256 hash of data
pub fn compute_hash(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_bucket_name() {
        // Valid names
        assert!(validate_bucket_name("mybucket").is_ok());
        assert!(validate_bucket_name("my-bucket").is_ok());
        assert!(validate_bucket_name("my.bucket").is_ok());
        assert!(validate_bucket_name("bucket123").is_ok());

        // Invalid names
        assert!(validate_bucket_name("ab").is_err()); // Too short
        assert!(validate_bucket_name(&"a".repeat(64)).is_err()); // Too long
        assert!(validate_bucket_name("MyBucket").is_err()); // Uppercase
        assert!(validate_bucket_name("-bucket").is_err()); // Starts with dash
        assert!(validate_bucket_name("bucket-").is_err()); // Ends with dash
        assert!(validate_bucket_name("my..bucket").is_err()); // Consecutive dots
    }

    #[test]
    fn test_validate_key() {
        assert!(validate_key("file.txt").is_ok());
        assert!(validate_key("dir/file.txt").is_ok());
        assert!(validate_key("").is_err()); // Empty
        assert!(validate_key(&"a".repeat(1025)).is_err()); // Too long
    }

    #[test]
    fn test_path_conversion() {
        assert_eq!(bucket_to_path("mybucket"), "/mybucket");
        assert_eq!(key_to_path("mybucket", "file.txt"), "/mybucket/file.txt");

        let (bucket, key) = path_to_bucket_key("/mybucket/file.txt").unwrap();
        assert_eq!(bucket, "mybucket");
        assert_eq!(key, "file.txt");

        let (bucket, key) = path_to_bucket_key("/mybucket/dir/file.txt").unwrap();
        assert_eq!(bucket, "mybucket");
        assert_eq!(key, "dir/file.txt");

        assert!(path_to_bucket_key("/mybucket").is_none());
    }

    #[test]
    fn test_etag_generation() {
        let hash = [0x42u8; 32];
        let etag = generate_etag(&hash);
        assert_eq!(etag.len(), 64); // 32 bytes * 2 hex chars
        assert!(etag.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_compute_hash() {
        let data = b"Hello, World!";
        let hash = compute_hash(data);
        assert_eq!(hash.len(), 32);

        // Same data produces same hash
        let hash2 = compute_hash(data);
        assert_eq!(hash, hash2);

        // Different data produces different hash
        let hash3 = compute_hash(b"Different data");
        assert_ne!(hash, hash3);
    }
}
