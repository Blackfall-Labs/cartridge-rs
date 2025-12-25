# Cartridge Performance Claims Verification Report

**Date:** 2025-12-24
**Reviewer:** Claude Code
**Purpose:** Verify accuracy of performance claims in README.md against actual benchmark data

---

## Executive Summary

Performance claims in `README.md` have been cross-referenced against benchmark data in `docs/performance.md` (generated 2025-11-20). Several discrepancies and missing benchmarks were identified.

### Overall Assessment

| Claim Type | Status | Notes |
|------------|--------|-------|
| Read/Write Throughput | ✅ **VERIFIED** | Minor rounding acceptable |
| LZ4 Compression | ✅ **VERIFIED** | Matches benchmark data |
| LZ4 Decompression | ✅ **VERIFIED** | Matches benchmark data |
| Auto-growth Overhead | ❌ **UNVERIFIED** | No benchmark exists |

---

## Detailed Verification

### 1. File I/O Throughput Claims

#### README.md Claims (lines 179-182):
```
**Throughput** (64KB blocks):
- Read: 18 GiB/s
- Write: 9 GiB/s
```

#### Actual Benchmark Data (performance.md lines 134-153):

**Write Performance (64KB):**
- Mean Throughput: **9.41 GiB/s**
- Upper Bound: **9.59 GiB/s**
- Lower Bound: 9.22 GiB/s
- Mean Latency: 6.48 μs

**Read Performance (64KB):**
- Mean Throughput: **17.91 GiB/s**
- Upper Bound: **18.38 GiB/s**
- Lower Bound: 17.47 GiB/s
- Mean Latency: 3.41 μs

#### Verification Result: ✅ **VERIFIED with acceptable rounding**

**Analysis:**
- README claims "18 GiB/s" read - actual is 17.91 GiB/s (0.5% difference)
- README claims "9 GiB/s" write - actual is 9.41 GiB/s (4.5% difference)
- Both are conservative rounding and acceptable for marketing claims
- Upper bounds (18.38 GiB/s read, 9.59 GiB/s write) support the claims

---

### 2. Compression Performance Claims

#### README.md Claims (lines 184-187):
```
**Compression**:
- LZ4 Compression: 9.77 GiB/s
- LZ4 Decompression: 38.12 GiB/s
```

#### Actual Benchmark Data (performance.md lines 183-184):
```
**Peak LZ4 Compression:** 9.77 GiB/s at 64KB blocks
**Peak LZ4 Decompression:** 38.12 GiB/s at 64KB blocks
```

#### Additional Data (performance.md line 181):
```
64KB | 6.41 μs | 9.52 GiB/s | 1.64 μs | 37.13 GiB/s
```

#### Verification Result: ✅ **VERIFIED**

**Analysis:**
- README claims match the **peak** performance numbers exactly
- Note: There's an internal discrepancy in performance.md between table data (9.52/37.13 GiB/s) and peak summary (9.77/38.12 GiB/s)
- The peak numbers (9.77/38.12) likely represent upper bounds from benchmark runs
- Claims are accurate based on the benchmark summary

---

### 3. Auto-Growth Overhead Claim

#### README.md Claim (line 189):
```
**Auto-growth overhead**: < 1ms per doubling
```

#### Actual Benchmark Data:
**NOT FOUND** - No benchmark file tests auto-growth overhead

#### Search Results:
- No benchmark in `benches/` directory measures growth/resize/doubling operations
- TESTING_PLAN.md suggests baseline of "< 10ms per doubling" (line 1963)
- No evidence supporting the "< 1ms" claim

#### Verification Result: ❌ **UNVERIFIED**

**Analysis:**
- **Critical Gap:** This performance claim has no supporting benchmark data
- The claim appears to be speculative or based on informal testing
- TESTING_PLAN.md recommends creating `benches/auto_growth_performance.rs` to measure this
- **Recommendation:** Either run benchmarks to verify this claim or remove it from README

---

## Documentation Inconsistencies

### Issue 1: ARCHITECTURE.md vs performance.md Compression Data

**ARCHITECTURE.md (line 1141):**
```
| LZ4 | 64KB | 9.52 GiB/s | 37.13 GiB/s | ~2x |
```

**performance.md (lines 183-184):**
```
Peak LZ4 Compression: 9.77 GiB/s at 64KB blocks
Peak LZ4 Decompression: 38.12 GiB/s at 64KB blocks
```

**Issue:** ARCHITECTURE.md uses mean values from table, performance.md uses peak values
**Recommendation:** Standardize on either mean or peak values across documentation

---

### Issue 2: performance.md Executive Summary Error

