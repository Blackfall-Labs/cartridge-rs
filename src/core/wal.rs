//! Write-Ahead Log for crash-safe cartridge operations.
//!
//! WAL files live inside the cartridge VFS at `/wal/<system>/wal.log`.
//! Each WAL file is pre-allocated with fixed-size entries so that writing
//! a log entry is a raw page overwrite — no allocator or catalog mutation
//! needed. This breaks the chicken-and-egg problem.
//!
//! ## Entry lifecycle
//!
//! ```text
//! Intent → Written → Committed → (cleared on checkpoint)
//! ```
//!
//! Crash at any point is recoverable:
//! - Before Written: nothing happened, discard intent
//! - After Written, before Committed: data in both locations, redo or discard
//! - After Committed: operation complete, clear on next open

use crate::error::{CartridgeError, Result};
use crate::header::PAGE_SIZE;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Magic bytes for WAL header: "WALH"
const WAL_MAGIC: [u8; 4] = *b"WALH";

/// Current WAL format version.
const WAL_VERSION: u16 = 1;

/// Fixed size of a single WAL entry in bytes.
/// Chosen as a power of 2 for alignment. Entries pack tightly after the header.
pub const WAL_ENTRY_SIZE: usize = 64;

/// WAL header occupies the first portion of page 0 of the WAL file.
/// Everything after the header (up to page boundary) is padding.
const WAL_HEADER_SIZE: usize = 32;

/// How many entries fit in a single 4KB page (after the header page).
const ENTRIES_PER_PAGE: usize = PAGE_SIZE / WAL_ENTRY_SIZE; // 64

/// Default pre-allocated pages for a WAL file (1 header + 7 data = 8 pages).
/// Gives 7 * 64 = 448 entry slots — plenty for incremental vacuum.
pub const DEFAULT_WAL_PAGES: usize = 8;

/// Maximum entries in default allocation.
pub const DEFAULT_WAL_CAPACITY: u32 = ((DEFAULT_WAL_PAGES - 1) * ENTRIES_PER_PAGE) as u32;

/// Reserved VFS prefix for WAL files.
pub const WAL_PREFIX: &str = "wal/";

// ---------------------------------------------------------------------------
// Op types
// ---------------------------------------------------------------------------

/// Operation type stored in a WAL entry.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalOp {
    /// Relocate a content page from source to dest.
    VacuumRelocate = 0x01,
    /// Free source page after successful relocate.
    VacuumFree = 0x02,
    /// Truncate the cartridge file after all relocations.
    VacuumTruncate = 0x03,
}

impl WalOp {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::VacuumRelocate),
            0x02 => Some(Self::VacuumFree),
            0x03 => Some(Self::VacuumTruncate),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry state
// ---------------------------------------------------------------------------

/// Lifecycle state of a WAL entry.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalState {
    /// Intent recorded, operation not yet started.
    Intent = 0,
    /// Data written to destination, source still valid.
    Written = 1,
    /// Operation fully committed (catalog updated, source freed).
    Committed = 2,
}

impl WalState {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Intent),
            1 => Some(Self::Written),
            2 => Some(Self::Committed),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// WAL Entry (64 bytes, fixed layout)
// ---------------------------------------------------------------------------

/// A single WAL entry. Fixed 64 bytes.
///
/// Layout:
/// ```text
/// [0..8]   sequence:  u64 LE
/// [8]      op:        u8
/// [9]      state:     u8
/// [10..18] source:    u64 LE  (source page ID)
/// [18..26] dest:      u64 LE  (dest page ID or new_total_blocks for truncate)
/// [26..34] path_hash: u64 LE  (FNV-1a of catalog path, for fast lookup)
/// [34..38] blk_index: u32 LE  (index into file's blocks vec)
/// [38..42] checksum:  u32 LE  (CRC32 of bytes 0..38)
/// [42..64] reserved:  22 bytes zero
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalEntry {
    pub sequence: u64,
    pub op: WalOp,
    pub state: WalState,
    pub source_page: u64,
    pub dest_page: u64,
    pub path_hash: u64,
    pub block_index: u32,
    pub checksum: u32,
}

