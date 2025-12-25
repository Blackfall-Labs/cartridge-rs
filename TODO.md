# Cartridge-RS TODO

## Critical Items

### Encryption API Implementation
**Status:** Not Yet Implemented
**Priority:** HIGH
**Tracked By:** Phase 5 Testing (tests/security_encryption.rs)

The encryption module exists (`src/core/encryption.rs`) but is not exposed on the public API:
- `create_encrypted()` - Create encrypted cartridge with password
- `open_encrypted()` - Open encrypted cartridge with password
- Key derivation (Argon2)
- AES-256-GCM encryption
- Nonce uniqueness validation

**Tests Waiting (5 ignored tests in tests/security_encryption.rs):**
- [ ] test_encryption_key_derivation
- [ ] test_encryption_nonce_uniqueness
- [ ] test_wrong_decryption_key
- [ ] test_encryption_tamper_detection
- [ ] test_encryption_performance

**Action Items:**
1. Expose encryption API on public Cartridge wrapper (src/lib.rs)
2. Add `CartridgeBuilder::with_encryption(password)`
3. Implement `Cartridge::open_encrypted(path, password)`
4. Enable and verify all 5 encryption security tests
5. Add encryption examples and documentation

**Estimated Effort:** 1-2 days

---

## Testing Plan Progress

- [x] Phase 1: Data Integrity & FFI Safety (26 tests)
- [x] Phase 2: Concurrency & Durability (26 tests)
- [x] Phase 3: Performance & Scale (8 tests)
- [x] Phase 4: Advanced Features (17 tests)
- [x] Phase 5: Security Audit (19 passing, 5 ignored - encryption)
- [ ] Phase 6: VFS FFI Unsafe Code Validation (IN PROGRESS)

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
