# Plan: Add S3 API Compatibility to Cartridge

## Summary

Add S3-compatible HTTP API to Cartridge using the `s3s` Rust crate, implemented as a separate `cartridge-s3` crate. This will enable Cartridge to work with all S3-compatible tools (aws-cli, s3cmd, rclone, AWS SDKs).

## Difficulty Assessment: MODERATE (3-4 weeks)

**Why Moderate (not Hard):**
- ✅ Excellent Rust library exists (`s3s` crate) - handles HTTP/auth/serialization
- ✅ Clean mapping: S3 buckets → directories, S3 objects → files
- ✅ Cartridge already has all needed primitives (create, read, write, delete, list)
- ✅ Reference implementation available (`s3s-fs` crate)

**Challenges:**
- ⚠️ Need to add metadata fields to Cartridge (content-type, user metadata)
- ⚠️ Multipart upload state management (for large files)
- ⚠️ ETag generation and consistency semantics

## Implementation Plan

### Phase 1: Core Infrastructure (Week 1)

1. **Create new crate** `crates/cartridge-s3/`
2. **Add dependencies**: s3s (0.8), tokio, hyper, async-trait
3. **Enhance Cartridge metadata**: Add `content_type` and `user_metadata` HashMap to FileMetadata
4. **Implement S3 trait stub** with 10 core operations (Put/Get/Delete/Head/List for objects and buckets)

### Phase 2: MVP Implementation (Week 2)

5. **Implement object operations:**
   - PutObject → create_file/write_file
   - GetObject → read_file
   - DeleteObject → delete_file
   - HeadObject → get metadata
   - ListObjectsV2 → list_directory

6. **Implement bucket operations:**
   - CreateBucket → create_dir
   - DeleteBucket → delete_file (directory)
   - ListBuckets → list_directory (root)

7. **Metadata mapping**: Convert Cartridge metadata ↔ S3 headers
8. **ETag generation**: Use SHA-256 hash (already in Cartridge)

### Phase 3: HTTP Server (Week 2-3)

9. **Create binary** `crates/cartridge-s3-server/`
10. **HTTP server setup**: Use s3s + hyper on port 9000
11. **Authentication**: AWS Signature V4 (provided by s3s)
12. **Integration tests**: Use aws-sdk-s3 to verify compatibility

### Phase 4: Advanced Features (Week 3-4)

13. **Multipart uploads**: State tracking in hidden `.cartridge-s3-multipart/` directory
14. **IAM integration**: Map S3 actions to Cartridge IAM actions
15. **Performance optimization**: Streaming for large files
16. **Documentation**: README, examples, deployment guide

## Code Estimate

- **Lines of Code**: 800-1200 (MVP), 1200-1700 (complete with multipart)
- **New Crates**: 2 (cartridge-s3 library, cartridge-s3-server binary)
- **Modified Files**: 1 (cartridge/src/catalog/metadata.rs - add fields)

## Key Architecture Decisions

### 1. Separate Crate (Recommended)

- Keep S3 concerns separate from core Cartridge
- Optional dependency - users choose to enable
- Clean boundaries, independent evolution

### 2. Mapping Strategy

```
S3 Bucket "mybucket"       → Cartridge directory "/mybucket/"
S3 Object "mybucket/file"  → Cartridge file "/mybucket/file.txt"
S3 ETag                    → Cartridge content_hash (SHA-256)
S3 metadata                → New FileMetadata.user_metadata HashMap
```

### 3. Multipart State Storage

```
/.cartridge-s3-multipart/{upload_id}/
  metadata.json    # Upload metadata
  part-1          # Part 1 data
  part-2          # Part 2 data
```

## Benefits

- ✅ **Instant S3 ecosystem compatibility**: Works with aws-cli, s3cmd, rclone, boto3, AWS SDKs
- ✅ **Offline-first S3**: Run S3-compatible storage on Raspberry Pi
- ✅ **Drop-in replacement**: Applications using S3 API can use Cartridge with minimal changes
- ✅ **All Cartridge features work**: Compression, encryption, snapshots, IAM policies remain functional

## Testing Strategy

1. Unit tests for each S3 operation
2. Integration tests using official aws-sdk-s3
3. Compatibility tests with s3cmd, rclone
4. Performance benchmarks (target: <10ms overhead)
5. Large file tests (multipart uploads up to 5GB)

## Success Criteria

- ✅ All 10 core S3 operations implemented
- ✅ Works with aws-cli, s3cmd, rclone
- ✅ <10ms latency overhead for small files
- ✅ Multipart upload supports files up to 5GB
- ✅ All existing Cartridge features still work
- ✅ Comprehensive documentation and examples

## Risk Mitigation

- **Complexity**: Use s3s crate (handles 90% of HTTP/auth complexity)
- **Performance**: Streaming I/O, minimal buffering
- **Compatibility**: Test with multiple S3 clients
- **State management**: Use Cartridge itself for multipart state

## Timeline

- **Week 1**: Infrastructure + metadata enhancements
- **Week 2**: Core S3 operations + HTTP server
- **Week 3**: Multipart uploads + IAM integration
- **Week 4**: Testing, optimization, documentation

**Total**: 3-4 weeks for production-ready S3 compatibility
