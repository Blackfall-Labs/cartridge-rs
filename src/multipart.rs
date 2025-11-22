//! Multipart upload management for S3
//!
//! Handles state for in-progress multipart uploads including
//! upload IDs, parts, and assembly of completed uploads.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Represents a single uploaded part
#[derive(Debug, Clone)]
pub struct UploadedPart {
    pub part_number: i32,
    pub data: Vec<u8>,
    pub etag: String,
}

/// Represents an in-progress multipart upload
#[derive(Debug, Clone)]
pub struct MultipartUpload {
    pub upload_id: String,
    pub bucket: String,
    pub key: String,
    pub parts: HashMap<i32, UploadedPart>,
}

impl MultipartUpload {
    pub fn new(bucket: String, key: String) -> Self {
        Self {
            upload_id: Uuid::new_v4().to_string(),
            bucket,
            key,
            parts: HashMap::new(),
        }
    }

    pub fn add_part(&mut self, part_number: i32, data: Vec<u8>, etag: String) {
        self.parts.insert(
            part_number,
            UploadedPart {
                part_number,
                data,
                etag,
            },
        );
    }

    pub fn get_part(&self, part_number: i32) -> Option<&UploadedPart> {
        self.parts.get(&part_number)
    }

    pub fn list_parts(&self) -> Vec<&UploadedPart> {
        let mut parts: Vec<&UploadedPart> = self.parts.values().collect();
        parts.sort_by_key(|p| p.part_number);
        parts
    }

    /// Assemble all parts into final object data
    pub fn assemble_parts(&self) -> Option<Vec<u8>> {
        if self.parts.is_empty() {
            return None;
        }

        // Get sorted part numbers directly from the HashMap
        let mut part_numbers: Vec<i32> = self.parts.keys().copied().collect();
        part_numbers.sort();

        tracing::debug!(
            "Assembling {} parts: {:?}",
            part_numbers.len(),
            part_numbers
        );

        // Verify parts are sequential starting from 1
        for (idx, &part_num) in part_numbers.iter().enumerate() {
            if part_num != (idx as i32 + 1) {
                tracing::error!(
                    "Missing part at index {}, expected {}, got {}",
                    idx,
                    idx + 1,
                    part_num
                );
                return None; // Missing part
            }
        }

        // Concatenate all part data in order
        let mut assembled = Vec::new();
        for &part_num in &part_numbers {
            if let Some(part) = self.parts.get(&part_num) {
                tracing::debug!("Adding part {} with {} bytes", part_num, part.data.len());
                assembled.extend_from_slice(&part.data);
            }
        }

        tracing::debug!(
            "Assembled total {} bytes from {} parts",
            assembled.len(),
            part_numbers.len()
        );
        Some(assembled)
    }
}

/// Manager for all multipart uploads
#[derive(Debug, Clone)]
pub struct MultipartManager {
    uploads: Arc<Mutex<HashMap<String, MultipartUpload>>>,
}

impl MultipartManager {
    pub fn new() -> Self {
        Self {
            uploads: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn create_upload(&self, bucket: String, key: String) -> String {
        let upload = MultipartUpload::new(bucket, key);
        let upload_id = upload.upload_id.clone();

        let mut uploads = self.uploads.lock();
        uploads.insert(upload_id.clone(), upload);

        upload_id
    }

    pub fn get_upload(&self, upload_id: &str) -> Option<MultipartUpload> {
        let uploads = self.uploads.lock();
        uploads.get(upload_id).cloned()
    }

    pub fn upload_part(
        &self,
        upload_id: &str,
        part_number: i32,
        data: Vec<u8>,
        etag: String,
    ) -> Result<(), String> {
        tracing::debug!(
            "Uploading part {} for upload {}, size: {} bytes",
            part_number,
            upload_id,
            data.len()
        );
        let mut uploads = self.uploads.lock();

        if let Some(upload) = uploads.get_mut(upload_id) {
            upload.add_part(part_number, data, etag);
            Ok(())
        } else {
            Err(format!("Upload ID not found: {}", upload_id))
        }
    }

    pub fn complete_upload(&self, upload_id: &str) -> Option<(String, String, Vec<u8>)> {
        let mut uploads = self.uploads.lock();

        if let Some(upload) = uploads.remove(upload_id) {
            upload
                .assemble_parts()
                .map(|data| (upload.bucket, upload.key, data))
        } else {
            None
        }
    }

    pub fn abort_upload(&self, upload_id: &str) -> bool {
        let mut uploads = self.uploads.lock();
        uploads.remove(upload_id).is_some()
    }

    pub fn list_parts(&self, upload_id: &str) -> Option<Vec<UploadedPart>> {
        let uploads = self.uploads.lock();
        uploads
            .get(upload_id)
            .map(|upload| upload.list_parts().into_iter().cloned().collect())
    }
}

impl Default for MultipartManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multipart_upload_lifecycle() {
        let manager = MultipartManager::new();

        // Create upload
        let upload_id = manager.create_upload("mybucket".to_string(), "mykey".to_string());
        assert!(!upload_id.is_empty());

        // Upload parts
        manager
            .upload_part(&upload_id, 1, b"part1".to_vec(), "etag1".to_string())
            .unwrap();
        manager
            .upload_part(&upload_id, 2, b"part2".to_vec(), "etag2".to_string())
            .unwrap();

        // List parts
        let parts = manager.list_parts(&upload_id).unwrap();
        assert_eq!(parts.len(), 2);

        // Complete upload
        let result = manager.complete_upload(&upload_id).unwrap();
        assert_eq!(result.0, "mybucket");
        assert_eq!(result.1, "mykey");
        assert_eq!(result.2, b"part1part2");

        // Upload should be removed after completion
        assert!(manager.get_upload(&upload_id).is_none());
    }

    #[test]
    fn test_abort_upload() {
        let manager = MultipartManager::new();

        let upload_id = manager.create_upload("bucket".to_string(), "key".to_string());
        manager
            .upload_part(&upload_id, 1, b"data".to_vec(), "etag".to_string())
            .unwrap();

        assert!(manager.abort_upload(&upload_id));
        assert!(manager.get_upload(&upload_id).is_none());
    }

    #[test]
    fn test_missing_parts() {
        let manager = MultipartManager::new();

        let upload_id = manager.create_upload("bucket".to_string(), "key".to_string());

        // Upload parts 1 and 3 (skipping 2)
        manager
            .upload_part(&upload_id, 1, b"part1".to_vec(), "etag1".to_string())
            .unwrap();
        manager
            .upload_part(&upload_id, 3, b"part3".to_vec(), "etag3".to_string())
            .unwrap();

        // Should fail due to missing part 2
        assert!(manager.complete_upload(&upload_id).is_none());
    }
}
