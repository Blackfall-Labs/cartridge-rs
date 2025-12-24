use cartridge::Cartridge;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

/// Benchmark pager read performance with hot cache (repeated reads)
fn bench_pager_hot_reads(c: &mut Criterion) {
    let page_counts = vec![10, 100, 1000];

    let mut group = c.benchmark_group("pager_hot_reads");

    for count in page_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            // Create cartridge and populate with files
            let mut cart = Cartridge::new((count * 2) as usize);
            for i in 0..count {
                let path = format!("/file_{}.txt", i);
                cart.create_file(&path, &vec![0x42u8; 4096]).unwrap();
            }

            b.iter(|| {
                // Read same files repeatedly (should hit cache)
                for i in 0..count {
                    let path = format!("/file_{}.txt", i);
                    let data = cart.read_file(&path).unwrap();
                    black_box(data);
                }
            });
        });
    }

    group.finish();
}

/// Benchmark pager read performance with cold cache (new files each iteration)
fn bench_pager_cold_reads(c: &mut Criterion) {
    let page_counts = vec![10, 100, 500];

    let mut group = c.benchmark_group("pager_cold_reads");
    group.sample_size(10); // Fewer samples for cold reads

    for count in page_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            b.iter_with_setup(
                || {
                    // Setup: Create fresh cartridge each iteration (cold cache)
                    let mut cart = Cartridge::new((count * 2) as usize);
                    for i in 0..count {
                        let path = format!("/file_{}.txt", i);
                        cart.create_file(&path, &vec![0x42u8; 4096]).unwrap();
                    }
                    cart
                },
                |mut cart| {
                    // Measure: Read all files (cold cache)
                    for i in 0..count {
                        let path = format!("/file_{}.txt", i);
                        let data = cart.read_file(&path).unwrap();
                        black_box(data);
                    }
                },
            );
        });
    }

    group.finish();
}

/// Benchmark cache hit rate under different access patterns
fn bench_cache_hit_rate(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_hit_rate");

    // 90/10 hot/cold access pattern (high hit rate expected)
    group.bench_function("90_10_access", |b| {
        let mut cart = Cartridge::new(2000);

        // Create 100 files
        for i in 0..100 {
            let path = format!("/file_{}.txt", i);
            cart.create_file(&path, &vec![0x42u8; 4096]).unwrap();
        }

        b.iter(|| {
            // 90% access to first 10 files (hot set)
            for _ in 0..90 {
                let i = rand::random::<usize>() % 10;
                let path = format!("/file_{}.txt", i);
                let data = cart.read_file(&path).unwrap();
                black_box(data);
            }

            // 10% access to remaining files (cold set)
            for _ in 0..10 {
                let i = 10 + (rand::random::<usize>() % 90);
                let path = format!("/file_{}.txt", i);
                let data = cart.read_file(&path).unwrap();
                black_box(data);
            }
        });
    });

    // 50/50 access pattern (medium hit rate)
    group.bench_function("50_50_access", |b| {
        let mut cart = Cartridge::new(2000);

        for i in 0..100 {
            let path = format!("/file_{}.txt", i);
            cart.create_file(&path, &vec![0x42u8; 4096]).unwrap();
        }

        b.iter(|| {
            for _ in 0..100 {
                let i = rand::random::<usize>() % 100;
                let path = format!("/file_{}.txt", i);
                let data = cart.read_file(&path).unwrap();
                black_box(data);
            }
        });
    });

    group.finish();
}

/// Benchmark page write-through performance
fn bench_pager_writes(c: &mut Criterion) {
    let write_counts = vec![10, 100, 500];

    let mut group = c.benchmark_group("pager_writes");

    for count in write_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let mut cart = Cartridge::new((count * 2) as usize);

            // Pre-create files
            for i in 0..count {
                let path = format!("/file_{}.txt", i);
                cart.create_file(&path, &vec![0x41u8; 4096]).unwrap();
            }

            b.iter(|| {
                // Write to all files
                for i in 0..count {
                    let path = format!("/file_{}.txt", i);
                    cart.write_file(&path, &vec![0x42u8; 4096]).unwrap();
                    black_box(&cart);
                }
            });
        });
    }

    group.finish();
}

/// Benchmark sequential vs random access patterns
fn bench_access_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("access_patterns");

    group.bench_function("sequential_reads", |b| {
        let mut cart = Cartridge::new(1000);

        for i in 0..100 {
            let path = format!("/file_{:03}.txt", i);
            cart.create_file(&path, &vec![0x42u8; 4096]).unwrap();
        }

        b.iter(|| {
            // Sequential access
            for i in 0..100 {
                let path = format!("/file_{:03}.txt", i);
                let data = cart.read_file(&path).unwrap();
                black_box(data);
            }
        });
    });

    group.bench_function("random_reads", |b| {
        let mut cart = Cartridge::new(1000);

        for i in 0..100 {
            let path = format!("/file_{:03}.txt", i);
            cart.create_file(&path, &vec![0x42u8; 4096]).unwrap();
        }

        b.iter(|| {
            // Random access
            for _ in 0..100 {
                let i = rand::random::<usize>() % 100;
                let path = format!("/file_{:03}.txt", i);
                let data = cart.read_file(&path).unwrap();
                black_box(data);
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_pager_hot_reads,
    bench_pager_cold_reads,
    bench_cache_hit_rate,
    bench_pager_writes,
    bench_access_patterns,
);
criterion_main!(benches);
