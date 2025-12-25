# Cartridge Testing Plan
**Version:** 1.0
**Last Updated:** 2025-12-24
**Status:** Comprehensive testing roadmap for production readiness

## Executive Summary

### Current State
- **Total Tests:** 242 tests across 29 test files
- **Benchmarks:** 8 comprehensive benchmark suites (2,129 lines)
- **Examples:** 10 working examples
- **Test Coverage:** Core functionality well-tested, critical gaps in edge cases
- **Unsafe Code:** 29 unsafe blocks (all in SQLite VFS FFI layer)

### Critical Gaps Identified
1. **VFS FFI Fuzzing:** HIGHEST PRIORITY - 29 unsafe blocks need comprehensive fuzzing
2. **Corruption Detection:** No systematic corruption testing for pages/B-tree/allocators
3. **Crash Recovery:** No testing of auto-growth interruption or snapshot consistency
4. **Concurrency:** Limited multi-threaded stress tests despite parking_lot::Mutex usage
5. **Property-Based Tests:** No allocator invariant validation
6. **Extreme Scale:** Largest test is small - need 100GB containers, 1M files
7. **Security:** No systematic attack testing for IAM bypass, encryption weaknesses

### Testing Philosophy

Cartridge adopts a **defensive programming** approach inspired by:
- **LMDB:** Memory-mapped I/O, ACID transactions, crash recovery
- **RocksDB/LevelDB:** LSM trees, compaction stress, corruption injection
- **SQLite:** Malformed database testing, OOM simulation, I/O error injection
- **ZFS:** Self-healing, checksums, corruption detection, scrubbing
- **Btrfs:** Extent allocators, CoW integrity

**Core Principles:**
1. **Safety First:** All unsafe code (VFS FFI) gets exhaustive testing
2. **ACID Guarantees:** Atomicity, Consistency, Isolation, Durability validated
3. **Fail-Safe Defaults:** Corruption never propagates, always detect early
4. **Allocator Correctness:** Free block accounting must always be exact
5. **Concurrent Safety:** RwLock patterns must prevent data races
6. **Observable Behavior:** All operations logged, traceable, debuggable

---

## Phase 1: Critical (Data Integrity & FFI Safety)

**Timeline:** 2-3 weeks
**Priority:** BLOCKER - Must complete before production use
**Focus:** VFS FFI fuzzing, corruption detection, allocator correctness

### 1.1 VFS FFI Fuzzing (HIGHEST PRIORITY)

**Rationale:** 29 unsafe blocks in VFS layer interact with C code - highest risk for memory safety violations

#### 1.1.1 Setup VFS Fuzzing Infrastructure

**File:** `fuzz/Cargo.toml`

```toml
[package]
name = "cartridge-fuzz"
version = "0.0.0"
publish = false
edition = "2021"

[dependencies]
cartridge-rs = { path = ".." }
libfuzzer-sys = "0.4"
rusqlite = { version = "0.32", features = ["bundled"] }

[[bin]]
name = "fuzz_vfs_open"
path = "fuzz_targets/fuzz_vfs_open.rs"
test = false
doc = false

[[bin]]
name = "fuzz_vfs_read_write"
path = "fuzz_targets/fuzz_vfs_read_write.rs"
test = false
doc = false

[[bin]]
name = "fuzz_vfs_operations"
path = "fuzz_targets/fuzz_vfs_operations.rs"
test = false
doc = false
```

#### 1.1.2 VFS Open/Close Fuzzer

**File:** `fuzz/fuzz_targets/fuzz_vfs_open.rs`

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
use cartridge_rs::Cartridge;
use rusqlite::Connection;

fuzz_target!(|data: &[u8]| {
    // Create cartridge
    let mut cart = Cartridge::create("fuzz-test", "Fuzz Test").unwrap();

    // Write fuzzed data as database file
    if cart.write("/db.sqlite", data).is_ok() {
        // Try to open via VFS
        unsafe {
            // Register VFS
            cartridge_rs::vfs::register_vfs();

            // Attempt to open - should not crash
            let uri = format!("file:///db.sqlite?vfs=cartridge&cartridge={}", "fuzz-test.cart");
            let _ = Connection::open_with_flags(
                &uri,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            );

            // Unregister
            cartridge_rs::vfs::unregister_vfs();
        }
    }
});
```

#### 1.1.3 VFS Read/Write Fuzzer

**File:** `fuzz/fuzz_targets/fuzz_vfs_read_write.rs`

```rust
#![no_main]
use libfuzzer_sys::{fuzz_target, arbitrary::{Arbitrary, Unstructured}};
use cartridge_rs::Cartridge;
use rusqlite::Connection;

#[derive(Debug, Arbitrary)]
struct VFSOperation {
    op_type: u8,      // 0=read, 1=write, 2=sync, 3=truncate
    offset: u32,
    length: u16,
    data: Vec<u8>,
}

fuzz_target!(|input: &[u8]| {
    let mut u = Unstructured::new(input);

    // Parse fuzzed operations
    let ops: Vec<VFSOperation> = match u.arbitrary() {
        Ok(ops) => ops,
        Err(_) => return,
    };

    // Setup VFS
    let mut cart = Cartridge::create("fuzz-vfs-rw", "Fuzz VFS RW").unwrap();
    cart.write("/db.sqlite", b"SQLite format 3\0").unwrap();

    unsafe {
        cartridge_rs::vfs::register_vfs();

        let uri = "file:///db.sqlite?vfs=cartridge&cartridge=fuzz-vfs-rw.cart";
        if let Ok(conn) = Connection::open(uri) {
            // Execute fuzzed VFS operations through SQLite
            for op in ops {
                match op.op_type % 4 {
                    0 => {
                        // Read
                        let _ = conn.query_row(
                            "SELECT randomblob(?)",
                            [op.length],
                            |_| Ok(())
                        );
                    }
                    1 => {
                        // Write
                        let _ = conn.execute(
                            "CREATE TABLE IF NOT EXISTS t(x)",
                            []
                        );
                    }
                    2 => {
                        // Sync
                        let _ = conn.execute("PRAGMA synchronous=FULL", []);
                    }
                    3 => {
                        // Truncate (via vacuum)
                        let _ = conn.execute("VACUUM", []);
                    }
                    _ => {}
                }
            }
        }

        cartridge_rs::vfs::unregister_vfs();
    }
});
```

#### 1.1.4 VFS Concurrent Operations Fuzzer

**File:** `fuzz/fuzz_targets/fuzz_vfs_concurrent.rs`

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
use cartridge_rs::Cartridge;
use rusqlite::Connection;
use std::sync::Arc;
use parking_lot::RwLock;

fuzz_target!(|data: &[u8]| {
    if data.len() < 100 {
        return;
    }

    // Setup cartridge with test database
    let mut cart = Cartridge::create("fuzz-concurrent", "Fuzz Concurrent").unwrap();
    cart.write("/db.sqlite", data).unwrap();

    let cart_arc = Arc::new(RwLock::new(cart));

    unsafe {
        cartridge_rs::vfs::register_vfs();

        // Spawn 4 concurrent VFS operations
        let handles: Vec<_> = (0..4).map(|_| {
            std::thread::spawn(|| {
                let uri = "file:///db.sqlite?vfs=cartridge&cartridge=fuzz-concurrent.cart";
                if let Ok(conn) = Connection::open_with_flags(
                    uri,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                ) {
                    // Try various operations
                    let _ = conn.query_row("SELECT 1", [], |_| Ok(()));
                    let _ = conn.execute("PRAGMA page_count", []);
                }
            })
        }).collect();

        for h in handles {
            let _ = h.join();
        }

        cartridge_rs::vfs::unregister_vfs();
    }
});
```

#### 1.1.5 Running VFS Fuzzers

**Commands:**
```bash
# Install cargo-fuzz
cargo install cargo-fuzz

# Run VFS open fuzzer (24 hours continuous)
cargo +nightly fuzz run fuzz_vfs_open -- -max_total_time=86400 -jobs=8

# Run with AddressSanitizer to detect memory errors
RUSTFLAGS="-Z sanitizer=address" cargo +nightly fuzz run fuzz_vfs_read_write

# Run with ThreadSanitizer to detect data races
RUSTFLAGS="-Z sanitizer=thread" cargo +nightly fuzz run fuzz_vfs_concurrent

# Collect coverage
cargo +nightly fuzz coverage fuzz_vfs_operations
```

### 1.2 Page Corruption Detection

**Test File:** `tests/corruption_pages.rs`

