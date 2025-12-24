use cartridge::{compression::CompressionMethod, Cartridge};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::time::Duration;

/// Simulate realistic mixed workload (80% reads, 20% writes)
fn bench_mixed_80_20(c: &mut Criterion) {
    let file_counts = vec![100, 500, 1_000];

    let mut group = c.benchmark_group("mixed_80_20");
    group.sample_size(20);

    for count in file_counts {
        group.throughput(Throughput::Elements(100));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let mut cart = Cartridge::new(count * 2);

            // Create directory structure
            cart.create_dir("/data").unwrap();

            // Pre-populate with files
            for i in 0..count {
                let path = format!("/data/file_{}.txt", i);
                cart.create_file(&path, &vec![0x42u8; 4096]).unwrap();
            }

            b.iter(|| {
                // 80 reads
                for _ in 0..80 {
                    let i = rand::random::<usize>() % count;
                    let path = format!("/data/file_{}.txt", i);
                    let data = cart.read_file(&path).unwrap();
                    black_box(data);
                }

                // 20 writes
                for _ in 0..20 {
                    let i = rand::random::<usize>() % count;
                    let path = format!("/data/file_{}.txt", i);
                    cart.write_file(&path, &vec![0x43u8; 4096]).unwrap();
                }
            });
        });
    }

    group.finish();
}

/// Simulate sequential access pattern (log appends, streaming)
fn bench_sequential_access(c: &mut Criterion) {
    let operation_counts = vec![100, 500, 1_000];

    let mut group = c.benchmark_group("sequential_access");

    for count in operation_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            b.iter(|| {
                let mut cart = Cartridge::new(count * 2);
                cart.create_dir("/logs").unwrap();

                // Sequential writes (like log appending)
                for i in 0..count {
                    let path = format!("/logs/entry_{:06}.log", i);
                    cart.create_file(&path, &vec![0x42u8; 512]).unwrap();
                }

                // Sequential reads (like streaming)
                for i in 0..count {
                    let path = format!("/logs/entry_{:06}.log", i);
                    let data = cart.read_file(&path).unwrap();
                    black_box(data);
                }
            });
        });
    }

    group.finish();
}

/// Simulate random access pattern (database-like)
fn bench_random_access(c: &mut Criterion) {
    let file_counts = vec![100, 500, 1_000];

    let mut group = c.benchmark_group("random_access");

    for count in file_counts {
        group.throughput(Throughput::Elements(100));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let mut cart = Cartridge::new(count * 2);
            cart.create_dir("/db").unwrap();

            // Pre-populate
            for i in 0..count {
                let path = format!("/db/record_{}.dat", i);
                cart.create_file(&path, &vec![0x42u8; 1024]).unwrap();
            }

            b.iter(|| {
                // Random reads and writes
                for _ in 0..100 {
                    let i = rand::random::<usize>() % count;
                    let path = format!("/db/record_{}.dat", i);

                    if rand::random::<bool>() {
                        let data = cart.read_file(&path).unwrap();
                        black_box(data);
                    } else {
                        cart.write_file(&path, &vec![0x43u8; 1024]).unwrap();
                    }
                }
            });
        });
    }

    group.finish();
}

/// Simulate small file churn (many creates/deletes)
fn bench_small_file_churn(c: &mut Criterion) {
    let churn_counts = vec![50, 100, 250];

    let mut group = c.benchmark_group("small_file_churn");

    for count in churn_counts {
        group.throughput(Throughput::Elements(count as u64 * 2)); // create + delete

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let mut cart = Cartridge::new(count * 4);
            cart.create_dir("/temp").unwrap();

            b.iter(|| {
                // Create many small files
                for i in 0..count {
                    let path = format!("/temp/file_{}.tmp", i);
                    cart.create_file(&path, &vec![0x42u8; 256]).unwrap();
                }

                // Delete them all
                for i in 0..count {
                    let path = format!("/temp/file_{}.tmp", i);
                    cart.delete_file(&path).unwrap();
                }
            });
        });
    }

    group.finish();
}