impl WalEntry {
    /// Serialize to exactly 64 bytes.
    pub fn to_bytes(&self) -> [u8; WAL_ENTRY_SIZE] {
        let mut buf = [0u8; WAL_ENTRY_SIZE];
        buf[0..8].copy_from_slice(&self.sequence.to_le_bytes());
        buf[8] = self.op as u8;
        buf[9] = self.state as u8;
        buf[10..18].copy_from_slice(&self.source_page.to_le_bytes());
        buf[18..26].copy_from_slice(&self.dest_page.to_le_bytes());
        buf[26..34].copy_from_slice(&self.path_hash.to_le_bytes());
        buf[34..38].copy_from_slice(&self.block_index.to_le_bytes());
        // Compute CRC32 over bytes 0..38
        let crc = crc32_quick(&buf[0..38]);
        buf[38..42].copy_from_slice(&crc.to_le_bytes());
        buf
    }

    /// Deserialize from exactly 64 bytes. Returns None if corrupt.
    pub fn from_bytes(buf: &[u8; WAL_ENTRY_SIZE]) -> Option<Self> {
        let sequence = u64::from_le_bytes(buf[0..8].try_into().ok()?);
        // Sequence 0 means empty slot
        if sequence == 0 {
            return None;
        }

        let op = WalOp::from_u8(buf[8])?;
        let state = WalState::from_u8(buf[9])?;
        let source_page = u64::from_le_bytes(buf[10..18].try_into().ok()?);
        let dest_page = u64::from_le_bytes(buf[18..26].try_into().ok()?);
        let path_hash = u64::from_le_bytes(buf[26..34].try_into().ok()?);
        let block_index = u32::from_le_bytes(buf[34..38].try_into().ok()?);
        let stored_crc = u32::from_le_bytes(buf[38..42].try_into().ok()?);

        // Verify CRC
        let computed_crc = crc32_quick(&buf[0..38]);
        if stored_crc != computed_crc {
            return None; // Torn write — treat as empty
        }

        Some(WalEntry {
            sequence,
            op,
            state,
            checksum: stored_crc,
            source_page,
            dest_page,
            path_hash,
            block_index,
        })
    }
}

// ---------------------------------------------------------------------------
// WAL Header (32 bytes, start of first page)
// ---------------------------------------------------------------------------

/// WAL file header. Occupies the first 32 bytes of the WAL file's first page.
///
/// Layout:
/// ```text
/// [0..4]   magic:       "WALH"
/// [4..6]   version:     u16 LE
/// [6..8]   entry_size:  u16 LE (always 64)
/// [8..12]  entry_count: u32 LE (valid entries currently in the log)
/// [12..20] sequence:    u64 LE (next sequence number to assign)
/// [20..24] capacity:    u32 LE (max entries)
/// [24]     state:       u8 (0=clean, 1=dirty)
/// [25..32] reserved:    7 bytes zero
/// ```
#[derive(Debug, Clone, Copy)]
pub struct WalHeader {
    pub entry_count: u32,
    pub sequence: u64,
    pub capacity: u32,
    pub dirty: bool,
}

impl WalHeader {
    pub fn new(capacity: u32) -> Self {
        WalHeader {
            entry_count: 0,
            sequence: 1, // Start at 1 so 0 means empty slot
            capacity,
            dirty: false,
        }
    }

    pub fn to_bytes(&self) -> [u8; WAL_HEADER_SIZE] {
        let mut buf = [0u8; WAL_HEADER_SIZE];
        buf[0..4].copy_from_slice(&WAL_MAGIC);
        buf[4..6].copy_from_slice(&WAL_VERSION.to_le_bytes());
        buf[6..8].copy_from_slice(&(WAL_ENTRY_SIZE as u16).to_le_bytes());
        buf[8..12].copy_from_slice(&self.entry_count.to_le_bytes());
        buf[12..20].copy_from_slice(&self.sequence.to_le_bytes());
        buf[20..24].copy_from_slice(&self.capacity.to_le_bytes());
        buf[24] = if self.dirty { 1 } else { 0 };
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        if buf.len() < WAL_HEADER_SIZE {
            return Err(CartridgeError::Corruption(
                "WAL header too short".into(),
            ));
        }
        if buf[0..4] != WAL_MAGIC {
            return Err(CartridgeError::Corruption(
                "WAL magic mismatch".into(),
            ));
        }
        let version = u16::from_le_bytes([buf[4], buf[5]]);
        if version != WAL_VERSION {
            return Err(CartridgeError::Corruption(format!(
                "WAL version {version}, expected {WAL_VERSION}"
            )));
        }
        let entry_count = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        let sequence = u64::from_le_bytes(buf[12..20].try_into().unwrap());
        let capacity = u32::from_le_bytes(buf[20..24].try_into().unwrap());
        let dirty = buf[24] != 0;

        Ok(WalHeader {
            entry_count,
            sequence,
            capacity,
            dirty,
        })
    }
}

