//! S3 backend implementation for Cartridge

use crate::error::{S3Error, S3Result};
use crate::multipart::MultipartManager;
use crate::utils::{
    bucket_to_path, compute_hash, generate_etag, key_to_path, validate_bucket_name, validate_key,
};
use cartridge_core::header::S3FeatureFuses;
use cartridge_core::Cartridge;
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::{debug, info};

/// S3 backend powered by Cartridge storage
pub struct CartridgeS3Backend {
    cartridge: Arc<RwLock<Cartridge>>,
    multipart: MultipartManager,
    s3_fuses: S3FeatureFuses,
}

impl CartridgeS3Backend {
    pub fn new(cartridge: Arc<RwLock<Cartridge>>) -> Self {
        info!("Initializing Cartridge S3 backend");

        let s3_fuses = {
            let cart = cartridge.read();
            cart.header().get_s3_fuses()
        };

        info!(
            "S3 fuses: versioning={:?}, acl={:?}, sse={:?}",
            s3_fuses.versioning_mode, s3_fuses.acl_mode, s3_fuses.sse_mode
        );

        CartridgeS3Backend {
            cartridge,
            multipart: MultipartManager::new(),
            s3_fuses,
        }
    }

    /// Get a reference to the S3 feature fuses
    pub fn s3_fuses(&self) -> &S3FeatureFuses {
        &self.s3_fuses
    }

    pub fn multipart_manager(&self) -> &MultipartManager {
        &self.multipart
    }

    pub fn create_bucket(&self, bucket: &str) -> S3Result<()> {
        validate_bucket_name(bucket)?;
        let path = bucket_to_path(bucket);
        debug!("Creating bucket: {} -> {}", bucket, path);
        let mut cart = self.cartridge.write();
        cart.create_dir(&path)?;
        info!("Bucket created: {}", bucket);
        Ok(())
    }

    pub fn delete_bucket(&self, bucket: &str) -> S3Result<()> {
        validate_bucket_name(bucket)?;
        let path = bucket_to_path(bucket);
        let mut cart = self.cartridge.write();
        let files = cart
            .list_dir(&path)
            .map_err(|_| S3Error::NoSuchBucket(bucket.to_string()))?;
        if !files.is_empty() {
            return Err(S3Error::BucketNotEmpty(bucket.to_string()));
        }
        cart.delete_file(&path)?;
        Ok(())
    }

    pub fn list_buckets(&self) -> S3Result<Vec<String>> {
        let cart = self.cartridge.read();
        let files = cart.list_dir("/")?;
        Ok(files
            .into_iter()
            .filter_map(|f| f.strip_prefix('/').map(|s| s.to_string()))
            .collect())
    }

    pub fn put_object(&self, bucket: &str, key: &str, data: &[u8]) -> S3Result<String> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        let path = key_to_path(bucket, key);
        let mut cart = self.cartridge.write();

        // S3 PUT overwrites existing objects, so delete first if exists
        let _ = cart.delete_file(&path); // Ignore error if file doesn't exist
        cart.create_file(&path, data)?;

