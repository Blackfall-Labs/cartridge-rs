use cartridge::{vfs::register_vfs, Cartridge};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use parking_lot::Mutex;
use rusqlite::Connection;
use std::sync::Arc;

/// Benchmark SQLite INSERT performance on Cartridge VFS
fn bench_vfs_inserts(c: &mut Criterion) {
    let row_counts = vec![100, 1_000, 10_000];

    let mut group = c.benchmark_group("vfs_inserts");

    for count in row_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            b.iter_with_setup(
                || {
                    // Setup: Register VFS and create connection
                    let cart = Cartridge::new(10000);
                    let cart_arc = Arc::new(Mutex::new(cart));

                    // Register Cartridge VFS
                    register_vfs(cart_arc.clone()).unwrap();

                    let conn = Connection::open_with_flags(
                        "test.db",
                        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                            | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
                    )
                    .unwrap();

                    conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY, data TEXT)", [])
                        .unwrap();

                    (conn, cart_arc)
                },
                |(mut conn, _cart_arc)| {
                    // Measure: INSERT rows
                    let tx = conn.transaction().unwrap();
                    for i in 0..count {
                        tx.execute(
                            "INSERT INTO test (data) VALUES (?)",
                            [format!("Test data {}", i)],
                        )
                        .unwrap();
                    }
                    tx.commit().unwrap();
                    black_box(&conn);
                },
            );
        });
    }

    group.finish();
}

/// Benchmark SQLite SELECT performance on Cartridge VFS
fn bench_vfs_selects(c: &mut Criterion) {
    let row_counts = vec![100, 1_000, 10_000];

    let mut group = c.benchmark_group("vfs_selects");

    for count in row_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            // Setup once: Create and populate database
            let cart = Cartridge::new(20000);
            let cart_arc = Arc::new(Mutex::new(cart));

            register_vfs(cart_arc.clone()).unwrap();

            let mut conn = Connection::open_with_flags(
                "test.db",
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                    | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
            )
            .unwrap();

            conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY, data TEXT)", [])
                .unwrap();

            // Populate with test data
            let tx = conn.transaction().unwrap();
            for i in 0..count {
                tx.execute(
                    "INSERT INTO test (data) VALUES (?)",
                    [format!("Test data {}", i)],
                )
                .unwrap();
            }
            tx.commit().unwrap();

            b.iter(|| {
                // Measure: SELECT all rows
                let mut stmt = conn.prepare("SELECT * FROM test").unwrap();
                let rows: Vec<(i64, String)> = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                black_box(rows);
            });
        });
    }

    group.finish();
}

/// Benchmark SQLite transaction throughput on Cartridge VFS
fn bench_vfs_transactions(c: &mut Criterion) {
    let tx_sizes = vec![10, 100, 1_000];

    let mut group = c.benchmark_group("vfs_transactions");

    for size in tx_sizes {
        group.throughput(Throughput::Elements(size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let cart = Cartridge::new(10000);
            let cart_arc = Arc::new(Mutex::new(cart));

            register_vfs(cart_arc.clone()).unwrap();

            let mut conn = Connection::open_with_flags(
                "test.db",
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                    | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
            )
            .unwrap();

            conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY, data TEXT)", [])
                .unwrap();

            b.iter(|| {
                // Measure: Transaction with N operations
                let tx = conn.transaction().unwrap();
                for i in 0..size {
                    tx.execute(
                        "INSERT INTO test (data) VALUES (?)",
                        [format!("Data {}", i)],
                    )
                    .unwrap();
                }
                tx.commit().unwrap();
                black_box(&conn);
            });
        });
    }

    group.finish();
}

/// Benchmark SQLite UPDATE performance on Cartridge VFS
fn bench_vfs_updates(c: &mut Criterion) {
    let row_counts = vec![100, 1_000, 5_000];

    let mut group = c.benchmark_group("vfs_updates");

    for count in row_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &count| {
            // Setup: Create and populate database
            let cart = Cartridge::new(20000);
            let cart_arc = Arc::new(Mutex::new(cart));

            register_vfs(cart_arc.clone()).unwrap();

            let mut conn = Connection::open_with_flags(
                "test.db",
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                    | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
            )
            .unwrap();

            conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY, data TEXT)", [])
                .unwrap();

            // Populate
            let tx = conn.transaction().unwrap();
            for i in 0..count {
                tx.execute(
                    "INSERT INTO test (data) VALUES (?)",
                    [format!("Original {}", i)],
                )
                .unwrap();
            }
            tx.commit().unwrap();

            b.iter(|| {
                // Measure: UPDATE all rows
                let tx = conn.transaction().unwrap();
                tx.execute("UPDATE test SET data = 'Updated'", []).unwrap();
                tx.commit().unwrap();
                black_box(&conn);
            });
        });
    }

    group.finish();
}

/// Benchmark SQLite indexed vs non-indexed query performance
fn bench_vfs_index_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("vfs_index_performance");

    // Without index
    group.bench_function("without_index", |b| {
        let cart = Cartridge::new(30000);
        let cart_arc = Arc::new(Mutex::new(cart));

        register_vfs(cart_arc.clone()).unwrap();

        let mut conn = Connection::open_with_flags(
            "test.db",
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
        )
        .unwrap();

        conn.execute(
            "CREATE TABLE test (id INTEGER PRIMARY KEY, value INTEGER, data TEXT)",
            [],
        )
        .unwrap();

        // Insert 10,000 rows
        let tx = conn.transaction().unwrap();
        for i in 0..10_000 {
            tx.execute(
                "INSERT INTO test (value, data) VALUES (?, ?)",
                [&(i % 100) as &dyn rusqlite::ToSql, &format!("Data {}", i)],
            )
            .unwrap();
        }
        tx.commit().unwrap();

        b.iter(|| {
            // Query without index (table scan)
            let mut stmt = conn.prepare("SELECT * FROM test WHERE value = 42").unwrap();
            let rows: Vec<(i64, i64, String)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            black_box(rows);
        });
    });

    // With index
    group.bench_function("with_index", |b| {
        let cart = Cartridge::new(30000);
        let cart_arc = Arc::new(Mutex::new(cart));

        register_vfs(cart_arc.clone()).unwrap();

        let mut conn = Connection::open_with_flags(
            "test.db",
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
        )
        .unwrap();

        conn.execute(
            "CREATE TABLE test (id INTEGER PRIMARY KEY, value INTEGER, data TEXT)",
            [],
        )
        .unwrap();

        // Create index
        conn.execute("CREATE INDEX idx_value ON test(value)", [])
            .unwrap();

        // Insert 10,000 rows
        let tx = conn.transaction().unwrap();
        for i in 0..10_000 {
            tx.execute(
                "INSERT INTO test (value, data) VALUES (?, ?)",
                [&(i % 100) as &dyn rusqlite::ToSql, &format!("Data {}", i)],
            )
            .unwrap();
        }
        tx.commit().unwrap();

        b.iter(|| {
            // Query with index (index seek)
            let mut stmt = conn.prepare("SELECT * FROM test WHERE value = 42").unwrap();
            let rows: Vec<(i64, i64, String)> = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            black_box(rows);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_vfs_inserts,
    bench_vfs_selects,
    bench_vfs_transactions,
    bench_vfs_updates,
    bench_vfs_index_performance,
);
criterion_main!(benches);
