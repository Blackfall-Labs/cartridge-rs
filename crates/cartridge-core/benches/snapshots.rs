use cartridge::{header::Header, snapshot::SnapshotManager, Cartridge};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::collections::HashMap;
use tempfile::TempDir;

/// Benchmark snapshot creation at different scales
fn bench_snapshot_creation(c: &mut Criterion) {
    let file_counts = vec![10, 100, 1_000, 5_000];

    let mut group = c.benchmark_group("snapshot_creation");

    for count in file_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            b.iter_with_setup(
                || {
                    // Setup: Create cartridge with files and snapshot manager
                    let temp_dir = TempDir::new().unwrap();
                    let snapshot_dir = temp_dir.path().join("snapshots");
                    let cart_dir = temp_dir.path().join("cart");

                    let mut cart = Cartridge::new(count * 2);
                    for i in 0..count {
                        let path = format!("/file_{}.txt", i);
                        cart.create_file(&path, &vec![0x42u8; 1024]).unwrap();
                    }

                    let snap_mgr = SnapshotManager::new(&snapshot_dir).unwrap();

                    // Create page data for snapshot
                    let mut pages = HashMap::new();
                    for i in 0..count {
                        pages.insert(i as u64, vec![0x42u8; 4096]);
                    }

                    (snap_mgr, cart_dir, pages, temp_dir)
                },
                |(mut snap_mgr, cart_dir, pages, _temp_dir)| {
                    // Measure: Create snapshot
                    let snap_id = snap_mgr
                        .create_snapshot(
                            format!("v{}", count),
                            "Benchmark snapshot".to_string(),
                            cart_dir.clone(),
                            Header::new(),
                            &pages,
                        )
                        .unwrap();
                    black_box(snap_id);
                },
            );
        });
    }

    group.finish();
}

/// Benchmark snapshot restoration (loading metadata + pages)
fn bench_snapshot_restore(c: &mut Criterion) {
    let file_counts = vec![10, 100, 1_000];

    let mut group = c.benchmark_group("snapshot_restore");

    for count in file_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            // Setup: Create snapshot to restore
            let temp_dir = TempDir::new().unwrap();
            let snapshot_dir = temp_dir.path().join("snapshots");
            let cart_dir = temp_dir.path().join("cart");

            let mut cart = Cartridge::new(count * 2);
            for i in 0..count {
                let path = format!("/file_{}.txt", i);
                cart.create_file(&path, &vec![0x42u8; 1024]).unwrap();
            }

            let mut snap_mgr = SnapshotManager::new(&snapshot_dir).unwrap();

            let mut pages = HashMap::new();
            for i in 0..count {
                pages.insert(i as u64, vec![0x42u8; 4096]);
            }

            let snap_id = snap_mgr
                .create_snapshot(
                    "restore_test".to_string(),
                    "Snapshot for restore benchmark".to_string(),
                    cart_dir.clone(),
                    Header::new(),
                    &pages,
                )
                .unwrap();

            b.iter(|| {
                // Measure: Restore snapshot pages
                let restored_pages = snap_mgr.restore_snapshot(snap_id).unwrap();
                black_box(restored_pages);
            });
        });
    }

    group.finish();
}

/// Benchmark snapshot deletion
fn bench_snapshot_deletion(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_deletion");

    group.bench_function("delete_single", |b| {
        b.iter_with_setup(
            || {
                // Setup: Create snapshot to delete
                let temp_dir = TempDir::new().unwrap();
                let snapshot_dir = temp_dir.path().join("snapshots");
                let cart_dir = temp_dir.path().join("cart");

                let mut snap_mgr = SnapshotManager::new(&snapshot_dir).unwrap();

                let pages = HashMap::new();
                let snap_id = snap_mgr
                    .create_snapshot(
                        "delete_test".to_string(),
                        "Snapshot to delete".to_string(),
                        cart_dir,
                        Header::new(),
                        &pages,
                    )
                    .unwrap();

                (snap_mgr, snap_id, temp_dir)
            },
            |(mut snap_mgr, snap_id, _temp_dir)| {
                // Measure: Delete snapshot
                snap_mgr.delete_snapshot(snap_id).unwrap();
                black_box(&snap_mgr);
            },
        );
    });

    group.finish();
}

