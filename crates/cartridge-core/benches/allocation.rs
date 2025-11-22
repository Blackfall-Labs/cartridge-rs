use cartridge::allocator::{
    bitmap::BitmapAllocator, extent::ExtentAllocator, hybrid::HybridAllocator, BlockAllocator,
};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

/// Benchmark allocating 100K blocks
fn bench_allocate_100k(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocate_100k_blocks");

    group.bench_function("bitmap", |b| {
        b.iter(|| {
            let mut alloc = BitmapAllocator::new(100_000);
            // Allocate in chunks of 10 blocks
            for _ in 0..10_000 {
                alloc.allocate(10 * 4096).unwrap();
            }
        });
    });

    group.bench_function("extent", |b| {
        b.iter(|| {
            let mut alloc = ExtentAllocator::new(100_000);
            // Allocate in chunks of 10 blocks
            for _ in 0..10_000 {
                alloc.allocate(10 * 4096).unwrap();
            }
        });
    });

    group.bench_function("hybrid_small", |b| {
        b.iter(|| {
            let mut alloc = HybridAllocator::new(100_000);
            // Allocate small files (uses bitmap)
            for _ in 0..10_000 {
                alloc.allocate(10 * 1024).unwrap();
            }
        });
    });

    group.bench_function("hybrid_large", |b| {
        b.iter(|| {
            let mut alloc = HybridAllocator::new(100_000);
            // Allocate large files (uses extent)
            for _ in 0..100 {
                alloc.allocate(1024 * 1024).unwrap();
            }
        });
    });

    group.finish();
}

/// Benchmark allocation + free cycles (fragmentation test)
fn bench_alloc_free_cycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("alloc_free_cycle");

    group.bench_function("bitmap", |b| {
        b.iter(|| {
            let mut alloc = BitmapAllocator::new(10_000);
            let mut allocations = Vec::new();

            // Allocate
            for _ in 0..100 {
                let blocks = alloc.allocate(10 * 4096).unwrap();
                allocations.push(blocks);
            }

            // Free every other allocation
            for (i, blocks) in allocations.iter().enumerate() {
                if i % 2 == 0 {
                    alloc.free(blocks).unwrap();
                }
            }

            // Re-allocate
            for _ in 0..50 {
                alloc.allocate(10 * 4096).unwrap();
            }

            black_box(&alloc);
        });
    });

    group.bench_function("extent", |b| {
        b.iter(|| {
            let mut alloc = ExtentAllocator::new(10_000);
            let mut allocations = Vec::new();

            // Allocate
            for _ in 0..100 {
                let blocks = alloc.allocate(10 * 4096).unwrap();
                allocations.push(blocks);
            }

            // Free every other allocation
            for (i, blocks) in allocations.iter().enumerate() {
                if i % 2 == 0 {
                    alloc.free(blocks).unwrap();
                }
            }

            // Re-allocate
            for _ in 0..50 {
                alloc.allocate(10 * 4096).unwrap();
            }

            black_box(&alloc);
        });
    });

    group.finish();
}

/// Benchmark fragmentation score calculation
fn bench_fragmentation_score(c: &mut Criterion) {
    let mut group = c.benchmark_group("fragmentation_score");

    // Setup allocators with some fragmentation
    let mut bitmap = BitmapAllocator::new(10_000);
    let mut extent = ExtentAllocator::new(10_000);

    // Create fragmentation
    for i in 0..100 {
        let b1 = bitmap.allocate(10 * 4096).unwrap();
        let e1 = extent.allocate(10 * 4096).unwrap();

        if i % 2 == 0 {
            bitmap.free(&b1).unwrap();
            extent.free(&e1).unwrap();
        }
    }

    group.bench_function("bitmap", |b| {
        b.iter(|| black_box(bitmap.fragmentation_score()));
    });

    group.bench_function("extent", |b| {
        b.iter(|| black_box(extent.fragmentation_score()));
    });

    group.finish();
}

/// Benchmark individual allocation sizes
fn bench_allocation_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocation_by_size");

    for size_kb in [4, 16, 64, 256].iter() {
        let size_bytes = size_kb * 1024;

        group.bench_with_input(
            BenchmarkId::new("hybrid", format!("{}KB", size_kb)),
            &size_bytes,
            |b, &size| {
                b.iter(|| {
                    let mut alloc = HybridAllocator::new(10_000);
                    for _ in 0..100 {
                        alloc.allocate(size).unwrap();
                    }
                });
            },
        );
    }

    // Benchmark 1MB separately with larger allocator
    group.bench_function("hybrid/1024KB", |b| {
        b.iter(|| {
            let mut alloc = HybridAllocator::new(100_000);
            for _ in 0..100 {
                alloc.allocate(1024 * 1024).unwrap();
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_allocate_100k,
    bench_alloc_free_cycle,
    bench_fragmentation_score,
    bench_allocation_sizes
);
criterion_main!(benches);