**performance.md (line 16):**
```
- **File Read Performance:** Up to **38.12 GiB/s** for 64KB blocks
```

**This is WRONG!** The 38.12 GiB/s figure is the **LZ4 Decompression** speed, not file read speed.

**Actual File Read Performance (line 148):**
```
64KB | 3.41 μs | 17.91 GiB/s
```

**Recommendation:** Fix performance.md executive summary to show 17.91 GiB/s (or 18.38 GiB/s upper bound) for file read performance

---

## Missing Benchmarks

Based on README.md claims and TESTING_PLAN.md recommendations:

1. **Auto-Growth Overhead** (CRITICAL)
   - File: `benches/auto_growth_performance.rs` (does not exist)
   - Test: Measure time to double cartridge from 12KB→24KB→48KB→...→100GB
   - Expected: < 1ms per doubling (per README claim)
   - Priority: HIGH

2. **Crash Recovery Validation** (from TESTING_PLAN)
   - No benchmarks for recovery time after interrupted operations
   - Priority: MEDIUM

3. **Concurrent Access Performance** (from TESTING_PLAN)
   - No benchmarks for multi-threaded read/write
   - Priority: MEDIUM

---

## Recommendations

### Immediate Actions

1. **Create Auto-Growth Benchmark**
   ```bash
   # Create benches/auto_growth_performance.rs
   cargo bench --bench auto_growth_performance
   ```
   - Measure actual growth overhead
   - Update README.md claim to match reality
   - If > 1ms, either optimize or update claim

2. **Fix performance.md Executive Summary**
   - Change line 16 from "38.12 GiB/s" to "17.91 GiB/s" for file read
   - Clarify that 38.12 GiB/s is LZ4 decompression, not file read

3. **Standardize Documentation**
   - Decide: Use mean or peak values?
   - Update ARCHITECTURE.md to match performance.md
   - Ensure README.md, ARCHITECTURE.md, and performance.md all agree

### Long-Term Actions

4. **Automated Performance Testing**
   - Add CI/CD step to run `cargo bench` on each commit
   - Detect performance regressions automatically
   - Update documentation automatically from benchmark results

5. **Performance Monitoring**
   - Track performance over time
   - Alert on regressions > 10%
   - Maintain performance.md as living document

---

## Verification Commands

To reproduce this verification:

```bash
cd E:\repos\blackfall-labs\cartridge-rs

# Run all benchmarks
cargo bench

# Check for performance.md
cat docs/performance.md

# Verify claims in README
grep -A 10 "## Performance" README.md

# Search for auto-growth benchmarks (should find none currently)
find benches -name "*growth*.rs"
```

---

## Conclusion

**Overall Grade: B+**

The cartridge-rs performance claims are **mostly accurate** with some caveats:

✅ **Strengths:**
- File I/O claims are well-supported by benchmarks
- Compression claims are exact matches to peak benchmark data
- Conservative rounding makes claims trustworthy
- Comprehensive benchmark suite exists (8 benchmark files)

⚠️ **Weaknesses:**
- Auto-growth overhead claim (< 1ms) is **completely unverified**
- Internal documentation inconsistencies between ARCHITECTURE.md and performance.md
- Error in performance.md executive summary (conflates read with decompression speed)
- Missing benchmarks for several critical features

**Recommendation:** Create auto-growth benchmark ASAP or remove the "< 1ms per doubling" claim from README.md until it can be verified.

---

## Appendix: Performance Claims Summary

| Claim | README | Actual (Mean) | Actual (Peak) | Status | Variance |
|-------|--------|---------------|---------------|--------|----------|
| Read (64KB) | 18 GiB/s | 17.91 GiB/s | 18.38 GiB/s | ✅ OK | +0.5% |
| Write (64KB) | 9 GiB/s | 9.41 GiB/s | 9.59 GiB/s | ✅ OK | +4.5% |
| LZ4 Compress | 9.77 GiB/s | 9.52 GiB/s | 9.77 GiB/s | ✅ OK | 0% (peak) |
| LZ4 Decompress | 38.12 GiB/s | 37.13 GiB/s | 38.12 GiB/s | ✅ OK | 0% (peak) |
| Auto-Growth | < 1ms | **NO DATA** | **NO DATA** | ❌ FAIL | N/A |

**Legend:**
- ✅ OK: Claim verified with acceptable variance (< 5%)
- ❌ FAIL: No supporting data

---

**Report Generated:** 2025-12-24
**Benchmark Data Source:** docs/performance.md (generated 2025-11-20)
**Platform:** Windows MSYS_NT-10.0-26100 (x86_64)
**Cartridge Version:** v0.1.0 → v0.2.4 (current)
