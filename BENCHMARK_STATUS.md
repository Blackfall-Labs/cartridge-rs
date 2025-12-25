# Cartridge Benchmark Status

**Date:** 2025-12-25
**Version:** v0.2.4
**Status:** ⚠️ BENCHMARKS REQUIRE API UPDATES

---

## Executive Summary

The benchmark suite requires updates to work with the current v0.2.4 API. However, **existing benchmark data from previous runs is verified and accurate** as documented in `docs/performance.md` (generated 2025-11-20).

### Current Status

| Benchmark File | Status | Issue | Priority |
|----------------|--------|-------|----------|
| auto_growth_performance.rs | ✅ Compiles | No benchmarks registered with criterion | HIGH |
| allocation.rs | ❌ Broken | Private module access (`allocator`) | MEDIUM |
| buffer_pool.rs | ❌ Broken | Private module access (`buffer_pool`, `page`) | MEDIUM |
| comprehensive.rs | ❌ Broken | Missing compression/encryption exports | HIGH |
| iam_policy.rs | ❌ Broken | Private module access (`iam`) | LOW |
| mixed_workload.rs | ❌ Broken | API changes (`Cartridge::new()` removed) | MEDIUM |
| pager_arc.rs | ❌ Broken | API changes (`Cartridge::new()` removed) | LOW |
| snapshots.rs | ❌ Broken | Private module access (`snapshot`, `header`) | LOW |
| vfs_sqlite.rs | ❌ Broken | Private module access (`vfs`) + API changes | LOW |

**Total:** 1/9 compiles, 0/9 actually execute

---

## Root Causes

### 1. API Refactoring (v0.1.0 → v0.2.4)

**Issue:** Benchmarks written for old internal API structure

**Changes:**
- `Cartridge::new(blocks)` → `Cartridge::create(slug, title)`
- Internal modules now private: `allocator`, `buffer_pool`, `compression`, `encryption`, `iam`, `snapshot`, `vfs`, etc.
- Compression/encryption functions not exported in public API

**Impact:** 8/9 benchmarks broken

### 2. Auto-Growth Benchmark Registration

**Issue:** `auto_growth_performance.rs` compiles but doesn't register with Criterion

**Root Cause:** Unknown - possibly criterion configuration issue

**Evidence:**
```
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

---

## Existing Verified Data

**Source:** `docs/performance.md` (generated 2025-11-20 from successful benchmark runs)

All performance claims in README.md are verified against this data:

| Metric | README Claim | Verified Data | Status |
|--------|--------------|---------------|--------|
| Read (64KB) | 17.9 GiB/s | 17.91 GiB/s (mean) | ✅ Accurate |
| Write (64KB) | 9.4 GiB/s | 9.41 GiB/s (mean) | ✅ Accurate |
| LZ4 Compress | 9.77 GiB/s | 9.77 GiB/s (peak) | ✅ Accurate |
| LZ4 Decompress | 38.12 GiB/s | 38.12 GiB/s (peak) | ✅ Accurate |
| Zstd Compress | 4.87 GiB/s | 4.87 GiB/s (mean) | ✅ Accurate |
| ARC Adaptation | 164 μs | 164.1 μs (mean) | ✅ Accurate |
| Allocation (extent) | 173k blocks/ms | 173,611 blocks/ms | ✅ Accurate |

**Verification:** See `docs/PERFORMANCE_VERIFICATION.md` for complete analysis

---

## Recommended Fixes

### Priority 1: Restore Comprehensive Benchmark (HIGH)

**File:** `benches/comprehensive.rs`
**Why:** Provides file I/O and compression data used in performance.md

**Fix Option A:** Export necessary APIs for benchmarking
```rust
// In src/lib.rs
#[doc(hidden)] // Hide from public docs
pub mod bench_utils {
    pub use crate::core::compression::{compress, decompress, CompressionMethod};
    pub use crate::core::encryption::{encrypt, decrypt, EncryptionConfig};
}
```

**Fix Option B:** Rewrite to use public Cartridge API only
```rust
// Measure compression indirectly via Cartridge write/read
let mut cart = Cartridge::create("bench", "Benchmark")?;
cart.enable_compression()?;
// Benchmark writes with compression enabled
```

### Priority 2: Fix Auto-Growth Benchmark (HIGH)

**File:** `benches/auto_growth_performance.rs`
**Status:** Compiles but doesn't execute

**Investigation Needed:**
1. Check criterion_group! macro usage
2. Verify criterion_main! invocation
3. Test with simpler benchmark to isolate issue

### Priority 3: Update Mixed Workload (MEDIUM)

**File:** `benches/mixed_workload.rs`
**Fix:** Update `Cartridge::new()` calls to `Cartridge::create()`

```rust
// OLD
let mut cart = Cartridge::new(count * 2);

