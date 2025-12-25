//! s3s::S3 trait implementation for CartridgeS3Backend

use crate::backend::CartridgeS3Backend;
use crate::error::S3Error;

use s3s::dto::*;
use s3s::s3_error;
use s3s::{S3Request, S3Response, S3Result};

use std::ops::Not;

/// Convert S3Error to s3s error
fn map_error(err: S3Error) -> s3s::S3Error {
    match err {
        S3Error::NoSuchBucket(bucket) => {
            s3_error!(NoSuchBucket, "Bucket does not exist: {}", bucket)
        }
        S3Error::NoSuchKey(key) => s3_error!(NoSuchKey, "Key does not exist: {}", key),
        S3Error::BucketAlreadyExists(bucket) => {
            s3_error!(BucketAlreadyExists, "Bucket already exists: {}", bucket)
        }
        S3Error::BucketNotEmpty(bucket) => {
            s3_error!(BucketNotEmpty, "Bucket not empty: {}", bucket)
        }
        S3Error::InvalidBucketName(name) => {
            s3_error!(InvalidBucketName, "Invalid bucket name: {}", name)
        }
        S3Error::InvalidKey(key) => s3_error!(InvalidArgument, "Invalid key: {}", key),
        S3Error::InvalidRequest(msg) => s3_error!(InvalidArgument, "{}", msg),
        S3Error::Internal(msg) => s3_error!(InternalError, "Internal error: {}", msg),
        S3Error::Io(e) => s3_error!(InternalError, "IO error: {:?}", e),
        S3Error::Cartridge(e) => s3_error!(InternalError, "Cartridge error: {:?}", e),
    }
}

#[async_trait::async_trait]
impl s3s::S3 for CartridgeS3Backend {
    // [instrument removed for simplicity]
    async fn create_bucket(
        &self,
        req: S3Request<CreateBucketInput>,
    ) -> S3Result<S3Response<CreateBucketOutput>> {
        let input = req.input;

        self.create_bucket(&input.bucket).map_err(map_error)?;

        let output = CreateBucketOutput::default();
        Ok(S3Response::new(output))
    }

    // [instrument removed for simplicity]
    async fn delete_bucket(
        &self,
        req: S3Request<DeleteBucketInput>,
    ) -> S3Result<S3Response<DeleteBucketOutput>> {
        let input = req.input;

        self.delete_bucket(&input.bucket).map_err(map_error)?;

        Ok(S3Response::new(DeleteBucketOutput {}))
    }

    // [instrument removed for simplicity]
    async fn head_bucket(
        &self,
        req: S3Request<HeadBucketInput>,
    ) -> S3Result<S3Response<HeadBucketOutput>> {
        let input = req.input;

        // Check if bucket exists by trying to list it
        let buckets = self.list_buckets().map_err(map_error)?;
        if buckets.contains(&input.bucket).not() {
            return Err(s3_error!(NoSuchBucket));
        }

        Ok(S3Response::new(HeadBucketOutput::default()))
    }

    // [instrument removed for simplicity]
    async fn list_buckets(
        &self,
        _: S3Request<ListBucketsInput>,
    ) -> S3Result<S3Response<ListBucketsOutput>> {
        let bucket_names = self.list_buckets().map_err(map_error)?;

        let buckets: Vec<Bucket> = bucket_names
            .into_iter()
            .map(|name| Bucket {
                name: Some(name),
                creation_date: None,
                bucket_region: None,
            })
            .collect();

        let output = ListBucketsOutput {
            buckets: if buckets.is_empty() {
                None
            } else {
                Some(buckets)
            },
            owner: None,
            ..Default::default()
        };

        Ok(S3Response::new(output))
    }

    // [instrument removed for simplicity]
    async fn delete_object(
        &self,
        req: S3Request<DeleteObjectInput>,
    ) -> S3Result<S3Response<DeleteObjectOutput>> {
        let input = req.input;

        self.delete_object(&input.bucket, &input.key)
            .map_err(map_error)?;

        let output = DeleteObjectOutput::default();
        Ok(S3Response::new(output))
    }