```rust
#[test]
fn test_corrupted_page_header() {
    let mut cart = Cartridge::create("corrupt-test", "Corrupt Test").unwrap();
    cart.write("/file.txt", b"test data").unwrap();
    drop(cart);

    // Corrupt page header at specific offset
    corrupt_page_header("corrupt-test.cart", page_number: 3, offset: 8);

    // Should detect corruption on read
    let cart = Cartridge::open("corrupt-test.cart").unwrap();
    let result = cart.read("/file.txt");
    assert!(matches!(result, Err(CartridgeError::CorruptedPageHeader { .. })));
}

#[test]
fn test_invalid_page_type() {
    let mut cart = Cartridge::create("corrupt-test", "Corrupt Test").unwrap();
    cart.write("/file.txt", b"test data").unwrap();
    drop(cart);

    // Set page type to invalid value (99)
    modify_page_type("corrupt-test.cart", page_number: 3, new_type: 99);

    let cart = Cartridge::open("corrupt-test.cart").unwrap();
    let result = cart.read("/file.txt");
    assert!(matches!(result, Err(CartridgeError::InvalidPageType(99))));
}

#[test]
fn test_page_checksum_mismatch() {
    let mut cart = Cartridge::create("corrupt-test", "Corrupt Test").unwrap();
    cart.write("/file.txt", b"test data").unwrap();
    drop(cart);

    // Enable checksums
    let mut cart = Cartridge::open("corrupt-test.cart").unwrap();
    cart.header_mut().enable_checksums = true;
    drop(cart);

    // Corrupt page data (not header, to preserve checksum field)
    corrupt_page_data("corrupt-test.cart", page_number: 3, offset: 100);

    let cart = Cartridge::open("corrupt-test.cart").unwrap();
    let result = cart.read("/file.txt");
    assert!(matches!(result, Err(CartridgeError::ChecksumMismatch { .. })));
}

#[test]
fn test_truncated_pages() {
    let mut cart = Cartridge::create("corrupt-test", "Corrupt Test").unwrap();
    cart.write("/large.bin", &vec![0xAB; 100 * 1024]).unwrap();
    drop(cart);

    // Truncate file mid-page
    let file_size = std::fs::metadata("corrupt-test.cart").unwrap().len();
    truncate_file("corrupt-test.cart", file_size - 2048); // Cut off half of last page

    let cart = Cartridge::open("corrupt-test.cart").unwrap();
    let result = cart.read("/large.bin");
    assert!(matches!(result, Err(CartridgeError::TruncatedPage { .. })));
}

#[test]
fn test_page_sequence_validation() {
    // Pages should be sequentially consistent
    let mut cart = Cartridge::create("corrupt-test", "Corrupt Test").unwrap();
    cart.write("/file.txt", b"test data").unwrap();
    drop(cart);

    // Swap two pages
    swap_pages("corrupt-test.cart", page1: 3, page2: 5);

    let cart = Cartridge::open("corrupt-test.cart").unwrap();
    let result = cart.read("/file.txt");
    assert!(result.is_err()); // Should detect sequence mismatch
}
```

### 1.3 B-Tree Catalog Corruption

**Test File:** `tests/corruption_btree.rs`

```rust
#[test]
fn test_corrupted_btree_node() {
    let mut cart = Cartridge::create("corrupt-btree", "Corrupt BTree").unwrap();

    // Add files to build B-tree
    for i in 0..100 {
        cart.write(&format!("/file{}.txt", i), b"data").unwrap();
    }
    drop(cart);

    // Corrupt B-tree catalog page
    corrupt_btree_catalog_page("corrupt-btree.cart", btree_root_page: 1);

    let cart = Cartridge::open("corrupt-btree.cart").unwrap();
    let result = cart.list("/");
    assert!(matches!(result, Err(CartridgeError::CorruptedCatalog { .. })));
}

#[test]
fn test_btree_infinite_loop() {
    let mut cart = Cartridge::create("corrupt-btree", "Corrupt BTree").unwrap();

    for i in 0..50 {
        cart.write(&format!("/file{}.txt", i), b"data").unwrap();
    }
    drop(cart);

    // Create circular reference in B-tree (parent → child → parent)
    create_btree_cycle("corrupt-btree.cart");

    let cart = Cartridge::open("corrupt-btree.cart").unwrap();
    let result = cart.list("/");
    assert!(matches!(result, Err(CartridgeError::CatalogCycleDetected)));
}

#[test]
fn test_btree_missing_entries() {
    let mut cart = Cartridge::create("corrupt-btree", "Corrupt BTree").unwrap();

    for i in 0..20 {
        cart.write(&format!("/file{}.txt", i), b"data").unwrap();
    }
    drop(cart);

    // Delete random entries from B-tree
    delete_random_btree_entries("corrupt-btree.cart", count: 5);

    let cart = Cartridge::open("corrupt-btree.cart").unwrap();
    let all_files = cart.list("/").unwrap();

    // Should have < 20 files (some missing)
    assert!(all_files.len() < 20);

    // Attempting to read missing files should fail
    for i in 0..20 {
        let result = cart.read(&format!("/file{}.txt", i));
        // Some will succeed, some will fail
    }
}

#[test]
fn test_btree_key_ordering_violation() {
    let mut cart = Cartridge::create("corrupt-btree", "Corrupt BTree").unwrap();

    for i in 0..30 {
        cart.write(&format!("/file{:03}.txt", i), b"data").unwrap();
    }
    drop(cart);

    // Violate B-tree key ordering
    swap_btree_keys("corrupt-btree.cart", key1: "file005.txt", key2: "file020.txt");

    let cart = Cartridge::open("corrupt-btree.cart").unwrap();

    // Searches may fail or return wrong results
    let result = cart.metadata("/file010.txt");
    // Behavior is undefined with corrupted B-tree
}
```

### 1.4 Allocator Corruption Detection

**Test File:** `tests/corruption_allocators.rs`

```rust
#[test]
fn test_bitmap_allocator_corruption() {
    let mut cart = Cartridge::create("corrupt-alloc", "Corrupt Alloc").unwrap();

    // Allocate some blocks via small files
    for i in 0..10 {
        cart.write(&format!("/small{}.txt", i), &vec![i as u8; 10 * 1024]).unwrap();
    }
    drop(cart);

    // Corrupt bitmap allocator data
    corrupt_bitmap_allocator("corrupt-alloc.cart");

    let mut cart = Cartridge::open("corrupt-alloc.cart").unwrap();

    // Allocating new file may reuse already-allocated blocks
    let result = cart.write("/new.txt", b"new data");

    // May succeed but cause data corruption
    // Better: implement allocator verification
}

#[test]
fn test_extent_allocator_corruption() {
    let mut cart = Cartridge::create("corrupt-extent", "Corrupt Extent").unwrap();

    // Allocate via large files (extent allocator)
    cart.write("/large1.bin", &vec![0xAB; 512 * 1024]).unwrap();
    cart.write("/large2.bin", &vec![0xCD; 512 * 1024]).unwrap();
    drop(cart);

    // Corrupt extent free list
    corrupt_extent_allocator("corrupt-extent.cart");

    let mut cart = Cartridge::open("corrupt-extent.cart").unwrap();
    let result = cart.write("/large3.bin", &vec![0xEF; 512 * 1024]);

    // May allocate overlapping blocks
}

#[test]
fn test_hybrid_allocator_sync_mismatch() {
    let mut cart = Cartridge::create("corrupt-hybrid", "Corrupt Hybrid").unwrap();

    // Mix small and large files
    cart.write("/small.txt", &vec![0x01; 10 * 1024]).unwrap();
    cart.write("/large.bin", &vec![0x02; 512 * 1024]).unwrap();
    drop(cart);

    // Cause bitmap/extent sync mismatch
    desync_hybrid_allocators("corrupt-hybrid.cart");

    let mut cart = Cartridge::open("corrupt-hybrid.cart").unwrap();

    // Both allocators may claim same blocks
    cart.write("/new_small.txt", &vec![0x03; 10 * 1024]).unwrap();
    cart.write("/new_large.bin", &vec![0x04; 512 * 1024]).unwrap();

    // Verify no block collision
    verify_no_block_overlap(&cart);
}

#[test]
fn test_free_blocks_counter_mismatch() {
    let mut cart = Cartridge::create("corrupt-free", "Corrupt Free").unwrap();
    cart.write("/file.txt", b"data").unwrap();
    drop(cart);

    // Modify header.free_blocks to be incorrect
    modify_free_blocks_counter("corrupt-free.cart", incorrect_value: 999999);

    let mut cart = Cartridge::open("corrupt-free.cart").unwrap();

    // Verify allocator recalculates correct free_blocks
    let actual_free = cart.header().free_blocks;
    let allocator_free = cart.allocator.free_blocks();
    assert_eq!(actual_free, allocator_free as u64);
}
```

### 1.5 Allocator Property-Based Tests

**Test File:** `tests/property_allocator.rs`

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_allocator_free_blocks_accurate(
        allocations in prop::collection::vec((1usize..1024*1024, any::<u8>()), 1..100)
    ) {
        let mut cart = Cartridge::create("prop-test", "Prop Test").unwrap();

        let initial_free = cart.header().free_blocks;
        let mut allocated_blocks = 0u64;

        // Allocate files
        for (size, byte) in &allocations {
            let data = vec![*byte; *size];
            cart.write(&format!("/file_{}.bin", size), &data).unwrap();

            // Calculate blocks used
            let blocks_used = (*size as u64 + 4095) / 4096;
            allocated_blocks += blocks_used;
        }

        let final_free = cart.header().free_blocks;

        // Property: free blocks should decrease by allocated amount
        // (allowing for auto-growth)
        prop_assert!(final_free <= initial_free);
        prop_assert_eq!(cart.header().free_blocks, cart.allocator.free_blocks() as u64);
    }

    #[test]
    fn prop_allocator_no_double_allocation(
        file_count in 1usize..50,
        file_size in 4096usize..512*1024
    ) {
        let mut cart = Cartridge::create("prop-no-double", "Prop No Double").unwrap();

        let mut all_blocks = std::collections::HashSet::new();

        for i in 0..file_count {
            let data = vec![i as u8; file_size];
            cart.write(&format!("/file{}.bin", i), &data).unwrap();

            // Get blocks allocated for this file
            let metadata = cart.metadata(&format!("/file{}.bin", i)).unwrap();

            // Ensure no block is allocated twice
            for block in &metadata.blocks {
                prop_assert!(!all_blocks.contains(block), "Block {} allocated twice!", block);
                all_blocks.insert(*block);
            }
        }
    }

    #[test]
    fn prop_allocator_growth_maintains_invariants(
        operations in prop::collection::vec(1usize..512*1024, 1..100)
    ) {
        let mut cart = Cartridge::create("prop-growth", "Prop Growth").unwrap();

        for (i, size) in operations.iter().enumerate() {
            let data = vec![i as u8; *size];
            cart.write(&format!("/file{}.bin", i), &data).unwrap();

            // Invariant: free_blocks always matches allocator
            prop_assert_eq!(
                cart.header().free_blocks,
                cart.allocator.free_blocks() as u64
            );

            // Invariant: total_blocks >= used_blocks + free_blocks
            let total = cart.header().total_blocks;
            let free = cart.header().free_blocks;
            prop_assert!(total >= free);
        }
    }
}
```

### 1.6 Crash Recovery During Auto-Growth

**Test File:** `tests/crash_recovery_growth.rs`

```rust
#[test]
fn test_crash_during_growth() {
    for crash_point in 0..100 {
        // Create cartridge
        let mut cart = Cartridge::create("crash-growth", "Crash Growth").unwrap();

        // Trigger growth
        let large_data = vec![0xAB; 512 * 1024];

        // Simulate crash at random point during growth
        if random_percentage() < crash_point {
            // Complete the write
            cart.write("/large.bin", &large_data).unwrap();
        } else {
            // Crash mid-growth - drop without flushing
            drop(cart);
        }

        // Reopen - should either succeed with data or fail gracefully
        match Cartridge::open("crash-growth.cart") {
            Ok(cart) => {
                // If opened successfully, file should either exist completely or not at all
                match cart.read("/large.bin") {
                    Ok(data) => assert_eq!(data, large_data),
                    Err(_) => {}, // File doesn't exist - acceptable
                }
            }
            Err(_) => {
                // Container corrupted - this is acceptable for abrupt crash
            }
        }
    }
}

