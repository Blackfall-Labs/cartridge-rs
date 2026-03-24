# Plan: Universal WAL + Incremental Vacuum

## WAL Architecture

### VFS Layout
```
/wal/                           (protected, never vacuumed)
  vacuum/wal.log                page relocation journal
  checkpoint/wal.log            flush/checkpoint journal
  <system>/wal.log              per-system journals (future)
```

`/wal/` is a reserved prefix. Vacuum skips it. Delete operations reject it. It's infrastructure.

### WAL File Format

Pre-allocated file. Fixed-size entries so writes are page-aligned overwrites, no allocation needed.

```
WAL Header (first 4KB page of the file):
  magic: [u8; 4]       = "WALH"
  version: u16          = 1
  entry_size: u16       = size of one WAL entry (fixed)
  entry_count: u32      = number of valid entries
  sequence: u64         = monotonic sequence counter (survives crashes)
  capacity: u32         = max entries before rotation needed
  state: u8             = 0=clean, 1=dirty (has uncommitted work)

WAL Entry (fixed-size, packed sequentially after header):
  sequence: u64         = entry sequence number
  op: u8                = operation type
  state: u8             = 0=intent, 1=written, 2=committed
  source_page: u64      = page being relocated (vacuum) or written (checkpoint)
  dest_page: u64        = target page
  file_path_hash: u64   = hash of catalog path (fast lookup, not for recovery)
  block_index: u32      = index into file's blocks vec
  checksum: u32         = CRC32 of this entry (detect torn writes)
  _reserved: [u8; N]    = pad to power-of-2 entry size (64 bytes total)
```

### Op Types
```
0x01  VACUUM_RELOCATE     move content page from source to dest
0x02  VACUUM_FREE         mark source page free after relocate
0x03  VACUUM_TRUNCATE     shrink file after all relocations done
0x10  CHECKPOINT_WRITE    (future) journaled catalog/allocator write
```

### Bootstrap — No Chicken-and-Egg

WAL file is created once with pre-allocated pages (e.g., 8 pages = 32KB = ~500 entries at 64 bytes each). After creation, writing a WAL entry is:

1. Seek to `header_page_offset + sizeof(header) + entry_index * entry_size`
2. Write fixed-size entry bytes
3. `fsync()`

No allocator. No catalog mutation. Just raw page overwrites on pages that already belong to the WAL file.

The WAL file is created by `Cartridge::ensure_wal(system)` — called lazily on first use. Creation itself is a normal VFS `create_file` + pre-size. If we crash during creation, the file either exists (usable) or doesn't (will be created next time).

### Recovery on Open

`Cartridge::open()` checks for `/wal/*/wal.log`. For each:

1. Read header. If `state == clean`, skip.
2. Scan entries. For each entry with `state != committed`:
   - `VACUUM_RELOCATE` with `state=intent`: nothing happened. Discard.
   - `VACUUM_RELOCATE` with `state=written`: data copied but catalog not updated. Check if dest page has valid content (CRC or content match). If yes, complete the commit (update catalog, free source). If no, discard (source still valid).
3. Clear processed entries, set `state=clean`, fsync.

Recovery is idempotent. Running it twice changes nothing.

---

## Incremental Vacuum

### Algorithm

```
fn vacuum_step(&mut self, batch_size: usize) -> Result<VacuumProgress>
```

Each call relocates up to `batch_size` pages (default: 1-4). Returns progress so caller can decide when to stop.

**Phase 1 — Plan:**
1. Collect live page set from catalog (all file block vecs + overflow pages)
2. Find highest live page ID = `high_water`
3. Find free slots below `high_water`
4. Build relocation plan: move high pages into low free slots

**Phase 2 — Execute (per step):**
1. Pick next relocation from plan
2. Write WAL entry: `VACUUM_RELOCATE, state=intent, src=high, dst=low`
3. Copy page content: read src page, write to dst page, fsync
4. Update WAL entry: `state=written`
5. Update catalog: file's `blocks[index]` = dst. Mark src free in allocator.
6. Update WAL entry: `state=committed`
7. Dirty the catalog (will be persisted on next flush)

**Phase 3 — Truncate:**
1. All live pages are below new high_water
2. Write WAL: `VACUUM_TRUNCATE, new_total_blocks=X`
3. Shrink allocator: `shrink_capacity(new_total_blocks)`
4. Shrink file: `file.set_len(new_total_blocks * PAGE_SIZE)`
5. Update header: `total_blocks`, `free_blocks`
6. Flush
7. Clear WAL

### Allocator Shrink

New methods needed:

```rust
// bitmap.rs
pub fn shrink_capacity(&mut self, new_total_blocks: usize) -> Result<()>
  // Verify no allocated pages exist above new_total_blocks
  // Truncate bitmap vec
  // Update total_blocks, recalibrate free_blocks

// extent.rs
pub fn shrink_capacity(&mut self, new_total_blocks: usize) -> Result<()>
  // Verify no free extents reference pages above new_total_blocks
  // Remove/truncate extents above boundary
  // Update total_blocks, recalibrate free_blocks

// hybrid.rs
pub fn shrink_capacity(&mut self, new_total_blocks: usize) -> Result<()>
  // Delegate to both sub-allocators
  // Update total_blocks, recalibrate free_blocks
```

### Non-Blocking Integration

Vacuum runs between host operations, not during them:

- **Ternsig:** after periodic checkpoint (every 60s), run `vacuum_step(4)` if `needs_vacuum()`
- **Host writes during vacuum:** safe because:
  - WAL tracks what's in-flight
  - Only one page moves at a time
  - Catalog is the source of truth — if host reads a file, it reads from whatever page the catalog says
  - If host writes to a file whose page is being relocated, the write goes to wherever the catalog currently points. Vacuum's next step will see the catalog changed and adapt.

### VacuumProgress

```rust
pub struct VacuumProgress {
    pub pages_relocated: usize,
    pub pages_remaining: usize,
    pub bytes_reclaimable: u64,
    pub done: bool,
}
```

### needs_vacuum()

```rust
pub fn needs_vacuum(&self) -> bool {
    let stats = self.stats();
    let used = stats.used_blocks;
    let total = stats.total_blocks;
    // More than 50% waste OR more than 10MB of dead space
    let waste_ratio = 1.0 - (used as f64 / total as f64);
    let waste_bytes = (total - used) * PAGE_SIZE as u64;
    waste_ratio > 0.5 || waste_bytes > 10 * 1024 * 1024
}
```

---

## Implementation Order

1. **WAL file format** — `src/core/wal.rs`: WalHeader, WalEntry, WalFile (read/write/recover)
2. **ensure_wal()** — create WAL file in VFS with pre-allocated pages
3. **Recovery on open** — `Cartridge::open()` calls `recover_wal()` before returning
4. **shrink_capacity()** — bitmap, extent, hybrid allocators
5. **vacuum_step()** — single-batch incremental relocation using WAL
6. **needs_vacuum()** — threshold check
7. **VacuumProgress** — return type for callers
8. **Tests** — crash simulation (write WAL entry, don't commit, reopen, verify recovery)

## Not In Scope

- Per-database WAL for general writes (checkpoint journal) — future, same infrastructure
- WAL rotation/compaction — initial capacity is enough, revisit if WAL files grow
- Concurrent vacuum + write conflict resolution — single-threaded execution model means no true concurrency; interleaving is safe because each step is atomic