    // [instrument removed for simplicity]
    async fn get_object(
        &self,
        req: S3Request<GetObjectInput>,
    ) -> S3Result<S3Response<GetObjectOutput>> {
        let input = req.input;

        let data = self
            .get_object(&input.bucket, &input.key)
            .map_err(map_error)?;
        let size = data.len() as i64;

        // Compute ETag (SHA-256 for now, similar to our backend)
        use crate::utils::{compute_hash, generate_etag};
        let hash = compute_hash(&data);
        let etag = generate_etag(&hash);

        // Convert to ByteStream for s3s
        use bytes::Bytes;
        use futures::stream;
        let byte_stream =
            stream::once(async move { Result::<Bytes, std::io::Error>::Ok(Bytes::from(data)) });
        let body = StreamingBlob::wrap(byte_stream);

        let output = GetObjectOutput {
            body: Some(body),
            content_length: Some(size),
            e_tag: Some(ETag::Strong(etag)),
            ..Default::default()
        };

        Ok(S3Response::new(output))
    }

    // [instrument removed for simplicity]
    async fn head_object(
        &self,
        req: S3Request<HeadObjectInput>,
    ) -> S3Result<S3Response<HeadObjectOutput>> {
        let input = req.input;

        let (size, etag) = self
            .head_object(&input.bucket, &input.key)
            .map_err(map_error)?;

        let output = HeadObjectOutput {
            content_length: Some(size as i64),
            e_tag: Some(ETag::Strong(etag)),
            content_type: Some(mime::APPLICATION_OCTET_STREAM.to_string()),
            last_modified: Some(s3s::dto::Timestamp::from(std::time::SystemTime::now())),
            ..Default::default()
        };

        Ok(S3Response::new(output))
    }

    // [instrument removed for simplicity]
    async fn list_objects(
        &self,
        req: S3Request<ListObjectsInput>,
    ) -> S3Result<S3Response<ListObjectsOutput>> {
        let v2_resp = self.list_objects_v2(req.map_input(Into::into)).await?;

        Ok(v2_resp.map_output(|v2| ListObjectsOutput {
            contents: v2.contents,
            common_prefixes: v2.common_prefixes,
            delimiter: v2.delimiter,
            encoding_type: v2.encoding_type,
            name: v2.name,
            prefix: v2.prefix,
            max_keys: v2.max_keys,
            is_truncated: v2.is_truncated,
            ..Default::default()
        }))
    }

    // [instrument removed for simplicity]
    async fn list_objects_v2(
        &self,
        req: S3Request<ListObjectsV2Input>,
    ) -> S3Result<S3Response<ListObjectsV2Output>> {
        let input = req.input;

        let prefix = input.prefix.as_deref();
        let keys = self
            .list_objects(&input.bucket, prefix)
            .map_err(map_error)?;

        let max_keys = input.max_keys.unwrap_or(1000);
        let is_truncated = keys.len() > max_keys as usize;
        let keys_to_return: Vec<String> = keys.into_iter().take(max_keys as usize).collect();

        let contents: Vec<Object> = keys_to_return
            .into_iter()
            .map(|key| {
                Object {
                    key: Some(key),
                    size: None, // Could be enhanced later
                    last_modified: Some(s3s::dto::Timestamp::from(std::time::SystemTime::now())),
                    ..Default::default()
                }
            })
            .collect();

        let key_count = contents.len() as i32;

        let output = ListObjectsV2Output {
            contents: if contents.is_empty() {
                None
            } else {
                Some(contents)
            },
            name: Some(input.bucket),
            prefix: input.prefix,
            max_keys: Some(max_keys),
            is_truncated: Some(is_truncated),
            key_count: Some(key_count),
            ..Default::default()
        };

        Ok(S3Response::new(output))
    }

