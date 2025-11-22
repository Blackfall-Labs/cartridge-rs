use cartridge::{compression::*, encryption::*, engram_integration::EngramFreezer, Cartridge};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::time::Duration;
use tempfile::TempDir;

/// Benchmark file operations at different scales
fn bench_file_operations(c: &mut Criterion) {
    let sizes = vec![
        ("1KB", 1024),
        ("4KB", 4096),
        ("16KB", 16 * 1024),
        ("64KB", 64 * 1024),
        ("256KB", 256 * 1024),
        ("1MB", 1024 * 1024),
        ("4MB", 4 * 1024 * 1024),
    ];

    let mut group = c.benchmark_group("file_operations");

    for (name, size) in sizes {
        group.throughput(Throughput::Bytes(size as u64));

        // Write benchmark
        group.bench_with_input(BenchmarkId::new("write", name), &size, |b, &size| {
            b.iter(|| {
                let mut cart = Cartridge::new(10000);
                let data = vec![0x42u8; size];
                cart.create_file("/benchmark.bin", &data).unwrap();
                black_box(cart);
            });
        });

        // Read benchmark
        group.bench_with_input(BenchmarkId::new("read", name), &size, |b, &size| {
            let mut cart = Cartridge::new(10000);
            let data = vec![0x42u8; size];
            cart.create_file("/benchmark.bin", &data).unwrap();

            b.iter(|| {
                let result = cart.read_file("/benchmark.bin").unwrap();
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark compression at different data sizes
fn bench_compression_scalability(c: &mut Criterion) {
    let sizes = vec![
        ("512B", 512),
        ("4KB", 4096),
        ("64KB", 64 * 1024),
        ("1MB", 1024 * 1024),
        ("10MB", 10 * 1024 * 1024),
    ];

    let mut group = c.benchmark_group("compression_scalability");
    group.sample_size(20);

    for (name, size) in sizes {
        group.throughput(Throughput::Bytes(size as u64));

        // Repetitive data (highly compressible)
        let repetitive_data = vec![0x41u8; size];

        // LZ4 compression
        group.bench_with_input(
            BenchmarkId::new("lz4_compress", name),
            &repetitive_data,
            |b, data| {
                b.iter(|| {
                    let compressed = compress(data, CompressionMethod::Lz4).unwrap();
                    black_box(compressed);
                });
            },
        );

        // Zstd compression
        group.bench_with_input(
            BenchmarkId::new("zstd_compress", name),
            &repetitive_data,
            |b, data| {
                b.iter(|| {
                    let compressed = compress(data, CompressionMethod::Zstd).unwrap();
                    black_box(compressed);
                });
            },
        );

        // LZ4 decompression
        let lz4_compressed = compress(&repetitive_data, CompressionMethod::Lz4).unwrap();
        group.bench_with_input(
            BenchmarkId::new("lz4_decompress", name),
            &lz4_compressed,
            |b, data| {
                b.iter(|| {
                    let decompressed = decompress(data, CompressionMethod::Lz4).unwrap();
                    black_box(decompressed);
                });
            },
        );

        // Zstd decompression
        let zstd_compressed = compress(&repetitive_data, CompressionMethod::Zstd).unwrap();
        group.bench_with_input(
            BenchmarkId::new("zstd_decompress", name),
            &zstd_compressed,
            |b, data| {
                b.iter(|| {
                    let decompressed = decompress(data, CompressionMethod::Zstd).unwrap();
                    black_box(decompressed);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark encryption at different data sizes
fn bench_encryption_scalability(c: &mut Criterion) {
    let sizes = vec![
        ("1KB", 1024),
        ("16KB", 16 * 1024),
        ("256KB", 256 * 1024),
        ("1MB", 1024 * 1024),
        ("10MB", 10 * 1024 * 1024),
    ];

    let mut group = c.benchmark_group("encryption_scalability");
    group.sample_size(20);

    let key = EncryptionConfig::generate_key();

    for (name, size) in sizes {
        group.throughput(Throughput::Bytes(size as u64));

        let plaintext = vec![0x42u8; size];

        // Encryption
        group.bench_with_input(BenchmarkId::new("encrypt", name), &plaintext, |b, data| {
            b.iter(|| {
                let encrypted = encrypt(data, &key).unwrap();
                black_box(encrypted);
            });
        });

        // Decryption
        let ciphertext = encrypt(&plaintext, &key).unwrap();
        group.bench_with_input(BenchmarkId::new("decrypt", name), &ciphertext, |b, data| {
            b.iter(|| {
                let decrypted = decrypt(data, &key).unwrap();
                black_box(decrypted);
            });
        });
    }

    group.finish();
}

/// Benchmark engram freezing with different cartridge sizes
fn bench_engram_freezing(c: &mut Criterion) {
    let file_counts = vec![
        ("10_files", 10, 1024),
        ("100_files", 100, 1024),
        ("1000_files", 1000, 512),
    ];

    let mut group = c.benchmark_group("engram_freezing");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    for (name, file_count, file_size) in file_counts {
        group.throughput(Throughput::Bytes((file_count * file_size) as u64));

        group.bench_function(name, |b| {
            b.iter(|| {
                let temp_dir = TempDir::new().unwrap();
                let engram_path = temp_dir.path().join("test.eng");

                let mut cart = Cartridge::new(file_count * 2);
                for i in 0..file_count {
                    let path = format!("/file_{}.txt", i);
                    let data = vec![0x42u8; file_size];
                    cart.create_file(&path, &data).unwrap();
                }

                let freezer = EngramFreezer::new_default(
                    "bench".to_string(),
                    "1.0".to_string(),
                    "Benchmark".to_string(),
                );

                freezer.freeze(&mut cart, &engram_path).unwrap();
                black_box(engram_path);
            });
        });
    }

    group.finish();
}

/// Benchmark catalog lookups with different numbers of files
fn bench_catalog_scalability(c: &mut Criterion) {
    let file_counts = vec![100, 1000, 10000];

    let mut group = c.benchmark_group("catalog_scalability");

    for count in file_counts {
        group.bench_with_input(BenchmarkId::new("lookup", count), &count, |b, &count| {
            let mut cart = Cartridge::new(count * 2);

            // Create files
            for i in 0..count {
                let path = format!("/dir{}/file_{}.txt", i % 10, i);
                cart.create_file(&path, b"test").unwrap();
            }

            // Benchmark lookup
            b.iter(|| {
                let path = format!("/dir{}/file_{}.txt", count / 2 % 10, count / 2);
                let result = cart.read_file(&path).unwrap();
                black_box(result);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_file_operations,
    bench_compression_scalability,
    bench_encryption_scalability,
    bench_engram_freezing,
    bench_catalog_scalability,
);
criterion_main!(benches);