// ---------------------------------------------------------------------------
// WAL File — in-memory handle backed by cartridge VFS pages
// ---------------------------------------------------------------------------

/// Manages a WAL file stored in cartridge VFS pages.
///
/// The WAL file's pages are pre-allocated in the cartridge. Writing entries
/// is a direct page overwrite — no allocator or catalog changes needed.
///
/// Page layout:
/// - Page 0: header (32 bytes) + entry slots for remainder of page
/// - Page 1+: entry slots (64 entries per page)
pub struct WalFile {
    /// The page IDs in the cartridge that back this WAL file.
    /// Index 0 = header page, 1+ = data pages.
    pages: Vec<u64>,

    /// Parsed header.
    header: WalHeader,

    /// All valid entries, in sequence order.
    entries: Vec<WalEntry>,
}

impl WalFile {
    /// Create a new empty WAL with the given backing pages.
    ///
    /// `pages` must have at least 2 elements (1 header + 1 data page).
    pub fn new(pages: Vec<u64>) -> Result<Self> {
        if pages.len() < 2 {
            return Err(CartridgeError::Allocation(
                "WAL needs at least 2 pages".into(),
            ));
        }

        // Header page 0 holds the header + some entries.
        // Remaining pages hold only entries.
        let header_entry_slots = (PAGE_SIZE - WAL_HEADER_SIZE) / WAL_ENTRY_SIZE;
        let data_entry_slots = (pages.len() - 1) * ENTRIES_PER_PAGE;
        let capacity = (header_entry_slots + data_entry_slots) as u32;

        Ok(WalFile {
            pages,
            header: WalHeader::new(capacity),
            entries: Vec::new(),
        })
    }

    /// Load a WAL from raw page data read from the cartridge.
    ///
    /// `page_data` is a vec of (page_id, 4KB buffer) pairs in order.
    pub fn load(page_data: Vec<(u64, Vec<u8>)>) -> Result<Self> {
        if page_data.is_empty() {
            return Err(CartridgeError::Corruption("WAL has no pages".into()));
        }

        let pages: Vec<u64> = page_data.iter().map(|(id, _)| *id).collect();

        // Parse header from first page
        let header = WalHeader::from_bytes(&page_data[0].1)?;

        // Parse entries from all pages
        let mut entries = Vec::new();

        // Entries in header page start after the header
        let header_page = &page_data[0].1;
        let mut offset = WAL_HEADER_SIZE;
        while offset + WAL_ENTRY_SIZE <= PAGE_SIZE {
            let chunk: &[u8; WAL_ENTRY_SIZE] = header_page[offset..offset + WAL_ENTRY_SIZE]
                .try_into()
                .unwrap();
            if let Some(entry) = WalEntry::from_bytes(chunk) {
                entries.push(entry);
            }
            offset += WAL_ENTRY_SIZE;
        }

        // Entries in remaining pages
        for (_page_id, data) in page_data.iter().skip(1) {
            offset = 0;
            while offset + WAL_ENTRY_SIZE <= PAGE_SIZE {
                let chunk: &[u8; WAL_ENTRY_SIZE] =
                    data[offset..offset + WAL_ENTRY_SIZE].try_into().unwrap();
                if let Some(entry) = WalEntry::from_bytes(chunk) {
                    entries.push(entry);
                }
                offset += WAL_ENTRY_SIZE;
            }
        }

        // Sort by sequence for deterministic replay order
        entries.sort_by_key(|e| e.sequence);

        Ok(WalFile {
            pages,
            header,
            entries,
        })
    }