/// Simulate large file streaming (media, downloads)
fn bench_large_file_streaming(c: &mut Criterion) {
    let file_sizes = vec![
        ("1MB", 1024 * 1024),
        ("10MB", 10 * 1024 * 1024),
        ("50MB", 50 * 1024 * 1024),
    ];

    let mut group = c.benchmark_group("large_file_streaming");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(15));

    for (name, size) in file_sizes {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("write", name), &size, |b, &size| {
            let mut cart = Cartridge::new((size / 4096) * 2);
            cart.create_dir("/media").unwrap();

            b.iter(|| {
                let data = vec![0x42u8; size];
                cart.create_file("/media/large.bin", &data).unwrap();
                black_box(&cart);
            });
        });

        group.bench_with_input(BenchmarkId::new("read", name), &size, |b, &size| {
            let mut cart = Cartridge::new((size / 4096) * 2);
            cart.create_dir("/media").unwrap();
            let data = vec![0x42u8; size];
            cart.create_file("/media/large.bin", &data).unwrap();

            b.iter(|| {
                let result = cart.read_file("/media/large.bin").unwrap();
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Simulate directory traversal workload
fn bench_directory_traversal(c: &mut Criterion) {
    let file_counts = vec![100, 500, 1_000];

    let mut group = c.benchmark_group("directory_traversal");

    for count in file_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let mut cart = Cartridge::new(count * 2);
            cart.create_dir("/project").unwrap();

            // Create nested directory structure
            for i in 0..count {
                let dir = format!("/project/module_{}", i % 10);
                cart.create_dir(&dir).ok();
                let path = format!("{}/file_{}.rs", dir, i);
                cart.create_file(&path, &vec![0x42u8; 512]).unwrap();
            }

            b.iter(|| {
                // List all files (simulates directory walk)
                let files = cart.list_all_files().unwrap();
                black_box(files);
            });
        });
    }

    group.finish();
}

/// Simulate compression-heavy workload (text files, logs)
fn bench_compression_workload(c: &mut Criterion) {
    use cartridge::compression::{compress, decompress};

    let file_counts = vec![10, 50, 100];

    let mut group = c.benchmark_group("compression_workload");
    group.sample_size(20);

    for count in file_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            let mut cart = Cartridge::new(count * 4);
            cart.create_dir("/logs").unwrap();

            // Highly compressible data (logs with repetition)
            let log_data = b"[INFO] System startup complete. Initializing modules... ".repeat(50);

            b.iter(|| {
                // Write compressed files
                for i in 0..count {
                    let path = format!("/logs/app_{}.log", i);
                    let compressed = compress(&log_data, CompressionMethod::Lz4).unwrap();
                    cart.create_file(&path, &compressed).unwrap();
                }

                // Read and decompress
                for i in 0..count {
                    let path = format!("/logs/app_{}.log", i);
                    let compressed = cart.read_file(&path).unwrap();
                    let decompressed = decompress(&compressed, CompressionMethod::Lz4).unwrap();
                    black_box(decompressed);
                }
            });
        });
    }

    group.finish();
}

/// Simulate multi-user concurrent access simulation
fn bench_multi_user_simulation(c: &mut Criterion) {
    let user_counts = vec![5, 10, 20];

    let mut group = c.benchmark_group("multi_user_simulation");

    for users in user_counts {
        group.bench_with_input(BenchmarkId::from_parameter(users), &users, |b, &users| {
            let mut cart = Cartridge::new(users * 200);
            cart.create_dir("/users").unwrap();

            // Setup: Each user has their own directory with files
            for user_id in 0..users {
                let user_dir = format!("/users/user_{}", user_id);
                cart.create_dir(&user_dir).ok();

                for file_id in 0..10 {
                    let path = format!("{}/doc_{}.txt", user_dir, file_id);
                    cart.create_file(&path, &vec![0x42u8; 2048]).unwrap();
                }
            }

            b.iter(|| {
                // Simulate each user performing operations
                for user_id in 0..users {
                    // 70% read own files
                    for _ in 0..7 {
                        let file_id = rand::random::<usize>() % 10;
                        let path = format!("/users/user_{}/doc_{}.txt", user_id, file_id);
                        let data = cart.read_file(&path).unwrap();
                        black_box(data);
                    }

                    // 20% write own files
                    for _ in 0..2 {
                        let file_id = rand::random::<usize>() % 10;
                        let path = format!("/users/user_{}/doc_{}.txt", user_id, file_id);
                        cart.write_file(&path, &vec![0x43u8; 2048]).unwrap();
                    }

                    // 10% read shared files
                    if rand::random::<f32>() < 0.1 {
                        let other_user = rand::random::<usize>() % users;
                        let file_id = rand::random::<usize>() % 10;
                        let path = format!("/users/user_{}/doc_{}.txt", other_user, file_id);
                        if let Ok(data) = cart.read_file(&path) {
                            black_box(data);
                        }
                    }
                }
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_mixed_80_20,
    bench_sequential_access,
    bench_random_access,
    bench_small_file_churn,
    bench_large_file_streaming,
    bench_directory_traversal,
    bench_compression_workload,
    bench_multi_user_simulation,
);
criterion_main!(benches);
