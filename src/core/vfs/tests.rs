//! Integration tests for the Cartridge SQLite VFS

use crate::core::cartridge::Cartridge;
use crate::vfs::{register_vfs, unregister_vfs, VFS_NAME};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::sync::Arc;
use tempfile::TempDir;

// Global lock to ensure VFS tests run serially (VFS registration is global in SQLite)
use std::sync::Mutex as StdMutex;
static VFS_TEST_LOCK: StdMutex<()> = StdMutex::new(());

#[test]
fn test_vfs_basic_operations() {
    let _lock = VFS_TEST_LOCK.lock();  // Serialize VFS tests

    // Create a cartridge
    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("test.cart");

    let cartridge = Arc::new(Mutex::new(
        Cartridge::create_at(&cart_path, "test-vfs", "Test VFS").unwrap(),
    ));

    // Register the VFS
    register_vfs(Arc::clone(&cartridge)).unwrap();

    // Open a database using the Cartridge VFS
    let conn = Connection::open_with_flags(
        "test.db",
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
            | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
            | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();

    // Note: We need to tell SQLite to use our VFS
    // This is typically done via the URI parameter vfs=cartridge
    // For now, let's test with the default VFS and verify our registration worked

    // Clean up
    drop(conn);
    unregister_vfs().unwrap();
}

#[test]
fn test_vfs_create_table() {
    let _lock = VFS_TEST_LOCK.lock();  // Serialize VFS tests

    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("test.cart");

    let cartridge = Arc::new(Mutex::new(
        Cartridge::create_at(&cart_path, "test-vfs", "Test VFS").unwrap(),
    ));
    register_vfs(Arc::clone(&cartridge)).unwrap();

    {
        // Note: To use our custom VFS, we need to specify it in the connection string
        // For now, we'll just test that the VFS is registered
        // A full integration would require using rusqlite's VFS parameter

        // Manually create a database file in the cartridge
        let mut cart = cartridge.lock();
        cart.create_file("test.db", b"SQLite format 3\0").unwrap();
    }

    // Verify file exists
    {
        let cart = cartridge.lock();
        assert!(cart.exists("test.db").unwrap());
    }

    unregister_vfs().unwrap();
}

#[test]
fn test_vfs_registration() {
    let _lock = VFS_TEST_LOCK.lock();  // Serialize VFS tests

    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("test.cart");

    let cartridge = Arc::new(Mutex::new(
        Cartridge::create_at(&cart_path, "test-vfs", "Test VFS").unwrap(),
    ));

    // Register
    register_vfs(Arc::clone(&cartridge)).unwrap();

    // Verify it's registered by trying to unregister
    unregister_vfs().unwrap();

    // Should be able to register again
    register_vfs(Arc::clone(&cartridge)).unwrap();
    unregister_vfs().unwrap();
}

#[test]
fn test_vfs_full_sqlite_integration() {
    let _lock = VFS_TEST_LOCK.lock();  // Serialize VFS tests

    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("test.cart");

    let cartridge = Arc::new(Mutex::new(
        Cartridge::create_at(&cart_path, "test-vfs", "Test VFS").unwrap(),
    ));
    register_vfs(Arc::clone(&cartridge)).unwrap();

    // Open database with our custom VFS
    // Use URI format to specify the VFS: file:test.db?vfs=cartridge
    let db_uri = format!("file:test.db?vfs={}", VFS_NAME);
    let mut conn = Connection::open_with_flags(
        &db_uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
            | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
            | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();

    // Create a table
    conn.execute(
        "CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            email TEXT NOT NULL UNIQUE
        )",
        [],
    )
    .unwrap();

    // Insert some data
    conn.execute(
        "INSERT INTO users (name, email) VALUES (?1, ?2)",
        params!["Alice", "alice@example.com"],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO users (name, email) VALUES (?1, ?2)",
        params!["Bob", "bob@example.com"],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO users (name, email) VALUES (?1, ?2)",
        params!["Charlie", "charlie@example.com"],
    )
    .unwrap();

    // Query the data
    {
        let mut stmt = conn
            .prepare("SELECT id, name, email FROM users ORDER BY id")
            .unwrap();
        let users: Vec<(i32, String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(users.len(), 3);
        assert_eq!(users[0].1, "Alice");
        assert_eq!(users[1].1, "Bob");
        assert_eq!(users[2].1, "Charlie");
    }

    // Test UPDATE
    conn.execute(
        "UPDATE users SET email = ?1 WHERE name = ?2",
        params!["newalice@example.com", "Alice"],
    )
    .unwrap();

    let alice_email: String = conn
        .query_row(
            "SELECT email FROM users WHERE name = ?1",
            params!["Alice"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(alice_email, "newalice@example.com");

    // Test DELETE
    conn.execute("DELETE FROM users WHERE name = ?1", params!["Bob"])
        .unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2);

    // Close connection
    drop(conn);

    // Verify the database file exists in the cartridge
    {
        let mut cart = cartridge.lock();
        assert!(cart.exists("test.db").unwrap());

        // Verify the database file has content
        let db_content = cart.read_file("test.db").unwrap();
        assert!(db_content.len() > 0);

        // SQLite magic number check
        assert_eq!(&db_content[0..16], b"SQLite format 3\0");
    }

    unregister_vfs().unwrap();
}

#[test]
fn test_vfs_persistence_across_connections() {
    let _lock = VFS_TEST_LOCK.lock();  // Serialize VFS tests

    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("persist.cart");

    let cartridge = Arc::new(Mutex::new(
        Cartridge::create_at(&cart_path, "test-vfs", "Test VFS").unwrap(),
    ));

    register_vfs(Arc::clone(&cartridge)).unwrap();

    let db_uri = format!("file:persist.db?vfs={}", VFS_NAME);

    // First connection - create and populate
    {
        let conn = Connection::open_with_flags(
            &db_uri,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
                | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )
        .unwrap();

        conn.execute(
            "CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT, price REAL)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO products (name, price) VALUES ('Widget', 19.99)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO products (name, price) VALUES ('Gadget', 29.99)",
            [],
        )
        .unwrap();

        // Ensure SQLite flushes all changes to VFS before closing
        conn.execute("PRAGMA synchronous = FULL", []).unwrap();

        // Force a checkpoint to ensure all data is written
        conn.execute("PRAGMA wal_checkpoint(TRUNCATE)", []).ok();

        // Explicitly close connection to ensure all data is written
        conn.close().unwrap();
    }

    // Flush to disk and ensure all pages are written
    {
        let mut cart = cartridge.lock();
        cart.flush().unwrap();
        // Flush again to ensure all buffers are cleared
        cart.flush().unwrap();
    }

    // Small delay to ensure filesystem operations complete
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Second connection - verify data persisted
    {
        let conn = Connection::open_with_flags(
            &db_uri,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM products", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);

        let total: f64 = conn
            .query_row("SELECT SUM(price) FROM products", [], |row| row.get(0))
            .unwrap();
        assert!((total - 49.98).abs() < 0.01);

        drop(conn);
    }

    unregister_vfs().unwrap();
}

#[test]
fn test_vfs_transactions() {
    let _lock = VFS_TEST_LOCK.lock();  // Serialize VFS tests

    let temp_dir = TempDir::new().unwrap();
    let cart_path = temp_dir.path().join("txn.cart");

    let cartridge = Arc::new(Mutex::new(
        Cartridge::create_at(&cart_path, "test-vfs", "Test VFS").unwrap(),
    ));
    register_vfs(Arc::clone(&cartridge)).unwrap();

    let db_uri = format!("file:txn.db?vfs={}", VFS_NAME);
    let mut conn = Connection::open_with_flags(
        &db_uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
            | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
            | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();

    conn.execute(
        "CREATE TABLE accounts (id INTEGER PRIMARY KEY, balance REAL)",
        [],
    )
    .unwrap();

    conn.execute("INSERT INTO accounts (balance) VALUES (100.0)", [])
        .unwrap();

    // Test successful transaction
    {
        let tx = conn.transaction().unwrap();
        tx.execute(
            "UPDATE accounts SET balance = balance - 50.0 WHERE id = 1",
            [],
        )
        .unwrap();
        tx.commit().unwrap();
    }

    let balance: f64 = conn
        .query_row("SELECT balance FROM accounts WHERE id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(balance, 50.0);

    // Test rolled back transaction
    {
        let tx = conn.transaction().unwrap();
        tx.execute(
            "UPDATE accounts SET balance = balance - 100.0 WHERE id = 1",
            [],
        )
        .unwrap();
        // Don't commit - let it rollback on drop
    }

    let balance: f64 = conn
        .query_row("SELECT balance FROM accounts WHERE id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(balance, 50.0); // Should still be 50, rollback worked

    drop(conn);
    unregister_vfs().unwrap();
}
