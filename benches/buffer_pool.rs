//! Benchmarks for ARC buffer pool performance

use cartridge::buffer_pool::BufferPool;
use cartridge::page::{Page, PageType};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::sync::Arc;

fn create_test_page() -> Arc<Page> {
    Arc::new(Page::new(PageType::ContentData))
}

fn benchmark_put(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_pool_put");

    for size in [100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let mut pool = BufferPool::new(size);
                for i in 0..size {
                    pool.put(black_box(i as u64), create_test_page());
                }
            });
        });
    }

    group.finish();
}

fn benchmark_get_hit(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_pool_get_hit");

    for size in [100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| {
                let mut pool = BufferPool::new(size);
                // Fill pool
                for i in 0..size {
                    pool.put(i as u64, create_test_page());
                }
                // Always hit (page 0 exists)
                black_box(pool.get(0));
            });
        });
    }

    group.finish();
}

fn benchmark_get_miss(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_pool_get_miss");

    for size in [100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let mut pool = BufferPool::new(size);
            b.iter(|| {
                // Always miss (page doesn't exist)
                black_box(pool.get(999999));
            });
        });
    }

    group.finish();
}

fn benchmark_sequential_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_pool_sequential");

    let size = 1000;
    let access_count = 10000;

    group.bench_function("sequential_scan", |b| {
        b.iter(|| {
            let mut pool = BufferPool::new(size);

            // First pass - fill cache
            for i in 0..size {
                pool.put(i as u64, create_test_page());
            }

            // Sequential scan
            for i in 0..access_count {
                let page_id = (i % size) as u64;
                black_box(pool.get(page_id));
            }
        });
    });

    group.finish();
}

fn benchmark_random_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_pool_random");

    let size = 1000;
    let access_pattern: Vec<u64> = vec![1, 5, 2, 1, 3, 5, 1, 2, 4, 1, 8, 3, 1, 5, 2];

    group.bench_function("random_access", |b| {
        b.iter(|| {
            let mut pool = BufferPool::new(size);

            // Simulate random access pattern
            for &page_id in access_pattern.iter().cycle().take(10000) {
                if pool.get(page_id).is_none() {
                    pool.put(page_id, create_test_page());
                }
            }
        });
    });

    group.finish();
}

fn benchmark_hit_rate(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_pool_hit_rate");

    // Working set: 500 pages
    // Cache size: 100 pages
    // 80% of accesses hit the hot 50 pages
    let working_set = 500;
    let cache_size = 100;
    let hot_set = 50;

    group.bench_function("80_20_workload", |b| {
        b.iter(|| {
            let mut pool = BufferPool::new(cache_size);
            let mut rng = 42u64; // Simple PRNG

            for _ in 0..10000 {
                // 80% chance of accessing hot set
                rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
                let page_id = if (rng % 100) < 80 {
                    // Hot set (0-49)
                    rng % hot_set
                } else {
                    // Cold set (50-499)
                    hot_set + (rng % (working_set - hot_set))
                };

                if pool.get(page_id).is_none() {
                    pool.put(page_id, create_test_page());
                }
            }

            let stats = pool.stats();
            black_box(stats.hit_rate());
        });
    });

    group.finish();
}

fn benchmark_arc_adaptation(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_pool_arc_adaptation");

    let cache_size = 100;

    // Test ARC's adaptive behavior with changing workload
    group.bench_function("workload_shift", |b| {
        b.iter(|| {
            let mut pool = BufferPool::new(cache_size);

            // Phase 1: Sequential scan (favors recency)
            for i in 0..200 {
                if pool.get(i).is_none() {
                    pool.put(i, create_test_page());
                }
            }

            // Phase 2: Loop over hot set (favors frequency)
            for _ in 0..100 {
                for i in 0..10 {
                    if pool.get(i).is_none() {
                        pool.put(i, create_test_page());
                    }
                }
            }

            // Phase 3: Mix of both
            let mut rng = 12345u64;
            for _ in 0..200 {
                rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
                let page_id = if (rng % 2) == 0 {
                    rng % 10 // Hot set
                } else {
                    10 + (rng % 190) // Cold set
                };

                if pool.get(page_id).is_none() {
                    pool.put(page_id, create_test_page());
                }
            }

            let stats = pool.stats();
            black_box(stats.p); // ARC adaptation parameter
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_put,
    benchmark_get_hit,
    benchmark_get_miss,
    benchmark_sequential_access,
    benchmark_random_access,
    benchmark_hit_rate,
    benchmark_arc_adaptation
);

criterion_main!(benches);