    // [instrument removed for simplicity]
    async fn put_object(
        &self,
        req: S3Request<PutObjectInput>,
    ) -> S3Result<S3Response<PutObjectOutput>> {
        let input = req.input;

        let body = input.body.ok_or_else(|| s3_error!(IncompleteBody))?;

        // Collect the streaming body into bytes
        use futures::TryStreamExt;
        let data: Vec<u8> = body
            .try_fold(Vec::new(), |mut acc, chunk| async move {
                acc.extend_from_slice(&chunk);
                Ok(acc)
            })
            .await
            .map_err(|e| s3_error!(InternalError, "Failed to read body: {:?}", e))?;

        let etag = self
            .put_object(&input.bucket, &input.key, &data)
            .map_err(map_error)?;

        let output = PutObjectOutput {
            e_tag: Some(ETag::Strong(etag)),
            ..Default::default()
        };

        Ok(S3Response::new(output))
    }

    // Multipart upload operations

    async fn create_multipart_upload(
        &self,
        req: S3Request<CreateMultipartUploadInput>,
    ) -> S3Result<S3Response<CreateMultipartUploadOutput>> {
        let input = req.input;

        let upload_id = self
            .multipart_manager()
            .create_upload(input.bucket.clone(), input.key.clone());

        let output = CreateMultipartUploadOutput {
            bucket: Some(input.bucket),
            key: Some(input.key),
            upload_id: Some(upload_id),
            ..Default::default()
        };

        Ok(S3Response::new(output))
    }

    async fn upload_part(
        &self,
        req: S3Request<UploadPartInput>,
    ) -> S3Result<S3Response<UploadPartOutput>> {
        let input = req.input;

        let upload_id = &input.upload_id;
        let part_number = input.part_number;

        let body = input.body.ok_or_else(|| s3_error!(IncompleteBody))?;

        // Collect the streaming body into bytes
        use futures::TryStreamExt;
        let data: Vec<u8> = body
            .try_fold(Vec::new(), |mut acc, chunk| async move {
                acc.extend_from_slice(&chunk);
                Ok(acc)
            })
            .await
            .map_err(|e| s3_error!(InternalError, "Failed to read body: {:?}", e))?;

        // Compute ETag for the part
        use crate::utils::{compute_hash, generate_etag};
        let hash = compute_hash(&data);
        let etag = generate_etag(&hash);

        // Store the part
        self.multipart_manager()
            .upload_part(&upload_id, part_number, data, etag.clone())
            .map_err(|e| s3_error!(NoSuchUpload, "{}", e))?;

        let output = UploadPartOutput {
            e_tag: Some(ETag::Strong(etag)),
            ..Default::default()
        };

        Ok(S3Response::new(output))
    }

    async fn complete_multipart_upload(
        &self,
        req: S3Request<CompleteMultipartUploadInput>,
    ) -> S3Result<S3Response<CompleteMultipartUploadOutput>> {
        let input = req.input;

        let upload_id = &input.upload_id;

        // Complete the upload and get assembled data
        let (bucket, key, data) = self
            .multipart_manager()
            .complete_upload(&upload_id)
            .ok_or_else(|| s3_error!(NoSuchUpload, "Upload ID not found or parts missing"))?;

        // Upload the assembled object to Cartridge
        let etag = self.put_object(&bucket, &key, &data).map_err(map_error)?;

        let output = CompleteMultipartUploadOutput {
            bucket: Some(bucket),
            key: Some(key),
            e_tag: Some(ETag::Strong(etag)),
            ..Default::default()
        };

        Ok(S3Response::new(output))
    }

    async fn abort_multipart_upload(
        &self,
        req: S3Request<AbortMultipartUploadInput>,
    ) -> S3Result<S3Response<AbortMultipartUploadOutput>> {
        let input = req.input;

        let upload_id = &input.upload_id;

        if !self.multipart_manager().abort_upload(&upload_id) {
            return Err(s3_error!(NoSuchUpload, "Upload ID not found"));
        }

        Ok(S3Response::new(AbortMultipartUploadOutput::default()))
    }