/// Benchmark snapshot pruning (keeping N most recent)
fn bench_snapshot_pruning(c: &mut Criterion) {
    let snapshot_counts = vec![10, 50, 100];

    let mut group = c.benchmark_group("snapshot_pruning");

    for count in snapshot_counts {
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            b.iter_with_setup(
                || {
                    // Setup: Create many snapshots
                    let temp_dir = TempDir::new().unwrap();
                    let snapshot_dir = temp_dir.path().join("snapshots");
                    let cart_dir = temp_dir.path().join("cart");

                    let mut snap_mgr = SnapshotManager::new(&snapshot_dir).unwrap();

                    let pages = HashMap::new();

                    // Create N snapshots
                    for i in 0..count {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        snap_mgr
                            .create_snapshot(
                                format!("v{}", i),
                                format!("Snapshot {}", i),
                                cart_dir.clone(),
                                Header::new(),
                                &pages,
                            )
                            .unwrap();
                    }

                    (snap_mgr, temp_dir)
                },
                |(mut snap_mgr, _temp_dir)| {
                    // Measure: Prune to keep only 5 most recent
                    let deleted = snap_mgr.prune_old_snapshots(5).unwrap();
                    black_box(deleted);
                },
            );
        });
    }

    group.finish();
}

/// Benchmark dirty page tracking (COW overhead)
fn bench_dirty_tracking(c: &mut Criterion) {
    let write_counts = vec![10, 100, 500];

    let mut group = c.benchmark_group("dirty_tracking");

    for count in write_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let temp_dir = TempDir::new().unwrap();
            let snapshot_dir = temp_dir.path().join("snapshots");
            let cart_dir = temp_dir.path().join("cart");

            let mut cart = Cartridge::new(count * 2);

            // Create files
            for i in 0..count {
                let path = format!("/file_{}.txt", i);
                cart.create_file(&path, &vec![0x41u8; 1024]).unwrap();
            }

            let mut snap_mgr = SnapshotManager::new(&snapshot_dir).unwrap();

            // Create initial snapshot
            let pages: HashMap<u64, Vec<u8>> = HashMap::new();
            snap_mgr
                .create_snapshot(
                    "base".to_string(),
                    "Base snapshot".to_string(),
                    cart_dir.clone(),
                    Header::new(),
                    &pages,
                )
                .unwrap();

            b.iter(|| {
                // Measure: Write to files (triggers COW)
                for i in 0..count {
                    let path = format!("/file_{}.txt", i);
                    cart.write_file(&path, &vec![0x42u8; 1024]).unwrap();
                }
                black_box(&cart);
            });
        });
    }

    group.finish();
}

/// Benchmark snapshot listing performance
fn bench_snapshot_listing(c: &mut Criterion) {
    let snapshot_counts = vec![10, 100, 500];

    let mut group = c.benchmark_group("snapshot_listing");

    for count in snapshot_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            // Setup: Create many snapshots
            let temp_dir = TempDir::new().unwrap();
            let snapshot_dir = temp_dir.path().join("snapshots");
            let cart_dir = temp_dir.path().join("cart");

            let mut snap_mgr = SnapshotManager::new(&snapshot_dir).unwrap();

            let pages = HashMap::new();

            for i in 0..count {
                snap_mgr
                    .create_snapshot(
                        format!("v{}", i),
                        format!("Snapshot {}", i),
                        cart_dir.clone(),
                        Header::new(),
                        &pages,
                    )
                    .unwrap();
            }

            b.iter(|| {
                // Measure: List all snapshots
                let snapshots = snap_mgr.list_snapshots();
                black_box(snapshots);
            });
        });
    }

    group.finish();
}

/// Benchmark snapshot metadata access
fn bench_snapshot_metadata(c: &mut Criterion) {
    let mut group = c.benchmark_group("snapshot_metadata");

    group.bench_function("get_metadata", |b| {
        let temp_dir = TempDir::new().unwrap();
        let snapshot_dir = temp_dir.path().join("snapshots");
        let cart_dir = temp_dir.path().join("cart");

        let mut snap_mgr = SnapshotManager::new(&snapshot_dir).unwrap();

        let pages = HashMap::new();
        let snap_id = snap_mgr
            .create_snapshot(
                "test".to_string(),
                "Test snapshot".to_string(),
                cart_dir,
                Header::new(),
                &pages,
            )
            .unwrap();

        b.iter(|| {
            let metadata = snap_mgr.get_snapshot(snap_id);
            black_box(metadata);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_snapshot_creation,
    bench_snapshot_restore,
    bench_snapshot_deletion,
    bench_snapshot_pruning,
    bench_dirty_tracking,
    bench_snapshot_listing,
    bench_snapshot_metadata,
);
criterion_main!(benches);
