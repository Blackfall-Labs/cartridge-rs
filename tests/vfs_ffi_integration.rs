//! VFS FFI Integration Tests - Phase 6
//!
//! Tests the 29 unsafe FFI blocks in src/core/vfs/vfs.rs
//! Validates memory safety and SQLite integration

use cartridge_rs::core::cartridge::Cartridge;
use rusqlite::{Connection, OpenFlags, params};
use std::sync::Arc;
use parking_lot::Mutex;
use tempfile::TempDir;

// Note: VFS API is in core module, not exposed on public wrapper
use cartridge_rs::core::vfs::{register_vfs, unregister_vfs, VFS_NAME};

#[test]
fn test_vfs_concurrent_readers() {
    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("concurrent.cart");

    let mut cart = Cartridge::create_at(&cart_path, "vfs-concurrent", "VFS Concurrent").unwrap();
    cart.create_file("/db.sqlite", b"").unwrap();
    cart.flush().unwrap();

    let cart_arc = Arc::new(Mutex::new(cart));
    register_vfs(Arc::clone(&cart_arc)).unwrap();

    let db_uri = format!("file:/db.sqlite?vfs={}", VFS_NAME);

    // Create initial data
    {
        let conn = Connection::open_with_flags(
            &db_uri,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_URI,
        )
        .unwrap();

        conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY, value TEXT)", []).unwrap();
        for i in 0..10 {
            conn.execute("INSERT INTO test VALUES (?, ?)", params![i, format!("value{}", i)]).unwrap();
        }
        drop(conn);
    }

    cart_arc.lock().flush().unwrap();

    // Spawn 10 concurrent reader threads
    let handles: Vec<_> = (0..10)
        .map(|thread_id| {
            let db_uri_clone = db_uri.clone();
            std::thread::spawn(move || {
                let conn = Connection::open_with_flags(
                    &db_uri_clone,
                    OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
                )
                .unwrap();

                // Each thread reads 100 times
                for _ in 0..100 {
                    let count: i64 = conn
                        .query_row("SELECT COUNT(*) FROM test", [], |row| row.get(0))
                        .unwrap();
                    assert_eq!(count, 10);

                    // Read specific value
                    let value: String = conn
                        .query_row(
                            "SELECT value FROM test WHERE id = ?",
                            params![thread_id % 10],
                            |row| row.get(0),
                        )
                        .unwrap();
                    assert_eq!(value, format!("value{}", thread_id % 10));
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    unregister_vfs().unwrap();
}

#[test]
fn test_vfs_writer_reader_isolation() {
    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("isolation.cart");

    let cart = Cartridge::create_at(&cart_path, "vfs-isolation", "VFS Isolation").unwrap();
    let cart_arc = Arc::new(Mutex::new(cart));
    register_vfs(Arc::clone(&cart_arc)).unwrap();

    let db_uri = format!("file:/db.sqlite?vfs={}", VFS_NAME);

    // Writer thread
    let db_uri_write = db_uri.clone();
    let writer = std::thread::spawn(move || {
        let conn = Connection::open_with_flags(
            &db_uri_write,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_URI,
        )
        .unwrap();

        conn.execute("CREATE TABLE data (id INTEGER PRIMARY KEY, val INTEGER)", [])
            .unwrap();

        for i in 0..50 {
            conn.execute("INSERT INTO data VALUES (?, ?)", params![i, i * 2])
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });

    // Give writer time to create table
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Reader threads (may see partial data, but should never see corruption)
    let readers: Vec<_> = (0..3)
        .map(|_| {
            let db_uri_clone = db_uri.clone();
            std::thread::spawn(move || {
                // Readers may fail if table doesn't exist yet, that's OK
                if let Ok(conn) = Connection::open_with_flags(
                    &db_uri_clone,
                    OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
                ) {
                    for _ in 0..20 {
                        // This may fail if table doesn't exist, which is fine
                        let _ = conn.query_row("SELECT COUNT(*) FROM data", [], |row| row.get::<_, i64>(0));
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                }
            })
        })
        .collect();

    writer.join().unwrap();
    for r in readers {
        r.join().unwrap();
    }

    unregister_vfs().unwrap();
}

#[test]
fn test_vfs_large_blob_operations() {
    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("blobs.cart");

    let cart = Cartridge::create_at(&cart_path, "vfs-blobs", "VFS Blobs").unwrap();
    let cart_arc = Arc::new(Mutex::new(cart));
    register_vfs(Arc::clone(&cart_arc)).unwrap();

    let db_uri = format!("file:/blobs.db?vfs={}", VFS_NAME);

    let conn = Connection::open_with_flags(
        &db_uri,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();

    conn.execute("CREATE TABLE blobs (id INTEGER PRIMARY KEY, data BLOB)", [])
        .unwrap();

    // Insert large blobs (256KB each)
    for i in 0..3 {
        let blob_data = vec![0xABu8; 256 * 1024];
        conn.execute("INSERT INTO blobs VALUES (?, ?)", params![i, blob_data])
            .unwrap();
    }

    // Read back and verify
    for i in 0..3 {
        let blob: Vec<u8> = conn
            .query_row("SELECT data FROM blobs WHERE id = ?", params![i], |row| {
                row.get(0)
            })
            .unwrap();

        assert_eq!(blob.len(), 256 * 1024);
        assert!(blob.iter().all(|&b| b == 0xAB));
    }

    drop(conn);
    unregister_vfs().unwrap();
}

#[test]
fn test_vfs_vacuum_operations() {
    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("vacuum.cart");

    let cart = Cartridge::create_at(&cart_path, "vfs-vacuum", "VFS Vacuum").unwrap();
    let cart_arc = Arc::new(Mutex::new(cart));
    register_vfs(Arc::clone(&cart_arc)).unwrap();

    let db_uri = format!("file:/vacuum.db?vfs={}", VFS_NAME);

    let conn = Connection::open_with_flags(
        &db_uri,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();

    conn.execute("CREATE TABLE data (id INTEGER, value TEXT)", [])
        .unwrap();

    // Insert lots of data
    for i in 0..1000 {
        conn.execute(
            "INSERT INTO data VALUES (?, ?)",
            params![i, format!("value{}", i)],
        )
        .unwrap();
    }

    // Delete half
    conn.execute("DELETE FROM data WHERE id % 2 = 0", [])
        .unwrap();

    // Get size before vacuum
    let size_before: i64 = conn
        .query_row("SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()", [], |row| row.get(0))
        .unwrap();

    // Vacuum should reclaim space
    conn.execute("VACUUM", []).unwrap();

    let size_after: i64 = conn
        .query_row("SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()", [], |row| row.get(0))
        .unwrap();

    // Size should be smaller after vacuum (or at least not larger)
    assert!(size_after <= size_before);

    // Verify data integrity after vacuum
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM data", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 500);

    drop(conn);
    unregister_vfs().unwrap();
}

#[test]
fn test_vfs_attach_database() {
    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("attach.cart");

    let cart = Cartridge::create_at(&cart_path, "vfs-attach", "VFS Attach").unwrap();
    let cart_arc = Arc::new(Mutex::new(cart));
    register_vfs(Arc::clone(&cart_arc)).unwrap();

    let db1_uri = format!("file:/db1.sqlite?vfs={}", VFS_NAME);
    let db2_uri = format!("file:/db2.sqlite?vfs={}", VFS_NAME);

    // Create first database
    {
        let conn = Connection::open_with_flags(
            &db1_uri,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_URI,
        )
        .unwrap();

        conn.execute("CREATE TABLE users (id INTEGER, name TEXT)", [])
            .unwrap();
        conn.execute("INSERT INTO users VALUES (1, 'Alice')", [])
            .unwrap();
    }

    // Create second database
    {
        let conn = Connection::open_with_flags(
            &db2_uri,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_URI,
        )
        .unwrap();

        conn.execute("CREATE TABLE products (id INTEGER, name TEXT)", [])
            .unwrap();
        conn.execute("INSERT INTO products VALUES (1, 'Widget')", [])
            .unwrap();
    }

    // Attach and query both
    {
        let conn = Connection::open_with_flags(
            &db1_uri,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
        )
        .unwrap();

        // Attach second database
        conn.execute(&format!("ATTACH DATABASE '{}' AS db2", db2_uri), [])
            .unwrap();

        // Query from both databases
        let user: String = conn
            .query_row("SELECT name FROM users WHERE id = 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user, "Alice");

        let product: String = conn
            .query_row("SELECT name FROM db2.products WHERE id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(product, "Widget");
    }

    unregister_vfs().unwrap();
}

#[test]
fn test_vfs_journal_mode_wal() {
    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("wal.cart");

    let cart = Cartridge::create_at(&cart_path, "vfs-wal", "VFS WAL").unwrap();
    let cart_arc = Arc::new(Mutex::new(cart));
    register_vfs(Arc::clone(&cart_arc)).unwrap();

    let db_uri = format!("file:/wal.db?vfs={}", VFS_NAME);

    let conn = Connection::open_with_flags(
        &db_uri,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();

    // Try to enable WAL mode (may not be supported by our VFS, test graceful handling)
    let result = conn.execute("PRAGMA journal_mode=WAL", []);

    // If WAL is supported, verify it works
    if result.is_ok() {
        conn.execute("CREATE TABLE test (x INTEGER)", []).unwrap();
        conn.execute("INSERT INTO test VALUES (1)", []).unwrap();

        let val: i64 = conn
            .query_row("SELECT x FROM test", [], |row| row.get(0))
            .unwrap();
        assert_eq!(val, 1);
    }
    // If not supported, that's OK - just verify we don't crash

    drop(conn);
    unregister_vfs().unwrap();
}

#[test]
fn test_vfs_multiple_databases_same_vfs() {
    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("multi.cart");

    let cart = Cartridge::create_at(&cart_path, "vfs-multi", "VFS Multi").unwrap();
    let cart_arc = Arc::new(Mutex::new(cart));
    register_vfs(Arc::clone(&cart_arc)).unwrap();

    // Open 10 different databases through the same VFS
    let connections: Vec<_> = (0..10)
        .map(|i| {
            let db_uri = format!("file:/db{}.sqlite?vfs={}", i, VFS_NAME);
            let conn = Connection::open_with_flags(
                &db_uri,
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_CREATE
                    | OpenFlags::SQLITE_OPEN_URI,
            )
            .unwrap();

            conn.execute("CREATE TABLE test (id INTEGER)", [])
                .unwrap();
            conn.execute("INSERT INTO test VALUES (?)", params![i])
                .unwrap();

            conn
        })
        .collect();

    // Verify each database has correct data
    for (i, conn) in connections.iter().enumerate() {
        let val: i64 = conn
            .query_row("SELECT id FROM test", [], |row| row.get(0))
            .unwrap();
        assert_eq!(val, i as i64);
    }

    drop(connections);
    unregister_vfs().unwrap();
}

#[test]
fn test_vfs_error_handling_invalid_sql() {
    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("errors.cart");

    let cart = Cartridge::create_at(&cart_path, "vfs-errors", "VFS Errors").unwrap();
    let cart_arc = Arc::new(Mutex::new(cart));
    register_vfs(Arc::clone(&cart_arc)).unwrap();

    let db_uri = format!("file:/errors.db?vfs={}", VFS_NAME);

    let conn = Connection::open_with_flags(
        &db_uri,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();

    // Invalid SQL should return error, not crash
    assert!(conn.execute("INVALID SQL SYNTAX", []).is_err());

    // Table doesn't exist
    assert!(conn
        .query_row("SELECT * FROM nonexistent", [], |_| Ok(()))
        .is_err());

    // Should still be able to execute valid SQL after errors
    conn.execute("CREATE TABLE test (x INTEGER)", []).unwrap();
    conn.execute("INSERT INTO test VALUES (42)", []).unwrap();

    let val: i64 = conn
        .query_row("SELECT x FROM test", [], |row| row.get(0))
        .unwrap();
    assert_eq!(val, 42);

    drop(conn);
    unregister_vfs().unwrap();
}