    /// Append a new entry. Returns the entry with its assigned sequence number
    /// and the (page_id, offset, bytes) needed to write it to disk.
    ///
    /// The caller is responsible for writing the bytes to the cartridge file
    /// and calling fsync. This separation keeps the WAL module free of IO.
    pub fn append(&mut self, op: WalOp, state: WalState, source: u64, dest: u64, path_hash: u64, block_index: u32) -> Result<(WalEntry, WalWrite)> {
        if self.entries.len() as u32 >= self.header.capacity {
            return Err(CartridgeError::Allocation("WAL full".into()));
        }

        let seq = self.header.sequence;
        self.header.sequence += 1;
        self.header.entry_count += 1;
        self.header.dirty = true;

        let entry = WalEntry {
            sequence: seq,
            op,
            state,
            source_page: source,
            dest_page: dest,
            path_hash,
            block_index,
            checksum: 0, // filled by to_bytes
        };

        let slot_index = self.entries.len();
        let write = self.slot_location(slot_index);

        self.entries.push(entry);

        Ok((entry, WalWrite {
            page_id: write.0,
            offset_in_page: write.1,
            data: entry.to_bytes().to_vec(),
        }))
    }

    /// Update the state of an existing entry (identified by sequence number).
    /// Returns the write descriptor for the caller to persist.
    pub fn update_state(&mut self, sequence: u64, new_state: WalState) -> Result<WalWrite> {
        let idx = self.entries.iter().position(|e| e.sequence == sequence)
            .ok_or_else(|| CartridgeError::Corruption(format!(
                "WAL entry seq={sequence} not found"
            )))?;

        self.entries[idx].state = new_state;
        let entry = self.entries[idx];
        let loc = self.slot_location(idx);

        Ok(WalWrite {
            page_id: loc.0,
            offset_in_page: loc.1,
            data: entry.to_bytes().to_vec(),
        })
    }

    /// Get all uncommitted entries (state != Committed) for recovery.
    pub fn pending_entries(&self) -> Vec<&WalEntry> {
        self.entries.iter().filter(|e| e.state != WalState::Committed).collect()
    }

    /// Get all entries.
    pub fn entries(&self) -> &[WalEntry] {
        &self.entries
    }

    /// Clear all entries — used after vacuum completes or after recovery.
    /// Returns writes needed: zeroed entry slots + clean header.
    pub fn clear(&mut self) -> Vec<WalWrite> {
        let mut writes = Vec::new();

        // Zero out every entry slot
        let zero_entry = [0u8; WAL_ENTRY_SIZE];
        for i in 0..self.entries.len() {
            let loc = self.slot_location(i);
            writes.push(WalWrite {
                page_id: loc.0,
                offset_in_page: loc.1,
                data: zero_entry.to_vec(),
            });
        }

        self.entries.clear();
        self.header.entry_count = 0;
        self.header.dirty = false;

        // Write clean header
        writes.push(self.header_write());

        writes
    }

    /// Serialize the header for writing.
    pub fn header_write(&self) -> WalWrite {
        WalWrite {
            page_id: self.pages[0],
            offset_in_page: 0,
            data: self.header.to_bytes().to_vec(),
        }
    }

    /// The page IDs backing this WAL file.
    pub fn page_ids(&self) -> &[u64] {
        &self.pages
    }

    /// Is the WAL dirty (has uncommitted work)?
    pub fn is_dirty(&self) -> bool {
        self.header.dirty
    }

    /// Current entry count.
    pub fn entry_count(&self) -> u32 {
        self.header.entry_count
    }

    /// Maximum entry capacity.
    pub fn capacity(&self) -> u32 {
        self.header.capacity
    }

    // -----------------------------------------------------------------------
    // Internal
    // -----------------------------------------------------------------------

    /// Map a slot index to (page_id, byte_offset_within_page).
    ///
    /// Slots 0..N fit in the header page after the header bytes.
    /// Remaining slots spill into subsequent pages.
    fn slot_location(&self, slot_index: usize) -> (u64, usize) {
        let header_slots = (PAGE_SIZE - WAL_HEADER_SIZE) / WAL_ENTRY_SIZE;

        if slot_index < header_slots {
            // Fits in header page
            let offset = WAL_HEADER_SIZE + slot_index * WAL_ENTRY_SIZE;
            (self.pages[0], offset)
        } else {
            // Spills into data pages
            let data_slot = slot_index - header_slots;
            let page_index = 1 + data_slot / ENTRIES_PER_PAGE;
            let offset = (data_slot % ENTRIES_PER_PAGE) * WAL_ENTRY_SIZE;
            (self.pages[page_index], offset)
        }
    }
}

// ---------------------------------------------------------------------------
// Write descriptor — what the caller needs to persist
// ---------------------------------------------------------------------------

