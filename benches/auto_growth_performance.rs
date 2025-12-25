use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use cartridge_rs::Cartridge;

fn bench_sequential_growth(c: &mut Criterion) {
    let mut group = c.benchmark_group("auto_growth");

    // Measure growth overhead at different stages
    for stage in [3, 6, 12, 24, 48, 96] {
        group.bench_with_input(
            BenchmarkId::new("growth_to_blocks", stage),
            &stage,
            |b, &target_blocks| {
                b.iter(|| {
                    let mut cart = Cartridge::create(
                        &format!("bench-growth-{}", target_blocks),
                        "Bench Growth"
                    ).unwrap();

                    // Force growth to target by writing large files
                    let mut file_num = 0;
                    while cart.header().total_blocks < target_blocks as u64 {
                        let size = 512 * 1024; // 512KB to trigger extent allocator
                        cart.write(&format!("/file{}.bin", file_num), &vec![0xAB; size]).unwrap();
                        file_num += 1;
                    }

                    std::fs::remove_file(format!("bench-growth-{}.cart", target_blocks)).ok();
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
            let mut cart = Cartridge::create("bench-hybrid-small", "Bench Hybrid").unwrap();
            for i in 0..100 {
                cart.write(&format!("/small{}.txt", i), &vec![i as u8; 10 * 1024]).unwrap(); // 10KB
            }
            std::fs::remove_file("bench-hybrid-small.cart").ok();
        });
    });

    group.bench_function("large_file_extent", |b| {
        b.iter(|| {
            let mut cart = Cartridge::create("bench-hybrid-large", "Bench Hybrid").unwrap();
            for i in 0..10 {
                cart.write(&format!("/large{}.bin", i), &vec![i as u8; 512 * 1024]).unwrap(); // 512KB
            }
            std::fs::remove_file("bench-hybrid-large.cart").ok();
        });
    });

    group.bench_function("mixed_workload", |b| {
        b.iter(|| {
            let mut cart = Cartridge::create("bench-hybrid-mixed", "Bench Hybrid").unwrap();
            for i in 0..50 {
                cart.write(&format!("/small{}.txt", i), &vec![i as u8; 10 * 1024]).unwrap();
                cart.write(&format!("/large{}.bin", i), &vec![i as u8; 512 * 1024]).unwrap();
            }
            std::fs::remove_file("bench-hybrid-mixed.cart").ok();
        });
    });

    group.finish();
}

fn bench_growth_overhead_measurement(c: &mut Criterion) {
    let mut group = c.benchmark_group("growth_overhead");

    // Measure just the growth operation itself
    group.bench_function("single_growth_operation", |b| {
        b.iter_batched(
            || {
                // Setup: create cart and fill it
                let mut cart = Cartridge::create("bench-growth-op", "Bench Growth Op").unwrap();
                // Fill to near capacity to trigger growth on next write
                for i in 0..10 {
                    cart.write(&format!("/file{}.bin", i), &vec![0xAB; 256 * 1024]).unwrap();
                }
                cart
            },
            |mut cart| {
                // Measure: this write should trigger growth
                cart.write("/trigger.bin", &vec![0xCD; 512 * 1024]).unwrap();
                std::fs::remove_file("bench-growth-op.cart").ok();
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_allocator_free_blocks_tracking(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocator_tracking");

    group.bench_function("allocate_deallocate_cycle", |b| {
        b.iter(|| {
            let mut cart = Cartridge::create("bench-alloc-track", "Bench Alloc Track").unwrap();

            // Allocate
            for i in 0..100 {
                cart.write(&format!("/file{}.bin", i), &vec![i as u8; 64 * 1024]).unwrap();
            }

            // Deallocate
            for i in 0..100 {
                cart.delete(&format!("/file{}.bin", i)).unwrap();
            }

            std::fs::remove_file("bench-alloc-track.cart").ok();
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_sequential_growth,
    bench_hybrid_allocator_dispatch,
    bench_growth_overhead_measurement,
    bench_allocator_free_blocks_tracking
);
criterion_main!(benches);