    async fn list_parts(
        &self,
        req: S3Request<ListPartsInput>,
    ) -> S3Result<S3Response<ListPartsOutput>> {
        let input = req.input;

        let upload_id = &input.upload_id;

        let parts = self
            .multipart_manager()
            .list_parts(&upload_id)
            .ok_or_else(|| s3_error!(NoSuchUpload, "Upload ID not found"))?;

        let s3_parts: Vec<Part> = parts
            .into_iter()
            .map(|p| Part {
                part_number: Some(p.part_number),
                e_tag: Some(ETag::Strong(p.etag)),
                size: Some(p.data.len() as i64),
                ..Default::default()
            })
            .collect();

        let output = ListPartsOutput {
            parts: if s3_parts.is_empty() {
                None
            } else {
                Some(s3_parts)
            },
            ..Default::default()
        };

        Ok(S3Response::new(output))
    }

    async fn copy_object(
        &self,
        req: S3Request<CopyObjectInput>,
    ) -> S3Result<S3Response<CopyObjectOutput>> {
        let input = req.input;

        // Parse copy source: "/source-bucket/source-key"
        // CopySource is a complex enum, use Debug formatting
        let copy_source_str = format!("{:?}", input.copy_source);
        let parts: Vec<&str> = copy_source_str
            .trim_start_matches('/')
            .splitn(2, '/')
            .collect();

        if parts.len() != 2 {
            return Err(s3_error!(
                InvalidArgument,
                "Invalid copy source format: {}",
                copy_source_str
            ));
        }

        let source_bucket = parts[0];
        let source_key = parts[1];
        let dest_bucket = &input.bucket;
        let dest_key = &input.key;

        let etag = self
            .copy_object(source_bucket, source_key, dest_bucket, dest_key)
            .map_err(map_error)?;

        let output = CopyObjectOutput {
            copy_object_result: Some(CopyObjectResult {
                e_tag: Some(ETag::Strong(etag)),
                last_modified: Some(s3s::dto::Timestamp::from(std::time::SystemTime::now())),
                ..Default::default()
            }),
            ..Default::default()
        };

        Ok(S3Response::new(output))
    }

    async fn delete_objects(
        &self,
        req: S3Request<DeleteObjectsInput>,
    ) -> S3Result<S3Response<DeleteObjectsOutput>> {
        let input = req.input;

        // Delete is a required field, not Option
        let delete_request = input.delete;

        // Objects is a Vec<ObjectIdentifier>, extract keys
        // key is a required field (ObjectKey type), not Option
        let keys: Vec<String> = delete_request
            .objects
            .iter()
            .map(|obj| obj.key.clone())
            .collect();

        if keys.is_empty() {
            return Err(s3_error!(
                InvalidRequest,
                "No objects specified for deletion"
            ));
        }

        let results = self
            .delete_objects(&input.bucket, &keys)
            .map_err(map_error)?;

        let mut deleted = Vec::new();
        let mut errors = Vec::new();

        for (key, success, error) in results {
            if success {
                deleted.push(DeletedObject {
                    key: Some(key),
                    ..Default::default()
                });
            } else {
                errors.push(Error {
                    key: Some(key),
                    code: Some("InternalError".to_string()),
                    message: error,
                    ..Default::default()
                });
            }
        }

        let output = DeleteObjectsOutput {
            deleted: if deleted.is_empty() {
                None
            } else {
                Some(deleted)
            },
            errors: if errors.is_empty() {
                None
            } else {
                Some(errors)
            },
            ..Default::default()
        };

        Ok(S3Response::new(output))
    }

    async fn get_bucket_location(
        &self,
        _: S3Request<GetBucketLocationInput>,
    ) -> S3Result<S3Response<GetBucketLocationOutput>> {
        Ok(S3Response::new(GetBucketLocationOutput::default()))
    }

    async fn upload_part_copy(
        &self,
        _: S3Request<UploadPartCopyInput>,
    ) -> S3Result<S3Response<UploadPartCopyOutput>> {
        Err(s3_error!(
            NotImplemented,
            "Upload part copy not yet implemented"
        ))
    }
}