#[test]
fn test_concurrent_growth_conflict() {
    let cart = Arc::new(RwLock::new(Cartridge::create("concurrent-growth", "Concurrent Growth").unwrap()));

    // Two threads try to trigger growth simultaneously
    let handles: Vec<_> = (0..2).map(|i| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            let mut c = cart_clone.write();
            let data = vec![i as u8; 512 * 1024];
            c.write(&format!("/large{}.bin", i), &data).unwrap();
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    // Verify both files exist and allocator is consistent
    let c = cart.read();
    assert_eq!(c.read("/large0.bin").unwrap().len(), 512 * 1024);
    assert_eq!(c.read("/large1.bin").unwrap().len(), 512 * 1024);
    assert_eq!(c.header().free_blocks, c.allocator.free_blocks() as u64);
}

#[test]
fn test_growth_near_max_limit() {
    // Start with container near max size (default 40GB)
    let mut cart = Cartridge::create("near-max", "Near Max").unwrap();

    // Manually set to large size (simulate growth to near-max)
    // ... (requires internal API access)

    // Attempt to grow beyond limit
    let huge_data = vec![0xFF; 1024 * 1024 * 1024]; // 1GB
    let result = cart.write("/huge.bin", &huge_data);

    // Should fail with OutOfSpace, not panic
    assert!(matches!(result, Err(CartridgeError::OutOfSpace)));

    // Container should still be valid
    assert!(cart.list("/").is_ok());
}
```

---

## Phase 2: Concurrency & Durability

**Timeline:** 2 weeks
**Priority:** HIGH - Production stability
**Focus:** Thread safety, concurrent access, snapshot consistency

### 2.1 Concurrent Readers/Writers Stress Test

**Test File:** `tests/concurrency_stress.rs`

```rust
#[test]
fn test_100_concurrent_readers_10_writers() {
    let cart = Arc::new(RwLock::new(Cartridge::create("concurrent-stress", "Concurrent Stress").unwrap()));

    // Pre-populate with files
    {
        let mut c = cart.write();
        for i in 0..100 {
            c.write(&format!("/file{}.txt", i), format!("data{}", i).as_bytes()).unwrap();
        }
    }

    let handles: Vec<_> = (0..110).map(|thread_id| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            if thread_id < 10 {
                // Writer thread
                for i in 0..1000 {
                    let mut c = cart_clone.write();
                    c.write(&format!("/writer{}_{}.txt", thread_id, i), b"new data").unwrap();
                }
            } else {
                // Reader thread
                for _ in 0..10000 {
                    let c = cart_clone.read();
                    let idx = rand::random::<usize>() % 100;
                    let _ = c.read(&format!("/file{}.txt", idx));
                }
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    // Verify integrity
    let c = cart.read();
    assert_eq!(c.header().free_blocks, c.allocator.free_blocks() as u64);
}

#[test]
fn test_reader_writer_lock_fairness() {
    // Ensure writers don't starve readers (or vice versa)
    let cart = Arc::new(RwLock::new(Cartridge::create("fairness", "Fairness").unwrap()));

    let read_count = Arc::new(AtomicUsize::new(0));
    let write_count = Arc::new(AtomicUsize::new(0));

    // Continuous readers
    let reader_handles: Vec<_> = (0..50).map(|_| {
        let cart_clone = cart.clone();
        let read_count_clone = read_count.clone();
        std::thread::spawn(move || {
            for _ in 0..100 {
                let c = cart_clone.read();
                let _ = c.list("/");
                read_count_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
    }).collect();

    // Continuous writers
    let writer_handles: Vec<_> = (0..10).map(|thread_id| {
        let cart_clone = cart.clone();
        let write_count_clone = write_count.clone();
        std::thread::spawn(move || {
            for i in 0..100 {
                let mut c = cart_clone.write();
                c.write(&format!("/w{}_{}.txt", thread_id, i), b"data").unwrap();
                write_count_clone.fetch_add(1, Ordering::Relaxed);
            }
        })
    }).collect();

    for h in reader_handles.into_iter().chain(writer_handles) {
        h.join().unwrap();
    }

    // Both readers and writers should have made progress
    assert_eq!(read_count.load(Ordering::Relaxed), 50 * 100);
    assert_eq!(write_count.load(Ordering::Relaxed), 10 * 100);
}

#[test]
fn test_concurrent_deletes() {
    let cart = Arc::new(RwLock::new(Cartridge::create("concurrent-delete", "Concurrent Delete").unwrap()));

    // Pre-populate
    {
        let mut c = cart.write();
        for i in 0..1000 {
            c.write(&format!("/file{}.txt", i), b"data").unwrap();
        }
    }

    // Multiple threads delete different files
    let handles: Vec<_> = (0..10).map(|thread_id| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for i in 0..100 {
                let file_idx = thread_id * 100 + i;
                let mut c = cart_clone.write();
                c.delete(&format!("/file{}.txt", file_idx)).unwrap();
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    // Verify all deleted
    let c = cart.read();
    let remaining = c.list("/").unwrap();
    assert_eq!(remaining.len(), 0);

    // Allocator should have freed blocks
    assert!(c.header().free_blocks > 0);
}
```

### 2.2 SQLite VFS Multi-Connection Tests

**Test File:** `tests/concurrency_vfs_multi_connection.rs`

```rust
#[test]
fn test_multiple_vfs_connections_same_database() {
    let mut cart = Cartridge::create("vfs-multi", "VFS Multi").unwrap();

    // Create test database
    create_test_database_in_cartridge(&mut cart, "/db.sqlite", rows: 1000);

    unsafe {
        cartridge_rs::vfs::register_vfs();

        let uri = "file:///db.sqlite?vfs=cartridge&cartridge=vfs-multi.cart";

        // Open 10 concurrent connections
        let connections: Vec<_> = (0..10)
            .map(|_| Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap())
            .collect();

        // Each connection executes queries
        let handles: Vec<_> = connections.into_iter().enumerate().map(|(i, conn)| {
            std::thread::spawn(move || {
                for j in 0..100 {
                    let id = (i * 100 + j) % 1000;
                    let result: i64 = conn.query_row(
                        "SELECT value FROM test WHERE id = ?",
                        [id],
                        |row| row.get(0)
                    ).unwrap();
                    assert_eq!(result, id as i64);
                }
            })
        }).collect();

        for h in handles {
            h.join().unwrap();
        }

        cartridge_rs::vfs::unregister_vfs();
    }
}

#[test]
fn test_vfs_read_write_contention() {
    let mut cart = Cartridge::create("vfs-rw-contention", "VFS RW Contention").unwrap();
    cart.write("/db.sqlite", b"SQLite format 3\0").unwrap();

    unsafe {
        cartridge_rs::vfs::register_vfs();

        let uri = "file:///db.sqlite?vfs=cartridge&cartridge=vfs-rw-contention.cart";

        // One writer
        let writer_handle = std::thread::spawn(|| {
            let conn = Connection::open(uri).unwrap();
            conn.execute("CREATE TABLE test(id INTEGER, value TEXT)", []).unwrap();

            for i in 0..1000 {
                conn.execute("INSERT INTO test VALUES (?, ?)", [i, format!("value{}", i)]).unwrap();
            }
        });

        // Multiple readers (should wait for writes to complete)
        std::thread::sleep(std::time::Duration::from_millis(100)); // Let writer start

        let reader_handles: Vec<_> = (0..5).map(|_| {
            std::thread::spawn(|| {
                let conn = Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();

                for _ in 0..100 {
                    let count: i64 = conn.query_row("SELECT COUNT(*) FROM test", [], |row| row.get(0)).unwrap_or(0);
                    // Count should be between 0 and 1000
                    assert!(count >= 0 && count <= 1000);
                }
            })
        }).collect();

        writer_handle.join().unwrap();
        for h in reader_handles {
            h.join().unwrap();
        }

        cartridge_rs::vfs::unregister_vfs();
    }
}

#[test]
fn test_vfs_connection_cleanup_under_load() {
    unsafe {
        cartridge_rs::vfs::register_vfs();

        // Rapidly create and destroy connections
        for iteration in 0..100 {
            let mut cart = Cartridge::create(&format!("vfs-cleanup-{}", iteration), "VFS Cleanup").unwrap();
            cart.write("/db.sqlite", b"SQLite format 3\0").unwrap();
            drop(cart);

            let uri = format!("file:///db.sqlite?vfs=cartridge&cartridge=vfs-cleanup-{}.cart", iteration);

            // Open, use, close
            {
                let conn = Connection::open(&uri).unwrap();
                conn.execute("CREATE TABLE t(x)", []).ok();
                // Connection dropped here
            }

            // Verify temp files cleaned up
            assert_temp_files_cleaned();
        }

        cartridge_rs::vfs::unregister_vfs();
    }
}
```

### 2.3 Snapshot Consistency Tests

**Test File:** `tests/snapshot_consistency.rs`

```rust
#[test]
fn test_snapshot_concurrent_writes() {
    let mut cart = Cartridge::create("snapshot-concurrent", "Snapshot Concurrent").unwrap();

    // Initial data
    for i in 0..100 {
        cart.write(&format!("/file{}.txt", i), format!("v0_{}", i).as_bytes()).unwrap();
    }

    // Create snapshot
    let snapshot_id = cart.create_snapshot(
        "snapshot1".to_string(),
        "Test snapshot".to_string(),
        Path::new("./snapshots")
    ).unwrap();

    // Modify files after snapshot
    for i in 0..100 {
        cart.write(&format!("/file{}.txt", i), format!("v1_{}", i).as_bytes()).unwrap();
    }

    // Restore snapshot
    cart.restore_snapshot(snapshot_id, Path::new("./snapshots")).unwrap();

    // Verify data reverted
    for i in 0..100 {
        let data = cart.read(&format!("/file{}.txt", i)).unwrap();
        assert_eq!(data, format!("v0_{}", i).as_bytes());
    }
}

#[test]
fn test_snapshot_with_simultaneous_reads() {
    let cart = Arc::new(RwLock::new(Cartridge::create("snapshot-reads", "Snapshot Reads").unwrap()));

    // Pre-populate
    {
        let mut c = cart.write();
        for i in 0..50 {
            c.write(&format!("/file{}.txt", i), b"original").unwrap();
        }
    }

    // Snapshot creation thread
    let cart_clone1 = cart.clone();
    let snapshot_handle = std::thread::spawn(move || {
        let mut c = cart_clone1.write();
        c.create_snapshot(
            "snap1".to_string(),
            "Test".to_string(),
            Path::new("./snapshots")
        )
    });

    // Reader threads
    let reader_handles: Vec<_> = (0..10).map(|_| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for _ in 0..100 {
                let c = cart_clone.read();
                let idx = rand::random::<usize>() % 50;
                let _ = c.read(&format!("/file{}.txt", idx));
            }
        })
    }).collect();

    snapshot_handle.join().unwrap().unwrap();
    for h in reader_handles {
        h.join().unwrap();
    }
}

#[test]
fn test_snapshot_cow_page_tracking() {
    let mut cart = Cartridge::create("snapshot-cow", "Snapshot COW").unwrap();

    cart.write("/file.txt", b"original data").unwrap();

    let snapshot_id = cart.create_snapshot("s1".to_string(), "Test".to_string(), Path::new("./snapshots")).unwrap();

    // Modify file (should trigger COW)
    cart.write("/file.txt", b"modified data").unwrap();

    // Original snapshot should have original data
    let snapshot_path = Path::new("./snapshots").join(format!("snapshot_{}.cart", snapshot_id));
    let snapshot_cart = Cartridge::open(&snapshot_path).unwrap();
    let snapshot_data = snapshot_cart.read("/file.txt").unwrap();
    assert_eq!(snapshot_data, b"original data");

    // Current container should have modified data
    let current_data = cart.read("/file.txt").unwrap();
    assert_eq!(current_data, b"modified data");
}

#[test]
fn test_snapshot_chain_depth() {
    let mut cart = Cartridge::create("snapshot-chain", "Snapshot Chain").unwrap();

    // Create chain of 10 snapshots
    for i in 0..10 {
        cart.write(&format!("/file{}.txt", i), format!("data{}", i).as_bytes()).unwrap();
        cart.create_snapshot(
            format!("snap{}", i),
            format!("Snapshot {}", i),
            Path::new("./snapshots")
        ).unwrap();
    }

    // Verify each snapshot has correct data
    for i in 0..10 {
        let snapshot_path = Path::new("./snapshots").join(format!("snapshot_{}.cart", i));
        let snap = Cartridge::open(&snapshot_path).unwrap();

        // Should have files 0..=i
        for j in 0..=i {
            let data = snap.read(&format!("/file{}.txt", j)).unwrap();
            assert_eq!(data, format!("data{}", j).as_bytes());
        }

        // Should NOT have files > i
        for j in (i+1)..10 {
            assert!(snap.read(&format!("/file{}.txt", j)).is_err());
        }
    }
}
```

### 2.4 IAM Policy Cache Race Conditions

**Test File:** `tests/iam_policy_race_conditions.rs`

```rust
#[test]
fn test_iam_cache_concurrent_updates() {
    let mut cart = Cartridge::create("iam-cache-race", "IAM Cache Race").unwrap();

    // Set initial policy
    let policy = r#"
    {
        "statements": [
            {
                "effect": "allow",
                "actions": ["read"],
                "resources": ["/public/*"]
            }
        ]
    }
    "#;
    cart.set_iam_policy(policy).unwrap();

    let cart = Arc::new(RwLock::new(cart));

    // Multiple threads check permissions concurrently
    let handles: Vec<_> = (0..20).map(|thread_id| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for i in 0..1000 {
                let c = cart_clone.read();
                let allowed = c.check_permission(&format!("/public/file{}.txt", i), &Action::Read);
                assert!(allowed); // Should always be true
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_iam_cache_invalidation() {
    let mut cart = Cartridge::create("iam-invalidation", "IAM Invalidation").unwrap();

    // Initial policy: allow /public/*
    cart.set_iam_policy(r#"{"statements": [{"effect": "allow", "actions": ["read"], "resources": ["/public/*"]}]}"#).unwrap();

    let cart = Arc::new(RwLock::new(cart));

    // Reader thread
    let cart_clone1 = cart.clone();
    let reader_handle = std::thread::spawn(move || {
        let c = cart_clone1.read();
        for _ in 0..1000 {
            let _ = c.check_permission("/public/file.txt", &Action::Read);
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
    });

    // Policy updater thread
    let cart_clone2 = cart.clone();
    let updater_handle = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut c = cart_clone2.write();
        // Change policy to deny /public/*
        c.set_iam_policy(r#"{"statements": [{"effect": "deny", "actions": ["read"], "resources": ["/public/*"]}]}"#).unwrap();
    });

    reader_handle.join().unwrap();
    updater_handle.join().unwrap();

    // After update, access should be denied
    let c = cart.read();
    let allowed = c.check_permission("/public/file.txt", &Action::Read);
    assert!(!allowed);
}

#[test]
fn test_iam_cache_poisoning() {
    let mut cart = Cartridge::create("iam-poison", "IAM Poison").unwrap();
    cart.set_iam_policy(r#"{"statements": [{"effect": "allow", "actions": ["read"], "resources": ["/*"]}]}"#).unwrap();

    let cart = Arc::new(RwLock::new(cart));

    // Attempt to poison cache with rapid permission checks
    let handles: Vec<_> = (0..10).map(|thread_id| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for i in 0..10000 {
                let c = cart_clone.read();
                let path = format!("/file_{}_{}. txt", thread_id, i);
                let _ = c.check_permission(&path, &Action::Read);
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }

    // Cache should still be coherent
    let c = cart.read();
    assert!(c.check_permission("/any/file.txt", &Action::Read));
}
```

### 2.5 Buffer Pool Coherency

**Test File:** `tests/buffer_pool_coherency.rs`

```rust
#[test]
fn test_buffer_pool_concurrent_reads() {
    let mut cart = Cartridge::create("buffer-pool-reads", "Buffer Pool Reads").unwrap();

    // Create file
    cart.write("/cached.txt", b"cached data").unwrap();

    let cart = Arc::new(RwLock::new(cart));

    // Multiple readers of same file (should hit buffer pool cache)
    let handles: Vec<_> = (0..50).map(|_| {
        let cart_clone = cart.clone();
        std::thread::spawn(move || {
            for _ in 0..100 {
                let c = cart_clone.read();
                let data = c.read("/cached.txt").unwrap();
                assert_eq!(data, b"cached data");
            }
        })
    }).collect();

    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn test_buffer_pool_write_invalidation() {
    let cart = Arc::new(RwLock::new(Cartridge::create("buffer-pool-invalidate", "Buffer Pool Invalidate").unwrap()));

    {
        let mut c = cart.write();
        c.write("/file.txt", b"version 1").unwrap();
    }

    // Reader thread (caches page)
    let cart_clone1 = cart.clone();
    let reader_handle = std::thread::spawn(move || {
        let c = cart_clone1.read();
        let data1 = c.read("/file.txt").unwrap();
        assert_eq!(data1, b"version 1");

        std::thread::sleep(std::time::Duration::from_millis(100));

        // Read again - should see updated version
        let data2 = c.read("/file.txt").unwrap();
        // May still see v1 if cache not invalidated properly
    });

    // Writer thread (updates file, should invalidate cache)
    let cart_clone2 = cart.clone();
    let writer_handle = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut c = cart_clone2.write();
        c.write("/file.txt", b"version 2").unwrap();
    });

    reader_handle.join().unwrap();
    writer_handle.join().unwrap();

    // Final read should see version 2
    let c = cart.read();
    let final_data = c.read("/file.txt").unwrap();
    assert_eq!(final_data, b"version 2");
}
```

---

## Phase 3: Performance & Scale

**Timeline:** 1-2 weeks
**Priority:** MEDIUM - Production optimization
**Focus:** Large containers, extreme file counts, fragmentation validation

### 3.1 Extreme Load Tests

**Test File:** `tests/stress_extreme_scale.rs`

```rust
#[test]
#[ignore] // Run manually: cargo test --release test_100gb_container -- --ignored
fn test_100gb_container() {
    let mut cart = Cartridge::create("stress-100gb", "Stress 100GB").unwrap();

    // Create 100 x 1GB files
    for i in 0..100 {
        println!("Creating file {} of 100...", i + 1);
        let data = vec![i as u8; 1024 * 1024 * 1024]; // 1GB
        cart.write(&format!("/file{:03}.bin", i), &data).unwrap();

        // Verify free_blocks accounting
        assert_eq!(cart.header().free_blocks, cart.allocator.free_blocks() as u64);
    }

    // Verify container size
    let size = std::fs::metadata("stress-100gb.cart").unwrap().len();
    assert!(size > 100 * 1024 * 1024 * 1024); // > 100GB

    // Random access
    for _ in 0..100 {
        let idx = rand::random::<usize>() % 100;
        let data = cart.read(&format!("/file{:03}.bin", idx)).unwrap();
        assert_eq!(data.len(), 1024 * 1024 * 1024);
        assert_eq!(data[0], idx as u8);
    }
}

#[test]
#[ignore]
fn test_1m_files() {
    let mut cart = Cartridge::create("stress-1m-files", "Stress 1M Files").unwrap();

    println!("Creating 1,000,000 files...");
    for i in 0..1_000_000 {
        if i % 10_000 == 0 {
            println!("Progress: {} files", i);
        }

        let data = format!("file{}", i).repeat(10); // ~50 bytes
        cart.write(&format!("/f{}.txt", i), data.as_bytes()).unwrap();
    }

    println!("Verifying file count...");
    let all_files = cart.list("/").unwrap();
    assert_eq!(all_files.len(), 1_000_000);

    println!("Random access test...");
    for _ in 0..10_000 {
        let idx = rand::random::<usize>() % 1_000_000;
        let data = cart.read(&format!("/f{}.txt", idx)).unwrap();
        assert!(data.len() > 0);
    }
}

#[test]
#[ignore]
fn test_fragmentation_measurement() {
    let mut cart = Cartridge::create("stress-frag", "Stress Fragmentation").unwrap();

    // Create many files
    for i in 0..10_000 {
        let size = rand::random::<usize>() % (100 * 1024) + 1024; // 1KB-100KB
        cart.write(&format!("/file{}.bin", i), &vec![i as u8; size]).unwrap();
    }

    // Delete random 50%
    for i in (0..10_000).step_by(2) {
        cart.delete(&format!("/file{}.bin", i)).unwrap();
    }

    // Measure fragmentation
    let frag_score = cart.allocator.fragmentation_score();
    println!("Fragmentation score: {:.2}%", frag_score * 100.0);

    // Fragmentation should be measurable but not extreme
    assert!(frag_score < 0.8); // < 80% fragmented

    // Re-fill deleted space
    for i in 0..5_000 {
        let size = rand::random::<usize>() % (100 * 1024) + 1024;
        cart.write(&format!("/new{}.bin", i), &vec![0xFF; size]).unwrap();
    }

    // Verify allocator health
    assert_eq!(cart.header().free_blocks, cart.allocator.free_blocks() as u64);
}

#[test]
#[ignore]
fn test_max_auto_growth_limit() {
    let mut cart = Cartridge::create("stress-max-growth", "Stress Max Growth").unwrap();

    // Set low max_blocks for testing (default is 10M blocks = 40GB)
    cart.set_max_blocks(1000); // 4MB max

    // Fill until OutOfSpace
    let mut i = 0;
    loop {
        match cart.write(&format!("/file{}.bin", i), &vec![0xAB; 64 * 1024]) {
            Ok(_) => i += 1,
            Err(CartridgeError::OutOfSpace) => {
                println!("Reached capacity at {} files", i);
                break;
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    // Container should still be valid
    assert!(cart.list("/").is_ok());
    assert_eq!(cart.header().total_blocks as usize, 1000);
}
```

### 3.2 Auto-Growth Performance

**Benchmark:** `benches/auto_growth_performance.rs`

```rust
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use cartridge_rs::Cartridge;

fn bench_sequential_growth(c: &mut Criterion) {
    let mut group = c.benchmark_group("auto_growth");

    // Measure growth overhead at different stages
    for stage in [3, 6, 12, 24, 48, 96, 192] {
        group.bench_with_input(
            BenchmarkId::new("growth_to", stage),
            &stage,
            |b, &target_blocks| {
                b.iter(|| {
                    let mut cart = Cartridge::create("bench-growth", "Bench Growth").unwrap();

                    // Force growth to target
                    while cart.header().total_blocks < target_blocks {
                        let size = 512 * 1024; // 512KB to trigger extent allocator
                        cart.write(&format!("/file{}.bin", cart.header().total_blocks), &vec![0xAB; size]).unwrap();
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_hybrid_allocator_dispatch(c: &mut Criterion) {
    let mut group = c.benchmark_group("hybrid_allocator");

    group.bench_function("small_file_bitmap", |b| {
        b.iter(|| {
            let mut cart = Cartridge::create("bench-hybrid", "Bench Hybrid").unwrap();
            for i in 0..100 {
                cart.write(&format!("/small{}.txt", i), &vec![i as u8; 10 * 1024]).unwrap(); // 10KB
            }
        });
    });

    group.bench_function("large_file_extent", |b| {
        b.iter(|| {
            let mut cart = Cartridge::create("bench-hybrid", "Bench Hybrid").unwrap();
            for i in 0..10 {
                cart.write(&format!("/large{}.bin", i), &vec![i as u8; 512 * 1024]).unwrap(); // 512KB
            }
        });
    });

    group.bench_function("mixed_workload", |b| {
        b.iter(|| {
            let mut cart = Cartridge::create("bench-hybrid", "Bench Hybrid").unwrap();
            for i in 0..50 {
                cart.write(&format!("/small{}.txt", i), &vec![i as u8; 10 * 1024]).unwrap();
                cart.write(&format!("/large{}.bin", i), &vec![i as u8; 512 * 1024]).unwrap();
            }
        });
    });

    group.finish();
}

criterion_group!(benches, bench_sequential_growth, bench_hybrid_allocator_dispatch);
criterion_main!(benches);
```

### 3.3 Existing Benchmark Validation

**Verify all 8 existing benchmarks still pass and meet targets:**

```bash
# Run all benchmarks
cargo bench

# Expected baselines (to be validated):
# - allocation.rs: < 1μs per block allocation
# - buffer_pool.rs: > 1M cache hits/sec
# - comprehensive.rs: Full workflow < 100ms
# - iam_policy.rs: < 100μs per permission check
# - mixed_workload.rs: > 1000 ops/sec
# - pager_arc.rs: ARC cache hit rate > 80%
# - snapshots.rs: Snapshot creation < 1s for 1000 files
# - vfs_sqlite.rs: < 10% overhead vs native SQLite
```

---

## Phase 4: Advanced Features

**Timeline:** 1 week
**Priority:** MEDIUM
**Focus:** Snapshot edge cases, Engram freeze, audit logs

### 4.1 Snapshot Advanced Tests

**Test File:** `tests/snapshot_advanced.rs`

```rust
#[test]
fn test_snapshot_restore_idempotence() {
    // Restoring same snapshot multiple times should be idempotent
    let mut cart = Cartridge::create("snapshot-idempotent", "Snapshot Idempotent").unwrap();
    cart.write("/file.txt", b"original").unwrap();

    let snap_id = cart.create_snapshot("s1".to_string(), "Test".to_string(), Path::new("./snapshots")).unwrap();

    cart.write("/file.txt", b"modified").unwrap();

    // Restore once
    cart.restore_snapshot(snap_id, Path::new("./snapshots")).unwrap();
    let data1 = cart.read("/file.txt").unwrap();

    // Restore again
    cart.restore_snapshot(snap_id, Path::new("./snapshots")).unwrap();
    let data2 = cart.read("/file.txt").unwrap();

    assert_eq!(data1, data2);
    assert_eq!(data1, b"original");
}

#[test]
fn test_snapshot_metadata_integrity() {
    let mut cart = Cartridge::create("snapshot-metadata", "Snapshot Metadata").unwrap();

    for i in 0..100 {
        cart.write(&format!("/file{}.txt", i), b"data").unwrap();
    }

    let snap_id = cart.create_snapshot("s1".to_string(), "Test".to_string(), Path::new("./snapshots")).unwrap();

    // Verify snapshot metadata
    let snapshot_path = Path::new("./snapshots").join(format!("snapshot_{}.cart", snap_id));
    let snap_cart = Cartridge::open(&snapshot_path).unwrap();

    // All metadata should match
    for i in 0..100 {
        let orig_meta = cart.metadata(&format!("/file{}.txt", i)).unwrap();
        let snap_meta = snap_cart.metadata(&format!("/file{}.txt", i)).unwrap();

        assert_eq!(orig_meta.size, snap_meta.size);
        assert_eq!(orig_meta.file_type, snap_meta.file_type);
    }
}

#[test]
fn test_snapshot_with_deletes() {
    let mut cart = Cartridge::create("snapshot-deletes", "Snapshot Deletes").unwrap();

    for i in 0..50 {
        cart.write(&format!("/file{}.txt", i), b"data").unwrap();
    }

    let snap_id = cart.create_snapshot("s1".to_string(), "Test".to_string(), Path::new("./snapshots")).unwrap();

    // Delete half the files
    for i in 0..25 {
        cart.delete(&format!("/file{}.txt", i)).unwrap();
    }

    // Restore snapshot
    cart.restore_snapshot(snap_id, Path::new("./snapshots")).unwrap();

    // All files should be back
    let files = cart.list("/").unwrap();
    assert_eq!(files.len(), 50);
}
```

### 4.2 Engram Freeze Validation

**Test File:** `tests/engram_freeze_validation.rs`

```rust
#[test]
fn test_freeze_basic() {
    let mut cart = Cartridge::create("freeze-test", "Freeze Test").unwrap();

    for i in 0..100 {
        cart.write(&format!("/file{}.txt", i), format!("data{}", i).as_bytes()).unwrap();
    }

    // Freeze to engram
    let engram_path = Path::new("frozen.eng");
    cart.freeze_to_engram(engram_path).unwrap();

    // Verify engram
    let eng = engram_rs::Engram::open(engram_path).unwrap();

    for i in 0..100 {
        let data = eng.read(&format!("/file{}.txt", i)).unwrap();
        assert_eq!(data, format!("data{}", i).as_bytes());
    }
}

#[test]
fn test_freeze_large_container() {
    let mut cart = Cartridge::create("freeze-large", "Freeze Large").unwrap();

    // 10GB container
    for i in 0..100 {
        let data = vec![i as u8; 100 * 1024 * 1024]; // 100MB each
        cart.write(&format!("/large{}.bin", i), &data).unwrap();
    }

    // Freeze should succeed
    let engram_path = Path::new("frozen-large.eng");
    cart.freeze_to_engram(engram_path).unwrap();

    // Verify size
    let eng_size = std::fs::metadata(engram_path).unwrap().len();
    assert!(eng_size > 9 * 1024 * 1024 * 1024); // > 9GB (with compression)
}

#[test]
fn test_freeze_with_snapshots() {
    let mut cart = Cartridge::create("freeze-snapshots", "Freeze Snapshots").unwrap();

    cart.write("/file.txt", b"v1").unwrap();
    cart.create_snapshot("s1".to_string(), "V1".to_string(), Path::new("./snapshots")).unwrap();

    cart.write("/file.txt", b"v2").unwrap();
    cart.create_snapshot("s2".to_string(), "V2".to_string(), Path::new("./snapshots")).unwrap();

    // Freeze should capture current state (v2)
    cart.freeze_to_engram(Path::new("frozen-snap.eng")).unwrap();

    let eng = engram_rs::Engram::open("frozen-snap.eng").unwrap();
    let data = eng.read("/file.txt").unwrap();
    assert_eq!(data, b"v2");
}
```

### 4.3 Audit Log Tests

**Test File:** `tests/audit_log_integrity.rs`

```rust
#[test]
fn test_audit_log_under_high_load() {
    let mut cart = Cartridge::create("audit-load", "Audit Load").unwrap();
    cart.enable_audit_logging(true);

    // Perform 10,000 operations
    for i in 0..10_000 {
        cart.write(&format!("/file{}.txt", i), b"data").unwrap();
        if i % 2 == 0 {
            cart.delete(&format!("/file{}.txt", i)).unwrap();
        }
    }

    // Verify audit log
    let audit_entries = cart.get_audit_log();
    assert!(audit_entries.len() > 0);

    // Should have writes and deletes
    let writes = audit_entries.iter().filter(|e| matches!(e.operation, Operation::Create)).count();
    let deletes = audit_entries.iter().filter(|e| matches!(e.operation, Operation::Delete)).count();

    assert_eq!(writes, 10_000);
    assert_eq!(deletes, 5_000);
}

#[test]
fn test_audit_log_ring_buffer_wrapping() {
    let mut cart = Cartridge::create("audit-ring", "Audit Ring").unwrap();
    cart.enable_audit_logging(true);
    cart.set_audit_log_capacity(1000); // Small buffer

    // Exceed buffer capacity
    for i in 0..2000 {
        cart.write(&format!("/file{}.txt", i), b"data").unwrap();
    }

    let audit_entries = cart.get_audit_log();

    // Should have exactly 1000 entries (oldest discarded)
    assert_eq!(audit_entries.len(), 1000);

    // Newest entries should be from file1000-file1999
    assert!(audit_entries[0].path.contains("file1"));
}
```

---

## Phase 5: Security Audit

**Timeline:** 1 week
**Priority:** HIGH for production
**Focus:** Encryption security, IAM bypass attempts, memory safety

### 5.1 Encryption Security

**Test File:** `tests/security_encryption.rs`

```rust
#[test]
fn test_encryption_key_derivation() {
    // Weak passwords should still produce valid keys
    let mut cart1 = Cartridge::create_encrypted("enc1", "Enc1", "weak").unwrap();
    let mut cart2 = Cartridge::create_encrypted("enc2", "Enc2", "weak").unwrap();

    cart1.write("/file.txt", b"data1").unwrap();
    cart2.write("/file.txt", b"data2").unwrap();

    // Different nonces should produce different ciphertexts even with same password
    let data1 = std::fs::read("enc1.cart").unwrap();
    let data2 = std::fs::read("enc2.cart").unwrap();
    assert_ne!(data1, data2);
}

#[test]
fn test_encryption_nonce_uniqueness() {
    let mut seen_nonces = std::collections::HashSet::new();

    for i in 0..1000 {
        let cart = Cartridge::create_encrypted(&format!("enc{}", i), "Enc", "password").unwrap();
        let nonce = extract_encryption_nonce(&format!("enc{}.cart", i));

        assert!(!seen_nonces.contains(&nonce), "Nonce reused!");
        seen_nonces.insert(nonce);
    }
}

#[test]
fn test_wrong_decryption_key() {
    let mut cart = Cartridge::create_encrypted("enc-wrong-key", "Enc Wrong", "correct_password").unwrap();
    cart.write("/file.txt", b"secret").unwrap();
    drop(cart);

    // Attempt to open with wrong password
    let result = Cartridge::open_encrypted("enc-wrong-key.cart", "wrong_password");
    assert!(matches!(result, Err(CartridgeError::DecryptionFailed)));
}
```

### 5.2 IAM Bypass Attempts

**Test File:** `tests/security_iam_bypass.rs`

```rust
#[test]
fn test_iam_path_traversal() {
    let mut cart = Cartridge::create("iam-traversal", "IAM Traversal").unwrap();

    // Policy: allow /public/*, deny /private/*
    cart.set_iam_policy(r#"{
        "statements": [
            {"effect": "allow", "actions": ["read"], "resources": ["/public/*"]},
            {"effect": "deny", "actions": ["read"], "resources": ["/private/*"]}
        ]
    }"#).unwrap();

    // Attempt path traversal
    assert!(!cart.check_permission("/public/../private/secret.txt", &Action::Read));
    assert!(!cart.check_permission("/public/./../private/secret.txt", &Action::Read));
}

#[test]
fn test_iam_wildcard_bypass() {
    let mut cart = Cartridge::create("iam-wildcard", "IAM Wildcard").unwrap();

    cart.set_iam_policy(r#"{
        "statements": [
            {"effect": "allow", "actions": ["read"], "resources": ["/data/*.txt"]}
        ]
    }"#).unwrap();

    // Should NOT match nested paths
    assert!(!cart.check_permission("/data/subdir/file.txt", &Action::Read));

    // Should match direct children only
    assert!(cart.check_permission("/data/file.txt", &Action::Read));
}

#[test]
fn test_iam_deny_precedence() {
    let mut cart = Cartridge::create("iam-deny-prec", "IAM Deny Precedence").unwrap();

    cart.set_iam_policy(r#"{
        "statements": [
            {"effect": "allow", "actions": ["read"], "resources": ["/*"]},
            {"effect": "deny", "actions": ["read"], "resources": ["/secret.txt"]}
        ]
    }"#).unwrap();

    // Deny should override allow
    assert!(!cart.check_permission("/secret.txt", &Action::Read));
    assert!(cart.check_permission("/public.txt", &Action::Read));
}
```

### 5.3 Memory Safety Validation

**Commands:**
```bash
# Run with Miri (interpreter for detecting undefined behavior)
cargo +nightly miri test

# Run with AddressSanitizer
RUSTFLAGS="-Z sanitizer=address" cargo +nightly test

# Run with MemorySanitizer
RUSTFLAGS="-Z sanitizer=memory" cargo +nightly test

# Run with LeakSanitizer
RUSTFLAGS="-Z sanitizer=leak" cargo +nightly test

# Run with ThreadSanitizer
RUSTFLAGS="-Z sanitizer=thread" cargo +nightly test
```

**Test File:** `tests/memory_safety.rs`

```rust
#[test]
fn test_no_memory_leaks_vfs() {
    // Use LeakSanitizer to detect leaks
    for _ in 0..1000 {
        let mut cart = Cartridge::create("leak-test", "Leak Test").unwrap();
        cart.write("/db.sqlite", b"SQLite format 3\0").unwrap();

        unsafe {
            cartridge_rs::vfs::register_vfs();
            let uri = "file:///db.sqlite?vfs=cartridge&cartridge=leak-test.cart";
            let conn = rusqlite::Connection::open(uri).unwrap();
            drop(conn);
            cartridge_rs::vfs::unregister_vfs();
        }

        drop(cart);
    }
    // LeakSanitizer will report any leaks
}

#[test]
fn test_no_use_after_free() {
    // Miri/ASan will detect use-after-free
    let mut cart = Cartridge::create("uaf-test", "UAF Test").unwrap();
    cart.write("/file.txt", b"data").unwrap();

    let data_ref = cart.read("/file.txt").unwrap();
    drop(cart);

    // Should not access data_ref after cart dropped
    // (This test intentionally doesn't - just checking Miri catches it if we did)
}
```

---

## Phase 6: VFS FFI Unsafe Code Validation

**Timeline:** 1-2 weeks
**Priority:** CRITICAL for production SQLite VFS usage
**Focus:** Comprehensive unsafe FFI testing, memory safety, SQLite integration

### 6.1 VFS FFI Integration Tests

**Rationale:** The VFS layer contains 29 unsafe blocks interfacing with SQLite C API. These require specialized testing with actual SQLite connections and sanitizers to validate memory safety.

**Test File:** `tests/vfs_ffi_integration.rs`

```rust
use cartridge_rs::Cartridge;
use rusqlite::{Connection, OpenFlags};
use std::sync::Arc;
use parking_lot::Mutex;

#[test]
fn test_vfs_actual_sqlite_connection() {
    let mut cart = Cartridge::create("vfs-ffi-test", "VFS FFI Test").unwrap();
    cart.write("/db.sqlite", b"").unwrap();

    let cart_arc = Arc::new(Mutex::new(cart));

    unsafe {
        // Register VFS with actual SQLite
        cartridge_rs::core::vfs::register_vfs(cart_arc.clone()).unwrap();

        let uri = "file:///db.sqlite?vfs=cartridge&cartridge=vfs-ffi-test.cart";

        // Open actual SQLite connection through VFS
        let conn = Connection::open_with_flags(
            uri,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
        ).unwrap();

        // Execute real SQL operations
        conn.execute("CREATE TABLE test(id INTEGER PRIMARY KEY, value TEXT)", []).unwrap();
        conn.execute("INSERT INTO test VALUES (1, 'hello')", []).unwrap();
        conn.execute("INSERT INTO test VALUES (2, 'world')", []).unwrap();

        // Read back
        let value: String = conn.query_row(
            "SELECT value FROM test WHERE id = 1",
            [],
            |row| row.get(0)
        ).unwrap();
        assert_eq!(value, "hello");

        drop(conn);
        cartridge_rs::core::vfs::unregister_vfs().unwrap();
    }

    std::fs::remove_file("vfs-ffi-test.cart").ok();
}

#[test]
fn test_vfs_concurrent_connections() {
    let mut cart = Cartridge::create("vfs-concurrent", "VFS Concurrent").unwrap();
    cart.write("/db.sqlite", b"").unwrap();

    let cart_arc = Arc::new(Mutex::new(cart));

    unsafe {
        cartridge_rs::core::vfs::register_vfs(cart_arc.clone()).unwrap();

        let uri = "file:///db.sqlite?vfs=cartridge&cartridge=vfs-concurrent.cart";

        // Open multiple connections
        let connections: Vec<_> = (0..5)
            .map(|_| Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READONLY).unwrap())
            .collect();

        // Concurrent reads
        let handles: Vec<_> = connections.into_iter().map(|conn| {
            std::thread::spawn(move || {
                for _ in 0..100 {
                    let _: i64 = conn.query_row("SELECT 1", [], |row| row.get(0)).unwrap();
                }
            })
        }).collect();

        for h in handles {
            h.join().unwrap();
        }

        cartridge_rs::core::vfs::unregister_vfs().unwrap();
    }

    std::fs::remove_file("vfs-concurrent.cart").ok();
}

#[test]
fn test_vfs_locking_protocol() {
    // Test SQLite locking protocol through VFS
    let mut cart = Cartridge::create("vfs-locking", "VFS Locking").unwrap();
    cart.write("/db.sqlite", b"").unwrap();

    let cart_arc = Arc::new(Mutex::new(cart));

    unsafe {
        cartridge_rs::core::vfs::register_vfs(cart_arc.clone()).unwrap();

        let uri = "file:///db.sqlite?vfs=cartridge&cartridge=vfs-locking.cart";

        // Writer connection
        let writer = Connection::open_with_flags(
            uri,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
        ).unwrap();

        writer.execute("CREATE TABLE test(x)", []).unwrap();
        writer.execute("BEGIN EXCLUSIVE", []).unwrap();
        writer.execute("INSERT INTO test VALUES (1)", []).unwrap();

        // Reader should be blocked or handle gracefully
        let reader = Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READONLY).unwrap();
        // May timeout or succeed depending on lock handling

        writer.execute("COMMIT", []).unwrap();
        drop(writer);
        drop(reader);

        cartridge_rs::core::vfs::unregister_vfs().unwrap();
    }

    std::fs::remove_file("vfs-locking.cart").ok();
}

#[test]
fn test_vfs_uri_parameter_parsing() {
    // Test various URI formats
    let test_cases = vec![
        "file:///db.sqlite?vfs=cartridge&cartridge=test.cart",
        "file:///path/to/db.sqlite?vfs=cartridge&cartridge=/abs/path/test.cart",
        "file:///db.sqlite?mode=ro&vfs=cartridge&cartridge=test.cart",
    ];

    for uri in test_cases {
        // Verify URI parsing doesn't crash
        let _ = Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READONLY);
    }
}
```

### 6.2 VFS FFI Fuzzing (Actual SQLite Operations)

**File:** `fuzz/fuzz_targets/fuzz_vfs_sqlite_operations.rs`

```rust
#![no_main]
use libfuzzer_sys::{fuzz_target, arbitrary::{Arbitrary, Unstructured}};
use cartridge_rs::Cartridge;
use rusqlite::Connection;
use std::sync::Arc;
use parking_lot::Mutex;

#[derive(Debug, Arbitrary)]
enum SqlOperation {
    CreateTable { name: String },
    Insert { value: u32 },
    Select,
    Update { id: u32, value: u32 },
    Delete { id: u32 },
    Vacuum,
    Pragma { setting: String },
}

fuzz_target!(|input: &[u8]| {
    let mut u = Unstructured::new(input);

    let ops: Vec<SqlOperation> = match u.arbitrary() {
        Ok(ops) => ops,
        Err(_) => return,
    };

    if ops.is_empty() {
        return;
    }

    let slug = format!("fuzz-sql-{}", std::process::id());
    let mut cart = match Cartridge::create(&slug, "Fuzz SQL") {
        Ok(c) => c,
        Err(_) => return,
    };

    if cart.write("/db.sqlite", b"").is_err() {
        return;
    }

    let cart_arc = Arc::new(Mutex::new(cart));

    unsafe {
        if cartridge_rs::core::vfs::register_vfs(cart_arc.clone()).is_err() {
            return;
        }

        let uri = format!("file:///db.sqlite?vfs=cartridge&cartridge={}.cart", slug);

        if let Ok(conn) = Connection::open(&uri) {
            // Execute fuzzed SQL operations
            for op in ops.iter().take(20) {
                match op {
                    SqlOperation::CreateTable { name } => {
                        let _ = conn.execute(&format!("CREATE TABLE IF NOT EXISTS t_{} (id INTEGER, val INTEGER)", name), []);
                    }
                    SqlOperation::Insert { value } => {
                        let _ = conn.execute("INSERT INTO t_default VALUES (?, ?)", [value, value]);
                    }
                    SqlOperation::Select => {
                        let _ = conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |_| Ok(()));
                    }
                    SqlOperation::Update { id, value } => {
                        let _ = conn.execute("UPDATE t_default SET val = ? WHERE id = ?", [value, id]);
                    }
                    SqlOperation::Delete { id } => {
                        let _ = conn.execute("DELETE FROM t_default WHERE id = ?", [id]);
                    }
                    SqlOperation::Vacuum => {
                        let _ = conn.execute("VACUUM", []);
                    }
                    SqlOperation::Pragma { setting } => {
                        let _ = conn.execute(&format!("PRAGMA {}", setting), []);
                    }
                }
            }
        }

        let _ = cartridge_rs::core::vfs::unregister_vfs();
    }

    std::fs::remove_file(format!("{}.cart", slug)).ok();
});
```

### 6.3 Memory Safety Validation (Sanitizers)

**Commands to run:**

```bash
# AddressSanitizer - detects memory errors, buffer overflows, use-after-free
RUSTFLAGS="-Z sanitizer=address" cargo +nightly test vfs_ --target x86_64-unknown-linux-gnu

# ThreadSanitizer - detects data races
RUSTFLAGS="-Z sanitizer=thread" cargo +nightly test vfs_ --target x86_64-unknown-linux-gnu

# MemorySanitizer - detects uninitialized memory reads
RUSTFLAGS="-Z sanitizer=memory" cargo +nightly test vfs_ --target x86_64-unknown-linux-gnu

# LeakSanitizer - detects memory leaks
RUSTFLAGS="-Z sanitizer=leak" cargo +nightly test vfs_ --target x86_64-unknown-linux-gnu

# Miri - undefined behavior detection
cargo +nightly miri test vfs_
```

### 6.4 VFS Callback Coverage Tests

**Test File:** `tests/vfs_callback_coverage.rs`

Ensure all VFS callbacks are exercised:

```rust
#[test]
fn test_vfs_xopen_callback() {
    // Test that xOpen is called with various flags
    // Validate file handle initialization
}

#[test]
fn test_vfs_xread_callback() {
    // Test xRead with various buffer sizes
    // Validate offset handling
    // Test partial reads
}

#[test]
fn test_vfs_xwrite_callback() {
    // Test xWrite with various buffer sizes
    // Validate offset handling
    // Test partial writes
}

#[test]
fn test_vfs_xsync_callback() {
    // Test xSync with different sync modes
    // Validate durability guarantees
}

#[test]
fn test_vfs_xtruncate_callback() {
    // Test file truncation
    // Validate size changes
}

#[test]
fn test_vfs_xfilesize_callback() {
    // Test size reporting
    // Validate after writes
}

#[test]
fn test_vfs_xlock_unlock_callbacks() {
    // Test locking protocol
    // Validate lock state transitions
}

#[test]
fn test_vfs_xclose_callback() {
    // Test cleanup on close
    // Validate no leaks
}
```

### 6.5 VFS Stress Testing

**Test File:** `tests/vfs_stress.rs`

```rust
#[test]
fn test_vfs_1000_concurrent_connections() {
    // Open 1000 SQLite connections through VFS
    // Execute queries concurrently
    // Verify no crashes, leaks, or data races
}

#[test]
fn test_vfs_rapid_connect_disconnect() {
    // Rapidly open and close connections
    // Test for resource leaks
    // Validate cleanup
}

#[test]
fn test_vfs_large_database_operations() {
    // Create 100MB+ database through VFS
    // Test page cache behavior
    // Validate performance
}

#[test]
fn test_vfs_corruption_recovery() {
    // Corrupt VFS-backed database
    // Verify SQLite detects corruption
    // Test recovery mechanisms
}
```

### 6.6 Running VFS FFI Tests

**Prerequisites:**
```bash
# Install nightly toolchain
rustup install nightly

# Install sanitizer support (Linux only)
rustup component add rust-src --toolchain nightly

# Install cargo-fuzz
cargo install cargo-fuzz
```

**Test Commands:**
```bash
# Run basic VFS tests
cargo test vfs_ffi

# Run with AddressSanitizer (24 hour continuous)
RUSTFLAGS="-Z sanitizer=address" \
  cargo +nightly test vfs_ --target x86_64-unknown-linux-gnu

# Run VFS fuzzer (10M executions)
cargo +nightly fuzz run fuzz_vfs_sqlite_operations -- \
  -max_total_time=86400 \
  -jobs=8 \
  -max_len=4096

# Run with coverage reporting
cargo +nightly fuzz coverage fuzz_vfs_sqlite_operations
llvm-cov show target/x86_64-unknown-linux-gnu/coverage/*/release/fuzz_vfs_sqlite_operations
```

### 6.7 VFS FFI Success Criteria

**Must Pass Before Production:**
- [ ] All VFS integration tests pass on Linux, macOS, Windows
- [ ] 10M+ fuzzing executions without crashes
- [ ] AddressSanitizer clean (no memory errors)
- [ ] ThreadSanitizer clean (no data races)
- [ ] LeakSanitizer clean (no memory leaks)
- [ ] All 29 unsafe blocks have test coverage
- [ ] VFS overhead < 10% vs native SQLite on benchmarks
- [ ] 1000 concurrent connections stress test passes
- [ ] All SQLite locking protocol states tested
- [ ] URI parameter parsing handles all valid formats

**Known Limitations:**
- Sanitizers only work on Linux (x86_64-unknown-linux-gnu target)
- Miri has limited FFI support - may not test all VFS callbacks
- Windows/macOS testing requires manual validation without sanitizers

---

## Test Implementation Guide

### Dependencies to Add

**Cargo.toml:**
```toml
[dev-dependencies]
# Existing
tempfile = "3.12"
rand = "0.8"

# Add for testing
proptest = "1.4"           # Property-based testing
criterion = "0.5"          # Benchmarking
rusqlite = "0.32"          # SQLite for VFS tests
libfuzzer-sys = "0.4"      # Fuzzing (in fuzz/Cargo.toml)

[profile.bench]
opt-level = 3
lto = true
```

### Fuzzing Setup

```bash
# Install cargo-fuzz
cargo install cargo-fuzz

# Initialize fuzzing
cargo fuzz init

# Add fuzz targets (see Phase 1.1)

# Run fuzzing
cargo +nightly fuzz run fuzz_vfs_operations -- -max_total_time=86400
```

### CI/CD Integration

**.github/workflows/comprehensive-tests.yml:**
```yaml
name: Comprehensive Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
        rust: [stable, nightly]

    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ matrix.rust }}

      - name: Run tests
        run: cargo test --all-features

      - name: Run property tests
        run: cargo test --all-features -- --include-ignored proptest

      - name: Run benchmarks
        if: matrix.os == 'ubuntu-latest' && matrix.rust == 'stable'
        run: cargo bench --no-fail-fast

  fuzzing:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@nightly

      - name: Install cargo-fuzz
        run: cargo install cargo-fuzz

      - name: Fuzz VFS (5 min per target)
        run: |
          cargo fuzz run fuzz_vfs_open -- -max_total_time=300
          cargo fuzz run fuzz_vfs_read_write -- -max_total_time=300
          cargo fuzz run fuzz_vfs_operations -- -max_total_time=300

  sanitizers:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        sanitizer: [address, thread, leak]

    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@nightly

      - name: Run with ${{ matrix.sanitizer }} sanitizer
        run: |
          RUSTFLAGS="-Z sanitizer=${{ matrix.sanitizer }}" \
          cargo +nightly test --target x86_64-unknown-linux-gnu

  coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable

      - name: Install tarpaulin
        run: cargo install cargo-tarpaulin

      - name: Generate coverage
        run: cargo tarpaulin --out Xml --all-features

      - name: Upload to codecov
        uses: codecov/codecov-action@v3
```

---

## Success Metrics

### Coverage Targets
- **Line Coverage:** 85%+ (current: ~75% estimated)
- **Branch Coverage:** 75%+
- **Unsafe Code Coverage:** 100% (all VFS FFI paths tested)
- **Fuzzing:** 10M+ executions without crashes

### Performance Baselines
- **Auto-Growth:** < 10ms per doubling
- **Hybrid Allocator Dispatch:** < 1μs per allocation
- **VFS Overhead:** < 10% vs native SQLite
- **Snapshot Creation:** < 1s for 1000 files
- **IAM Permission Check:** < 100μs with cache

### Reliability Checklist

**Phase 1 - Critical (Data Integrity & FFI Safety):** ✅ COMPLETE
- [x] Page corruption detection tests (5 tests)
- [x] B-tree catalog corruption tests (4 tests)
- [x] Allocator corruption tests (6 tests)
- [x] Property-based allocator tests (5 tests)
- [x] Crash recovery during growth (6 tests)
- [x] Auto-flush on Drop implementation
- [x] Corruption error detection added

**Phase 2 - Concurrency & Durability:** ✅ COMPLETE
- [x] Concurrent stress tests (5 tests, up to 12 threads)
- [x] VFS-like multi-connection tests (5 tests)
- [x] Snapshot consistency tests (5 tests)
- [x] IAM policy race condition tests (5 tests)
- [x] Buffer pool coherency tests (6 tests)

**Phase 3 - Performance & Scale:** ⏳ PENDING
- [ ] 100GB container test
- [ ] 1M files test
- [ ] Fragmentation measurement test
- [ ] Max auto-growth limit test

**Phase 4 - Advanced Features:** ⏳ PENDING
- [ ] Snapshot advanced tests
- [ ] Engram freeze validation
- [ ] Audit log tests

**Phase 5 - Security Audit:** ⏳ PENDING
- [ ] Encryption security tests
- [ ] IAM bypass attempt tests
- [ ] Memory safety validation

**Phase 6 - VFS FFI Unsafe Code:** ⏳ PENDING (CRITICAL FOR PRODUCTION)
- [ ] All VFS FFI fuzzers pass 10M executions
- [ ] VFS integration tests with actual SQLite
- [ ] All 29 unsafe blocks have test coverage
- [ ] AddressSanitizer clean (no memory errors)
- [ ] ThreadSanitizer clean (no data races)
- [ ] LeakSanitizer clean (no memory leaks)
- [ ] VFS callback coverage (xOpen, xRead, xWrite, xSync, etc)
- [ ] 1000 concurrent SQLite connections stress test
- [ ] VFS overhead < 10% vs native SQLite

**General:**
- [x] Allocator invariants always hold (proptest)
- [x] Basic crash recovery maintains data integrity

### Security Checklist
- [ ] All encryption tests pass
- [ ] IAM bypass attempts blocked
- [ ] No timing leaks in crypto operations
- [ ] Path traversal blocked
- [ ] Memory safety validated (Miri/ASan)
- [ ] VFS FFI has 100% unsafe coverage

---

## Continuous Improvement

### Monthly Review
- Analyze fuzzing corpus for new seeds
- Update tests based on bug reports
- Add regression tests for all fixed bugs
- Review benchmark trends

### Tools to Monitor
- **cargo-audit**: Security vulnerabilities
- **cargo-outdated**: Dependency updates
- **cargo-deny**: License/security policy
- **cargo-geiger**: Unsafe code metrics

### Community Contributions
- All PRs require tests
- Minimum 80% coverage for new code
- All unsafe code needs justification + tests

---

**Document Maintenance:**
- Update quarterly or when major features added
- Track phase completion status
- Review after each production incident
