//! Audit logging for Cartridge operations
//!
//! Provides append-only audit trail for all file operations with:
//! - Lock-free ring buffer for high-performance logging
//! - Background flush thread for persistence
//! - Microsecond-precision timestamps
//! - Actor and session tracking

mod ring_buffer;

pub use ring_buffer::RingBuffer;

use parking_lot::Mutex;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Single audit log entry (32 bytes, cache-line friendly)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AuditEntry {
    /// Microsecond timestamp since UNIX epoch
    pub timestamp_us: u64,
    /// User or process ID that performed the operation
    pub actor_id: u32,
    /// Type of operation performed
    pub operation: Operation,
    /// Which resource table (0 = files, 1 = metadata, etc.)
    pub resource_table: u16,
    /// ID of the resource (file ID, metadata ID, etc.)
    pub resource_id: u64,
    /// Optional session ID for grouping related operations
    pub session_id: u32,
    /// Padding to align to 32 bytes
    _padding: u32,
}

impl AuditEntry {
    /// Create a new audit entry with current timestamp
    pub fn new(
        actor_id: u32,
        operation: Operation,
        resource_table: u16,
        resource_id: u64,
        session_id: u32,
    ) -> Self {
        let timestamp_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        AuditEntry {
            timestamp_us,
            actor_id,
            operation,
            resource_table,
            resource_id,
            session_id,
            _padding: 0,
        }
    }
}

/// Operation types for audit logging
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// File or resource creation
    Create = 0,
    /// Read access
    Read = 1,
    /// Modification
    Update = 2,
    /// Deletion
    Delete = 3,
    /// Metadata query
    Query = 4,
    /// Flush/sync operation
    Flush = 5,
}

/// High-performance audit logger with background flushing
pub struct AuditLogger {
    /// Lock-free ring buffer for audit entries
    ring_buffer: Arc<RingBuffer<AuditEntry>>,
    /// Background flush thread handle
    flush_thread: Option<JoinHandle<()>>,
    /// How often to flush entries to disk
    flush_interval: Duration,
    /// Whether the logger is running
    running: Arc<Mutex<bool>>,
}

impl AuditLogger {
    /// Create a new audit logger
    ///
    /// # Arguments
    /// * `capacity` - Ring buffer capacity (power of 2 recommended)
    /// * `flush_interval` - How often to flush entries to storage
    pub fn new(capacity: usize, flush_interval: Duration) -> Self {
        AuditLogger {
            ring_buffer: Arc::new(RingBuffer::new(capacity)),
            flush_thread: None,
            flush_interval,
            running: Arc::new(Mutex::new(false)),
        }
    }

    /// Start the background flush thread
    ///
    /// # Arguments
    /// * `flush_callback` - Function called with batches of audit entries
    pub fn start<F>(&mut self, flush_callback: F)
    where
        F: Fn(&[AuditEntry]) + Send + 'static,
    {
        *self.running.lock() = true;

        let ring_buffer = Arc::clone(&self.ring_buffer);
        let flush_interval = self.flush_interval;
        let running = Arc::clone(&self.running);

        let flush_thread = thread::spawn(move || {
            while *running.lock() {
                thread::sleep(flush_interval);

                // Read batch from ring buffer
                let entries = ring_buffer.read_batch(1000);
                if entries.is_empty() {
                    continue;
                }

                // Call flush callback
                flush_callback(&entries);
            }
        });

        self.flush_thread = Some(flush_thread);
    }

    /// Stop the background flush thread
    pub fn stop(&mut self) {
        *self.running.lock() = false;

        if let Some(thread) = self.flush_thread.take() {
            let _ = thread.join();
        }
    }

    /// Log an audit entry (non-blocking)
    pub fn log(&self, entry: AuditEntry) {
        self.ring_buffer.write(entry);
    }

    /// Convenience method to log a file operation
    pub fn log_file_op(&self, actor_id: u32, operation: Operation, file_id: u64, session_id: u32) {
        let entry = AuditEntry::new(actor_id, operation, 0, file_id, session_id);
        self.log(entry);
    }

    /// Get current ring buffer statistics
    pub fn stats(&self) -> (usize, usize) {
        self.ring_buffer.stats()
    }
}

impl Drop for AuditLogger {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_audit_entry_size() {
        // Ensure entry is exactly 32 bytes for cache-line efficiency
        assert_eq!(std::mem::size_of::<AuditEntry>(), 32);
    }

    #[test]
    fn test_audit_entry_creation() {
        let entry = AuditEntry::new(1, Operation::Create, 0, 42, 100);

        assert_eq!(entry.actor_id, 1);
        assert_eq!(entry.operation, Operation::Create);
        assert_eq!(entry.resource_table, 0);
        assert_eq!(entry.resource_id, 42);
        assert_eq!(entry.session_id, 100);
        assert!(entry.timestamp_us > 0);
    }

    #[test]
    fn test_audit_logger_basic() {
        let logger = AuditLogger::new(1024, Duration::from_millis(100));

        let entry = AuditEntry::new(1, Operation::Create, 0, 42, 100);
        logger.log(entry);

        let (write_pos, read_pos) = logger.stats();
        assert_eq!(write_pos, 1);
        assert_eq!(read_pos, 0);
    }

    #[test]
    fn test_audit_logger_flush() {
        let mut logger = AuditLogger::new(1024, Duration::from_millis(50));
        let flush_count = Arc::new(AtomicUsize::new(0));
        let flush_count_clone = Arc::clone(&flush_count);

        logger.start(move |entries| {
            flush_count_clone.fetch_add(entries.len(), Ordering::SeqCst);
        });

        // Log some entries
        for i in 0..100 {
            let entry = AuditEntry::new(1, Operation::Create, 0, i, 100);
            logger.log(entry);
        }

        // Wait for flush
        thread::sleep(Duration::from_millis(200));

        logger.stop();

        // Should have flushed entries
        assert!(flush_count.load(Ordering::SeqCst) > 0);
    }

    #[test]
    fn test_log_file_op_convenience() {
        let logger = AuditLogger::new(1024, Duration::from_millis(100));

        logger.log_file_op(1, Operation::Read, 42, 100);

        let (write_pos, _) = logger.stats();
        assert_eq!(write_pos, 1);
    }
}
