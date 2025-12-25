# Cartridge-RS TODO

## Critical Items

### Encryption API Implementation
**Status:** ✅ COMPLETED (2025-12-25)
**Priority:** HIGH
**Tracked By:** Phase 5 Testing (tests/security_encryption.rs)

The encryption module has been fully integrated and exposed on the public API:
- ✅ `enable_encryption(key)` - Enable encryption with provided key
- ✅ `disable_encryption()` - Disable encryption
- ✅ `is_encrypted()` - Check encryption status
- ✅ `EncryptionConfig::generate_key()` - Generate random key
- ✅ AES-256-GCM encryption with authenticated encryption
- ✅ Nonce uniqueness (random 96-bit nonces per encryption)

**Tests Status (5/5 passing in tests/security_encryption.rs):**
- ✅ test_encryption_key_derivation
- ✅ test_encryption_nonce_uniqueness
- ✅ test_wrong_decryption_key
- ✅ test_encryption_tamper_detection
- ✅ test_encryption_performance

**Implementation Details:**
- Files encrypted with AES-256-GCM (28-byte overhead per file)
- Original file size stored in `metadata.size`
- Encrypted size stored in `metadata.user_metadata["encrypted_size"]`
- Encryption flag stored in `metadata.user_metadata["encrypted"]`
- Automatic encryption/decryption on write/read operations

---

## Known Issues

### Engram Integration Tests
**Status:** ✅ FIXED (2025-12-25)
**Resolution:** Updated to engram-rs 1.1.1 API which requires explicit `initialize()` call after `open()`

**Overall Test Status:** 234/234 passing (100%) ✅

**What Changed:**
- engram-rs 1.1.1 changed the API so that `ArchiveReader::open()` no longer automatically loads the central directory
- Now requires explicit call to `reader.initialize()` after opening
- Updated all 10 usage sites across the codebase:
  - `src/core/engram_integration.rs` (2 locations)
  - `src/core/integration_tests.rs` (4 locations)
  - `tests/engram_freeze_validation.rs` (4 locations)

**Files Modified:**
- Added `.initialize()` calls after all `ArchiveReader::open()` invocations
- Removed debug logging from engram_integration.rs
- Updated TESTING_STATUS.md to reflect 234/234 passing tests

---

## Testing Plan Progress

- [x] Phase 1: Data Integrity & FFI Safety (26 tests)
- [x] Phase 2: Concurrency & Durability (26 tests)
- [x] Phase 3: Performance & Scale (8 tests)
- [x] Phase 4: Advanced Features (17 tests)
- [x] Phase 5: Security Audit (24 passing - encryption now included!)
- [x] Phase 6: VFS FFI Unsafe Code Validation (19 tests)

---

## Future Enhancements

### Catalog Scalability
- Current: Single 4KB page limit (~30-50 files depending on path length)
- Future: Multi-page catalog or B-tree paging for 1M+ files

### IAM Features
- [ ] Conditional policies (time-based, attribute-based)
- [ ] Policy versioning
- [ ] Audit trail for policy changes
- [ ] Role-based access control (RBAC)

### Snapshot Features
- [ ] Incremental snapshots (delta compression)
- [ ] Snapshot metadata tags
- [ ] Automated snapshot scheduling
- [ ] Snapshot retention policies

---

**Last Updated:** 2025-12-24