        let hash = compute_hash(data);
        Ok(generate_etag(&hash))
    }

    pub fn get_object(&self, bucket: &str, key: &str) -> S3Result<Vec<u8>> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        let path = key_to_path(bucket, key);
        let cart = self.cartridge.read();
        cart.read_file(&path)
            .map_err(|_| S3Error::NoSuchKey(format!("{}/{}", bucket, key)))
    }

    pub fn delete_object(&self, bucket: &str, key: &str) -> S3Result<()> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        let path = key_to_path(bucket, key);
        let mut cart = self.cartridge.write();
        cart.delete_file(&path)
            .map_err(|_| S3Error::NoSuchKey(format!("{}/{}", bucket, key)))
    }

    pub fn list_objects(&self, bucket: &str, prefix: Option<&str>) -> S3Result<Vec<String>> {
        validate_bucket_name(bucket)?;
        let bucket_path = bucket_to_path(bucket);
        let cart = self.cartridge.read();
        let files = cart
            .list_dir(&bucket_path)
            .map_err(|_| S3Error::NoSuchBucket(bucket.to_string()))?;
        Ok(files
            .into_iter()
            .filter_map(|f| {
                f.strip_prefix(&format!("{}/", bucket_path))
                    .and_then(|key| {
                        if let Some(p) = prefix {
                            if key.starts_with(p) {
                                Some(key.to_string())
                            } else {
                                None
                            }
                        } else {
                            Some(key.to_string())
                        }
                    })
            })
            .collect())
    }

    pub fn head_object(&self, bucket: &str, key: &str) -> S3Result<(u64, String)> {
        validate_bucket_name(bucket)?;
        validate_key(key)?;
        let path = key_to_path(bucket, key);
        let cart = self.cartridge.read();
        let data = cart
            .read_file(&path)
            .map_err(|_| S3Error::NoSuchKey(format!("{}/{}", bucket, key)))?;
        let size = data.len() as u64;
        let hash = compute_hash(&data);
        Ok((size, generate_etag(&hash)))
    }

    pub fn copy_object(
        &self,
        source_bucket: &str,
        source_key: &str,
        dest_bucket: &str,
        dest_key: &str,
    ) -> S3Result<String> {
        validate_bucket_name(source_bucket)?;
        validate_key(source_key)?;
        validate_bucket_name(dest_bucket)?;
        validate_key(dest_key)?;

        debug!(
            "Copying object: {}/{} -> {}/{}",
            source_bucket, source_key, dest_bucket, dest_key
        );

        // Read source object
        let data = self.get_object(source_bucket, source_key)?;

        // Write to destination
        let etag = self.put_object(dest_bucket, dest_key, &data)?;

        info!(
            "Object copied: {}/{} -> {}/{}",
            source_bucket, source_key, dest_bucket, dest_key
        );
        Ok(etag)
    }

    pub fn delete_objects(
        &self,
        bucket: &str,
        keys: &[String],
    ) -> S3Result<Vec<(String, bool, Option<String>)>> {
        validate_bucket_name(bucket)?;
        debug!(
            "Bulk deleting {} objects from bucket: {}",
            keys.len(),
            bucket
        );

        let mut results = Vec::with_capacity(keys.len());

        for key in keys {
            match self.delete_object(bucket, key) {
                Ok(()) => {
                    results.push((key.clone(), true, None));
                }
                Err(e) => {
                    let error_msg = format!("{:?}", e);
                    results.push((key.clone(), false, Some(error_msg)));
                }
            }
        }

        info!(
            "Bulk delete completed: {} keys, {} succeeded, {} failed",
            keys.len(),
            results.iter().filter(|(_, success, _)| *success).count(),
            results.iter().filter(|(_, success, _)| !*success).count()
        );

        Ok(results)
    }

    // ACL Operations

    /// Put object ACL
    pub fn put_object_acl(&self, bucket: &str, key: &str, acl: &crate::acl::S3Acl) -> S3Result<()> {
        use cartridge_core::header::S3AclMode;

        match self.s3_fuses.acl_mode {
            S3AclMode::Ignore => {
                debug!("ACL mode is Ignore, discarding ACL for {}/{}", bucket, key);
                Ok(())
            }
            S3AclMode::Record | S3AclMode::Enforce => {
                validate_bucket_name(bucket)?;
                validate_key(key)?;
                let path = key_to_path(bucket, key);

                // Serialize ACL to JSON
                let acl_json = acl
                    .to_json()
                    .map_err(|e| S3Error::Internal(format!("Failed to serialize ACL: {}", e)))?;

                // Store in metadata
                let mut cart = self.cartridge.write();
                cart.update_user_metadata(&path, "s3:acl", acl_json)?;

                info!(
                    "Stored ACL for {}/{} (mode: {:?})",
                    bucket, key, self.s3_fuses.acl_mode
                );
                Ok(())
            }
        }
    }

    /// Get object ACL
    pub fn get_object_acl(&self, bucket: &str, key: &str) -> S3Result<crate::acl::S3Acl> {
        use cartridge_core::header::S3AclMode;

        match self.s3_fuses.acl_mode {
            S3AclMode::Ignore => {
                debug!("ACL mode is Ignore, returning empty ACL for {}/{}", bucket, key);
                Ok(crate::acl::S3Acl::empty())
            }
            S3AclMode::Record | S3AclMode::Enforce => {
                validate_bucket_name(bucket)?;
                validate_key(key)?;
                let path = key_to_path(bucket, key);

                // Read metadata
                let cart = self.cartridge.read();
                let metadata = cart
                    .metadata(&path)
                    .map_err(|_| S3Error::NoSuchKey(format!("{}/{}", bucket, key)))?;

                // Check for ACL in user_metadata
                if let Some(acl_json) = metadata.user_metadata.get("s3:acl") {
                    let acl = crate::acl::S3Acl::from_json(acl_json).map_err(|e| {
                        S3Error::Internal(format!("Failed to parse ACL from metadata: {}", e))
                    })?;
                    Ok(acl)
                } else {
                    // No ACL stored, return empty
                    debug!("No ACL found for {}/{}, returning empty ACL", bucket, key);
                    Ok(crate::acl::S3Acl::empty())
                }
            }
        }
    }

    /// Check if a user has a specific permission on an object (for Enforce mode)
    pub fn check_acl_permission(
        &self,
        bucket: &str,
        key: &str,
        user: &str,
        required: &crate::acl::S3Permission,
    ) -> S3Result<bool> {
        use cartridge_core::header::S3AclMode;

        match self.s3_fuses.acl_mode {
            S3AclMode::Ignore | S3AclMode::Record => {
                // Not enforcing, allow all
                Ok(true)
            }
            S3AclMode::Enforce => {
                let acl = self.get_object_acl(bucket, key)?;
                Ok(crate::acl::check_permission(&acl, user, required))
            }
        }
    }

    // SSE Operations

    /// Put object with SSE headers
    pub fn put_object_with_sse(
        &self,
        bucket: &str,
        key: &str,
        data: &[u8],
        sse: &crate::sse::SseHeaders,
    ) -> S3Result<String> {
        use cartridge_core::header::S3SseMode;

        // Always write the object first
        let etag = self.put_object(bucket, key, data)?;

        // Handle SSE metadata based on mode
        match self.s3_fuses.sse_mode {
            S3SseMode::Ignore => {
                debug!("SSE mode is Ignore, discarding SSE headers for {}/{}", bucket, key);
            }
            S3SseMode::Record | S3SseMode::Transparent => {
                if !sse.is_empty() {
                    let sse_json = sse
                        .to_json()
                        .map_err(|e| S3Error::Internal(format!("Failed to serialize SSE headers: {}", e)))?;

                    let mut cart = self.cartridge.write();
                    let path = key_to_path(bucket, key);
                    cart.update_user_metadata(&path, "s3:sse", sse_json)?;

                    info!(
                        "Stored SSE headers for {}/{} (mode: {:?})",
                        bucket, key, self.s3_fuses.sse_mode
                    );
                }
            }
        }

        Ok(etag)
    }

    /// Get object with SSE headers
    pub fn get_object_with_sse(
        &self,
        bucket: &str,
        key: &str,
    ) -> S3Result<(Vec<u8>, Option<crate::sse::SseHeaders>)> {
        use cartridge_core::header::S3SseMode;

        // Always read the object first
        let data = self.get_object(bucket, key)?;

        // Handle SSE metadata based on mode
        let sse = match self.s3_fuses.sse_mode {
            S3SseMode::Ignore | S3SseMode::Record => {
                // Don't return SSE headers
                None
            }
            S3SseMode::Transparent => {
                validate_bucket_name(bucket)?;
                validate_key(key)?;
                let path = key_to_path(bucket, key);

                // Read metadata
                let cart = self.cartridge.read();
                match cart.metadata(&path) {
                    Ok(metadata) => {
                        // Check for SSE in user_metadata
                        if let Some(sse_json) = metadata.user_metadata.get("s3:sse") {
                            crate::sse::SseHeaders::from_json(sse_json).ok()
                        } else {
                            None
                        }
                    }
                    Err(_) => None,
                }
            }
        };

        Ok((data, sse))
    }

    /// Get SSE headers only (for HEAD requests)
    pub fn get_sse_headers(&self, bucket: &str, key: &str) -> S3Result<Option<crate::sse::SseHeaders>> {
        use cartridge_core::header::S3SseMode;

        match self.s3_fuses.sse_mode {
            S3SseMode::Ignore | S3SseMode::Record => Ok(None),
            S3SseMode::Transparent => {
                validate_bucket_name(bucket)?;
                validate_key(key)?;
                let path = key_to_path(bucket, key);

                let cart = self.cartridge.read();
                let metadata = cart
                    .metadata(&path)
                    .map_err(|_| S3Error::NoSuchKey(format!("{}/{}", bucket, key)))?;

                if let Some(sse_json) = metadata.user_metadata.get("s3:sse") {
                    let sse = crate::sse::SseHeaders::from_json(sse_json).map_err(|e| {
                        S3Error::Internal(format!("Failed to parse SSE headers from metadata: {}", e))
                    })?;
                    Ok(Some(sse))
                } else {
                    Ok(None)
                }
            }
        }
    }
}