/// Describes a write that must be applied to the cartridge file.
///
/// The WAL module computes these but never touches IO directly.
/// The Cartridge performs the actual seeks, writes, and fsyncs.
#[derive(Debug, Clone)]
pub struct WalWrite {
    /// Which cartridge page to write to.
    pub page_id: u64,
    /// Byte offset within the page.
    pub offset_in_page: usize,
    /// The bytes to write at that offset.
    pub data: Vec<u8>,
}

// ---------------------------------------------------------------------------
// FNV-1a hash for catalog path → u64
// ---------------------------------------------------------------------------

/// FNV-1a hash of a string to u64. Used for path_hash in WAL entries.
/// Not cryptographic — just a fast, well-distributed hash for correlation.
pub fn fnv1a_hash(s: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001B3;
    let mut hash = FNV_OFFSET;
    for byte in s.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

// ---------------------------------------------------------------------------
// CRC32 (minimal, no dependency)
// ---------------------------------------------------------------------------

/// Quick CRC32 (ISO 3309 / ITU-T V.42) over a byte slice.
/// Used to detect torn writes in WAL entries.
fn crc32_quick(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_round_trip() {
        let entry = WalEntry {
            sequence: 42,
            op: WalOp::VacuumRelocate,
            state: WalState::Intent,
            source_page: 1000,
            dest_page: 5,
            path_hash: fnv1a_hash("databases/pm.db"),
            block_index: 7,
            checksum: 0,
        };

        let bytes = entry.to_bytes();
        assert_eq!(bytes.len(), WAL_ENTRY_SIZE);

        let parsed = WalEntry::from_bytes(&bytes).expect("should parse");
        assert_eq!(parsed.sequence, 42);
        assert_eq!(parsed.op, WalOp::VacuumRelocate);
        assert_eq!(parsed.state, WalState::Intent);
        assert_eq!(parsed.source_page, 1000);
        assert_eq!(parsed.dest_page, 5);
        assert_eq!(parsed.block_index, 7);
    }

    #[test]
    fn empty_slot_returns_none() {
        let zeros = [0u8; WAL_ENTRY_SIZE];
        assert!(WalEntry::from_bytes(&zeros).is_none());
    }

    #[test]
    fn corrupt_crc_returns_none() {
        let entry = WalEntry {
            sequence: 1,
            op: WalOp::VacuumFree,
            state: WalState::Written,
            source_page: 100,
            dest_page: 0,
            path_hash: 0,
            block_index: 0,
            checksum: 0,
        };
        let mut bytes = entry.to_bytes();
        bytes[39] ^= 0xFF; // flip a CRC byte
        assert!(WalEntry::from_bytes(&bytes).is_none());
    }

    #[test]
    fn header_round_trip() {
        let header = WalHeader {
            entry_count: 10,
            sequence: 55,
            capacity: 448,
            dirty: true,
        };

        let bytes = header.to_bytes();
        let parsed = WalHeader::from_bytes(&bytes).expect("should parse");
        assert_eq!(parsed.entry_count, 10);
        assert_eq!(parsed.sequence, 55);
        assert_eq!(parsed.capacity, 448);
        assert!(parsed.dirty);
    }

    #[test]
    fn wal_file_append_and_update() {
        // Simulate 8 pre-allocated pages (IDs 100-107)
        let pages: Vec<u64> = (100..108).collect();
        let mut wal = WalFile::new(pages).unwrap();

        assert_eq!(wal.entry_count(), 0);
        assert!(!wal.is_dirty());

        // Append an entry
        let (entry, write) = wal.append(
            WalOp::VacuumRelocate,
            WalState::Intent,
            847, 4,
            fnv1a_hash("databases/pm.db"),
            2,
        ).unwrap();

        assert_eq!(entry.sequence, 1);
        assert_eq!(write.page_id, 100); // header page
        assert_eq!(write.offset_in_page, WAL_HEADER_SIZE); // first slot after header
        assert_eq!(wal.entry_count(), 1);
        assert!(wal.is_dirty());

        // Update state to Written
        let write2 = wal.update_state(1, WalState::Written).unwrap();
        assert_eq!(write2.page_id, 100);
        assert_eq!(wal.entries()[0].state, WalState::Written);

        // Update state to Committed
        let _write3 = wal.update_state(1, WalState::Committed).unwrap();
        assert_eq!(wal.entries()[0].state, WalState::Committed);
        assert!(wal.pending_entries().is_empty());
    }

    #[test]
    fn wal_file_clear() {
        let pages: Vec<u64> = (0..8).collect();
        let mut wal = WalFile::new(pages).unwrap();

        wal.append(WalOp::VacuumRelocate, WalState::Intent, 100, 3, 0, 0).unwrap();
        wal.append(WalOp::VacuumRelocate, WalState::Intent, 101, 4, 0, 1).unwrap();
        assert_eq!(wal.entry_count(), 2);

        let writes = wal.clear();
        assert_eq!(wal.entry_count(), 0);
        assert!(!wal.is_dirty());
        // 2 zero-out writes + 1 header write
        assert_eq!(writes.len(), 3);
    }

    #[test]
    fn wal_file_load_from_pages() {
        // Build a WAL, serialize its pages, then reload
        let page_ids: Vec<u64> = (50..58).collect();
        let mut wal = WalFile::new(page_ids.clone()).unwrap();

        let (_, _) = wal.append(WalOp::VacuumRelocate, WalState::Committed, 200, 3, 123, 0).unwrap();
        let (_, _) = wal.append(WalOp::VacuumRelocate, WalState::Written, 201, 4, 456, 1).unwrap();

        // Serialize to pages
        let page_data = serialize_wal_to_pages(&wal);

        // Reload
        let loaded = WalFile::load(page_data).unwrap();
        assert_eq!(loaded.entry_count(), 2);
        assert_eq!(loaded.entries()[0].source_page, 200);
        assert_eq!(loaded.entries()[1].source_page, 201);
        assert_eq!(loaded.pending_entries().len(), 1); // only the Written one
    }

    #[test]
    fn slot_location_spans_pages() {
        let pages: Vec<u64> = (0..4).collect();
        let wal = WalFile::new(pages).unwrap();

        // First slot in header page
        let (pid, off) = wal.slot_location(0);
        assert_eq!(pid, 0);
        assert_eq!(off, WAL_HEADER_SIZE);

        // Slots that fit in header page
        let header_slots = (PAGE_SIZE - WAL_HEADER_SIZE) / WAL_ENTRY_SIZE;
        let (pid, off) = wal.slot_location(header_slots - 1);
        assert_eq!(pid, 0);

        // First slot in data page 1
        let (pid, off) = wal.slot_location(header_slots);
        assert_eq!(pid, 1);
        assert_eq!(off, 0);

        // First slot in data page 2
        let (pid, off) = wal.slot_location(header_slots + ENTRIES_PER_PAGE);
        assert_eq!(pid, 2);
        assert_eq!(off, 0);
    }

    #[test]
    fn fnv1a_deterministic() {
        let h1 = fnv1a_hash("databases/pm.db");
        let h2 = fnv1a_hash("databases/pm.db");
        assert_eq!(h1, h2);

        let h3 = fnv1a_hash("databases/vcs.db");
        assert_ne!(h1, h3);
    }

    #[test]
    fn crc32_detects_changes() {
        let data = b"hello world";
        let crc1 = crc32_quick(data);

        let mut modified = *data;
        modified[5] = b'_';
        let crc2 = crc32_quick(&modified);

        assert_ne!(crc1, crc2);
    }

    // -----------------------------------------------------------------------
    // Test helper — serialize WAL state into page buffers
    // -----------------------------------------------------------------------

    fn serialize_wal_to_pages(wal: &WalFile) -> Vec<(u64, Vec<u8>)> {
        let mut page_data: Vec<(u64, Vec<u8>)> = wal.page_ids()
            .iter()
            .map(|&id| (id, vec![0u8; PAGE_SIZE]))
            .collect();

        // Write header
        let hdr_bytes = wal.header_write();
        page_data[0].1[..hdr_bytes.data.len()].copy_from_slice(&hdr_bytes.data);

        // Write entries
        for (i, entry) in wal.entries().iter().enumerate() {
            let (page_id, offset) = wal.slot_location(i);
            let page_idx = wal.page_ids().iter().position(|&p| p == page_id).unwrap();
            let bytes = entry.to_bytes();
            page_data[page_idx].1[offset..offset + WAL_ENTRY_SIZE].copy_from_slice(&bytes);
        }

        page_data
    }
}