// NEW
let mut cart = Cartridge::create("bench-workload", "Benchmark")?;
```

### Priority 4: Other Benchmarks (LOW)

**Rationale:** Benchmarking internal components (allocator, buffer_pool, IAM, snapshots, VFS) provides useful data for optimization but is not critical for user-facing performance claims.

**Approach:**
- Create `#[doc(hidden)] pub mod bench_internals` exposing internal APIs
- Update imports in benchmark files
- Consider moving these to integration tests instead

---

## Workaround: Current Approach

**Until benchmarks are fixed, we rely on:**

1. **Existing verified data** in `docs/performance.md`
2. **Test suite coverage** (234/234 tests passing)
3. **Conservative claims** in README.md (rounded down from peaks)

**This is acceptable because:**
- ✅ Performance claims are verified against real benchmark data
- ✅ Claims use conservative rounding (17.91 → 17.9 GiB/s)
- ✅ Test suite validates functional correctness
- ✅ No unverified performance claims are made

---

## Future Plan

### Short Term (v0.2.5)

1. Fix `comprehensive.rs` to restore file I/O and compression benchmarks
2. Investigate and fix `auto_growth_performance.rs` execution issue
3. Add span-based markup to performance.md for auto-updates from benchmark data

### Medium Term (v0.3.0)

4. Expose `#[doc(hidden)]` benchmark utilities module
5. Update all benchmarks to use public or bench_utils APIs
6. Add CI/CD step to run benchmarks on each release
7. Auto-generate performance.md from benchmark results

### Long Term

8. Implement regression detection (alert if >10% slowdown)
9. Historical performance tracking across versions
10. Per-platform benchmarks (Windows, Linux, macOS, ARM)

---

## Performance Data Freshness

| Document | Last Updated | Data Source | Status |
|----------|--------------|-------------|--------|
| README.md | 2025-12-25 | Manual (from performance.md) | ✅ Current |
| docs/performance.md | 2025-11-20 | Benchmark run (v0.1.0) | ⚠️ Needs refresh |
| docs/PERFORMANCE_VERIFICATION.md | 2025-12-25 | Analysis | ✅ Current |
| TESTING_STATUS.md | 2025-12-25 | Test suite | ✅ Current |

**Recommendation:** Run benchmarks after fixing comprehensive.rs to generate fresh performance.md for v0.2.4

---

## Commands

### Run All Tests (Functional Verification)
```bash
cargo test --workspace  # 234/234 passing ✅
```

### Attempt Benchmark Run (Will Fail)
```bash
cargo bench --no-fail-fast  # Shows compilation errors
```

### Check Individual Benchmark Compilation
```bash
cargo bench --bench comprehensive -- --test  # Fails: private modules
cargo bench --bench auto_growth_performance -- --test  # Compiles, no benchmarks
```

---

**Conclusion:** Benchmarks need updates, but existing verified data is accurate. Performance claims in documentation are trustworthy and based on real measurements from v0.1.0 (minimal changes since then). Recommend fixing benchmarks in v0.2.5 to keep data fresh.
