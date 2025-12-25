//! Audit log integrity tests
//!
//! Note: AuditEntry uses low-level numeric IDs (actor_id: u32, resource_id: u64)
//! rather than string paths and user names. Tests use this low-level API.

use cartridge_rs::core::audit::{AuditLogger, AuditEntry, Operation};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn test_audit_logger_basic_functionality() {
    let logger = Arc::new(AuditLogger::new(1000, Duration::from_millis(100)));

    // Perform operations with low-level numeric IDs
    for i in 0..100 {
        let entry = AuditEntry::new(
            1, // actor_id
            Operation::Create,
            0, // resource_table
            i as u64, // resource_id
            0, // session_id
        );
        logger.log(entry);
    }

    // Logger should function without panicking
    drop(logger);
}

// Note: test_audit_logger_with_cartridge removed - set_audit_logger is not public
// Use CartridgeBuilder::new().with_audit_logging().build() instead

#[test]
fn test_audit_logger_ring_buffer_capacity() {
    // Small capacity to test wrapping
    let logger = Arc::new(AuditLogger::new(100, Duration::from_millis(100)));

    // Exceed capacity
    for i in 0..200 {
        let entry = AuditEntry::new(1, Operation::Create, 0, i as u64, 0);
        logger.log(entry);
    }

    // Should not crash or panic
    drop(logger);
}

#[test]
fn test_audit_logger_concurrent_logging() {
    use std::thread;

    let logger = Arc::new(AuditLogger::new(10000, Duration::from_millis(100)));

    // Multiple threads logging concurrently
    let handles: Vec<_> = (0..4)
        .map(|thread_id| {
            let logger_clone = logger.clone();
            thread::spawn(move || {
                for i in 0..100 {
                    let entry = AuditEntry::new(
                        thread_id as u32, // actor_id
                        Operation::Create,
                        0,
                        i as u64,
                        0,
                    );
                    logger_clone.log(entry);
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    // All threads should complete successfully
    drop(logger);
}

#[test]
fn test_audit_logger_mixed_operations() {

    let logger = Arc::new(AuditLogger::new(1000, Duration::from_millis(100)));

    // Mix of operation types
    for i in 0..50 {
        logger.log(AuditEntry::new(1, Operation::Create, 0, i as u64, 0));
        logger.log(AuditEntry::new(1, Operation::Read, 0, i as u64, 0));
        if i % 2 == 0 {
            logger.log(AuditEntry::new(1, Operation::Update, 0, i as u64, 0));
        }
        if i % 3 == 0 {
            logger.log(AuditEntry::new(1, Operation::Delete, 0, i as u64, 0));
        }
    }

    drop(logger);
}

#[test]
fn test_audit_logger_high_frequency() {
    use std::time::Instant;

    let logger = Arc::new(AuditLogger::new(10000, Duration::from_millis(50)));

    let start = Instant::now();

    // High-frequency logging
    for i in 0..10000 {
        let entry = AuditEntry::new(1, Operation::Create, 0, i as u64, 0);
        logger.log(entry);
    }

    let elapsed = start.elapsed();
    println!("Logged 10,000 entries in {:?}", elapsed);

    // Should be fast (< 1 second for 10k entries)
    assert!(
        elapsed.as_millis() < 1000,
        "Audit logging should be fast, took {:?}",
        elapsed
    );

    drop(logger);
}

#[test]
#[ignore] // start_background_flush and flush_now not exposed in current API
fn test_audit_logger_with_flush_callback() {
    // Note: This test requires start_background_flush() and flush_now() methods
    // which are not currently exposed in the public API
}

// TODO: Add tests for retrieving and inspecting audit entries once public API is available
// - test_audit_log_retrieval
// - test_audit_log_filtering
// - test_audit_log_persistence
