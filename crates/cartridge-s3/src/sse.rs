//! S3 SSE (Server-Side Encryption) header support
//!
//! Provides three modes:
//! - Ignore: Discard SSE headers
//! - Record: Store SSE headers in metadata but don't return them
//! - Transparent: Store and return SSE headers
//!
//! Note: Cartridge always encrypts data with AES-256-GCM internally.
//! SSE headers are cosmetic metadata for S3 API compatibility.

use serde::{Deserialize, Serialize};

/// SSE header collection
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SseHeaders {
    /// x-amz-server-side-encryption (AES256, aws:kms, etc.)
    pub algorithm: Option<String>,
    /// x-amz-server-side-encryption-customer-algorithm
    pub customer_algorithm: Option<String>,
    /// x-amz-server-side-encryption-customer-key-MD5
    pub customer_key_md5: Option<String>,
    /// x-amz-server-side-encryption-aws-kms-key-id
    pub kms_key_id: Option<String>,
}

impl SseHeaders {
    /// Create empty SSE headers
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if any SSE headers are present
    pub fn is_empty(&self) -> bool {
        self.algorithm.is_none()
            && self.customer_algorithm.is_none()
            && self.customer_key_md5.is_none()
            && self.kms_key_id.is_none()
    }

    /// Serialize to JSON for metadata storage
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from JSON metadata
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Parse from HTTP headers (stub)
    ///
    /// TODO: Full implementation would parse from hyper::HeaderMap
    pub fn from_http_headers(_headers: &[(&str, &str)]) -> Self {
        // Stub: In full implementation, would parse actual HTTP headers
        Self::new()
    }

    /// Convert to HTTP header key-value pairs
    pub fn to_http_headers(&self) -> Vec<(String, String)> {
        let mut headers = Vec::new();

        if let Some(ref alg) = self.algorithm {
            headers.push(("x-amz-server-side-encryption".to_string(), alg.clone()));
        }
        if let Some(ref alg) = self.customer_algorithm {
            headers.push((
                "x-amz-server-side-encryption-customer-algorithm".to_string(),
                alg.clone(),
            ));
        }
        if let Some(ref md5) = self.customer_key_md5 {
            headers.push((
                "x-amz-server-side-encryption-customer-key-MD5".to_string(),
                md5.clone(),
            ));
        }
        if let Some(ref key_id) = self.kms_key_id {
            headers.push((
                "x-amz-server-side-encryption-aws-kms-key-id".to_string(),
                key_id.clone(),
            ));
        }

        headers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_headers_empty() {
        let headers = SseHeaders::new();
        assert!(headers.is_empty());
    }

    #[test]
    fn test_sse_headers_serialization() {
        let mut headers = SseHeaders::new();
        headers.algorithm = Some("AES256".to_string());
        headers.kms_key_id = Some("arn:aws:kms:us-east-1:123456789012:key/abcd".to_string());

        let json = headers.to_json().unwrap();
        let deserialized = SseHeaders::from_json(&json).unwrap();

        assert_eq!(deserialized.algorithm, Some("AES256".to_string()));
        assert_eq!(
            deserialized.kms_key_id,
            Some("arn:aws:kms:us-east-1:123456789012:key/abcd".to_string())
        );
        assert!(!deserialized.is_empty());
    }

    #[test]
    fn test_sse_headers_to_http() {
        let mut headers = SseHeaders::new();
        headers.algorithm = Some("AES256".to_string());
        headers.customer_key_md5 = Some("abc123".to_string());

        let http_headers = headers.to_http_headers();
        assert_eq!(http_headers.len(), 2);
        assert!(http_headers
            .iter()
            .any(|(k, v)| k == "x-amz-server-side-encryption" && v == "AES256"));
        assert!(http_headers
            .iter()
            .any(|(k, v)| k == "x-amz-server-side-encryption-customer-key-MD5" && v == "abc123"));
    }
}
