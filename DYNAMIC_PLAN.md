# Dynamic Growing Containers with Slug/Title Manifest

**Status**: In Progress
**Started**: 2025-11-21
**Goal**: Complete slug/title/manifest implementation AND make containers auto-grow by default.

---

## Overview

Transform Cartridge from requiring pre-allocated block counts to auto-growing containers that start minimal and expand as needed. Add strict kebab-case naming with slug/title distinction for better UX.

---

## PART 1: Finish Slug/Title/Manifest Implementation

### Completed ✅
- ✅ Added dependencies (`semver`, `validator`) to cartridge-core
- ✅ Created `src/validation.rs` with `ContainerSlug` newtype
- ✅ Created `src/manifest.rs` with `Manifest` struct (slug + title)
- ✅ Added error types: `InvalidContainerSlug`, `InvalidVersion`, `InvalidPath`, `ManifestNotFound`, `ManifestValidation`
- ✅ Exposed modules in lib.rs
- ✅ Library compiles successfully

### Remaining

#### 1. Fix Unused Import Warnings
- Remove `CartridgeError` from `manifest.rs` (line 8)
- Remove `validator::Validate` from `validation.rs` (line 9)

#### 2. Update `Cartridge::create()` Signature

**Current:**
```rust
pub fn create<P: AsRef<Path>>(path: P, total_blocks: usize) -> Result<Self>
```

**New:**
```rust
pub fn create<P: AsRef<Path>>(
    path: P,           // Slug or path (without .cart)
    slug: &str,        // Validated kebab-case identifier
    title: &str,       // Human-readable display name
) -> Result<Self>
```

**Implementation:**
- Use `validation::normalize_container_path()` to handle path
- Validate slug with `ContainerSlug::new()`
- Create with minimum initial size (3 blocks - see Part 2)
- Auto-create manifest at `/.cartridge/manifest.json`
- Store slug, title, version "0.1.0"

#### 3. Update `Cartridge::open()`

**Changes:**
- Use `validation::normalize_container_path()` to auto-append .cart
- Load manifest from `/.cartridge/manifest.json` if exists
- Warn (don't error) if manifest missing (backwards compatibility)
- Extract slug from manifest for consistency

#### 4. Add Manifest Methods to Cartridge

**New methods:**
```rust
impl Cartridge {
    /// Read container manifest
    pub fn read_manifest(&self) -> Result<Manifest>

    /// Write/update container manifest
    pub fn write_manifest(&mut self, manifest: &Manifest) -> Result<()>

    /// Get container slug
    pub fn slug(&self) -> Result<String>

    /// Get container title
    pub fn title(&self) -> Result<String>

    /// Update manifest with closure
    pub fn update_manifest<F>(&mut self, f: F) -> Result<()>
    where F: FnOnce(&mut Manifest)
}
```

**Manifest path constant:**
```rust
const MANIFEST_PATH: &str = "/.cartridge/manifest.json";
```

---

## PART 2: Add Auto-Growing Containers

### Design: Minimal Start + Auto-Grow

**Default behavior:**
- Start with **minimum necessary size**:
  - 1 page: header
  - 1 page: catalog root (B-tree)
  - 1 page: initial data space
  - **Total: 3 blocks (12KB)**
- Automatically grow when space needed
- No user-specified block count required

**Growth strategy:**
- Monitor free blocks percentage
- Grow when free blocks < 10%
- Double size each time (3 → 6 → 12 → 24 → 48 → 96...)
- Or grow by fixed increment (configurable)
- Update header with new total_blocks
- Extend allocator capacity

### Constants

```rust
// In cartridge.rs
const MIN_BLOCKS: usize = 3;              // Minimum: header + catalog + data
const DEFAULT_INITIAL_BLOCKS: usize = 3;  // Start minimal by default
const GROW_THRESHOLD: f64 = 0.10;         // Grow when <10% free
const GROW_FACTOR: usize = 2;             // Double size each time
const DEFAULT_MAX_BLOCKS: usize = 10_000_000;  // ~40GB safety limit
```

### Changes to Cartridge Struct

**Add fields:**
```rust
pub struct Cartridge {
    // ... existing fields ...

    /// Enable automatic growth (default: true)
    auto_grow: bool,

    /// Maximum blocks allowed (prevents runaway growth)
    max_blocks: usize,
}
```

### Auto-Grow Implementation

**New method:**
```rust
impl Cartridge {
    /// Ensure sufficient capacity, growing if needed
    fn ensure_capacity(&mut self, bytes_needed: usize) -> Result<()> {
        if !self.auto_grow {
            return Ok(()); // Manual management
        }

        let blocks_needed = (bytes_needed + PAGE_SIZE - 1) / PAGE_SIZE;
        let free_blocks = self.header.free_blocks as usize;

        // Check if we have enough free space
        if free_blocks >= blocks_needed {
            return Ok(());
        }

        // Calculate free percentage
        let free_pct = free_blocks as f64 / self.header.total_blocks as f64;

        // Grow if below threshold
        if free_pct < GROW_THRESHOLD {
            self.grow()?;
        }

        Ok(())
    }

    /// Grow container capacity
    fn grow(&mut self) -> Result<()> {
        let current = self.header.total_blocks as usize;
        let new_total = (current * GROW_FACTOR).min(self.max_blocks);

        if new_total == current {
            return Err(CartridgeError::OutOfSpace);
        }

        tracing::info!("Growing container: {} -> {} blocks", current, new_total);

        // Extend file
        if let Some(file) = &self.file {
            let mut f = file.lock();
            f.extend(new_total)?;
        }

        // Update header
        let added_blocks = new_total - current;
        self.header.total_blocks = new_total as u64;
        self.header.free_blocks += added_blocks as u64;

        // Extend allocator capacity
        self.allocator.extend_capacity(new_total)?;

        Ok(())
    }
}
```

**Update allocation call sites:**

In `create_file()`:
```rust
pub fn create_file(&mut self, path: &str, content: &[u8]) -> Result<()> {
    // Ensure capacity before allocating
    self.ensure_capacity(content.len())?;

    // ... rest of implementation
}
```

In `write_file()`:
```rust
pub fn write_file(&mut self, path: &str, content: &[u8]) -> Result<()> {
    // Ensure capacity before allocating
    self.ensure_capacity(content.len())?;

    // ... rest of implementation
}
```

### Update CartridgeFile::extend()

**Add to `src/io.rs`:**
```rust
impl CartridgeFile {
    /// Extend file to new block count
    pub fn extend(&mut self, new_total_blocks: usize) -> Result<()> {
        let new_size = new_total_blocks * PAGE_SIZE;
        self.file.set_len(new_size as u64)?;
        Ok(())
    }
}
```

### Update HybridAllocator::extend_capacity()

**Add to allocator:**
```rust
impl HybridAllocator {
    /// Extend allocator capacity
    pub fn extend_capacity(&mut self, new_total_blocks: usize) -> Result<()> {
        // Extend bitmap allocator
        self.bitmap.extend_capacity(new_total_blocks)?;

        // Extent allocator automatically handles new space
        Ok(())
    }
}
```

---

## PART 3: Update cartridge-rs High-Level API

### Simple API (No Capacity Management)

**Old:**
```rust
Cartridge::create("data.cart", 10_000)?;
```

**New:**
```rust
Cartridge::create("my-container", "My Container")?;
// Starts at 12KB, grows automatically
```

### Builder API (For Advanced Use)

```rust
CartridgeBuilder::new()
    .slug("my-container")
    .title("My Container")
    .description("My container description")
    .initial_blocks(10_000)   // Optional: start bigger
    .max_blocks(1_000_000)    // Optional: set limit
    .with_audit_logging()
    .build()?
```

**Implementation in `crates/cartridge-rs/src/lib.rs`:**

Update `Cartridge::create()`:
```rust
pub fn create<P: AsRef<Path>>(
    path: P,
    slug: &str,
    title: &str,
) -> Result<Self> {
    let inner = CoreCartridge::create(path, slug, title)?;
    Ok(Cartridge { inner })
}
```

Update `CartridgeBuilder`:
```rust
pub struct CartridgeBuilder {
    path: Option<String>,
    slug: Option<String>,
    title: Option<String>,
    description: Option<String>,
    initial_blocks: usize,     // Default: 3
    max_blocks: usize,         // Default: 10_000_000
    enable_audit: bool,
}

impl CartridgeBuilder {
    pub fn new() -> Self {
        CartridgeBuilder {
            path: None,
            slug: None,
            title: None,
            description: None,
            initial_blocks: 3,
            max_blocks: 10_000_000,
            enable_audit: false,
        }
    }

    pub fn slug(mut self, slug: impl Into<String>) -> Self {
        self.slug = Some(slug.into());
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn initial_blocks(mut self, blocks: usize) -> Self {
        self.initial_blocks = blocks;
        self
    }

    pub fn max_blocks(mut self, blocks: usize) -> Self {
        self.max_blocks = blocks;
        self
    }
}
```

---

## PART 4: Update Examples

### Basic Usage Example

**File: `crates/cartridge-core/examples/basic_usage.rs`**

```rust
// Old
let mut cart = Cartridge::create("example.cart", 10_000)?;

// New
let mut cart = Cartridge::create("example", "Example Container", "My Example")?;
```

### Library Dependency Example

**File: `crates/cartridge-core/examples/library_dependency.rs`**

Update all instances to use slug/title API.

---

## PART 5: Update Documentation

### Terminology Changes

**Global search and replace:**
- "archive" → "container" (when referring to instances)
- "Archive" → "Container" (in type names where appropriate)
- Keep "Cartridge" as the format name
- Reserve "archive" for immutable Engram archives

### Files to Update

1. `README.md`:
   - Update "mutable archive" → "mutable container"
   - Add slug vs title explanation
   - Update examples

2. `LIBRARY_USAGE.md`:
   - Show new API without block counts
   - Explain auto-growth
   - Show slug/title distinction

3. `crates/cartridge-rs/README.md`:
   - Update all examples
   - Explain capacity management

4. `docs/CARTRIDGE_CORE_README.md`:
   - Update architecture
   - Document auto-growth behavior

---

## Key Terminology

| Term | Usage | Example |
|------|-------|---------|
| **Cartridge** | Format name | "Cartridge format specification" |
| **Container** | Mutable instance | "a Cartridge container", "open the container" |
| **Slug** | Kebab-case identifier | "us-const" (filename, registry key) |
| **Title** | Human-readable name | "U.S. Constitution" (display name) |
| **Archive** | Reserved for Engram | "freeze to Engram archive" (immutable) |

---

## User-Facing API Changes

### Before (Current)
```rust
// User must specify capacity upfront
let mut cart = Cartridge::create("data.cart", 10_000)?;
cart.write_file("file.txt", b"data")?;  // Confusing: write_file vs create_file
```

### After (New)
```rust
// No capacity needed, auto-grows
let mut cart = Cartridge::create("my-container", "My Container")?;
cart.write("file.txt", b"data")?;  // Simple: always works

// Check how much it grew
println!("Size: {} blocks", cart.stats().total_blocks);
```

### Advanced Usage
```rust
// Power users can control growth
let mut cart = CartridgeBuilder::new()
    .slug("large-container")
    .title("Large Container")
    .initial_blocks(100_000)  // Start with ~400MB
    .max_blocks(10_000_000)   // Cap at ~40GB
    .build()?;
```

---

## Implementation Checklist

### Phase 1: Core Infrastructure ✅
- [x] Add dependencies (semver, validator)
- [x] Create validation.rs with ContainerSlug
- [x] Create manifest.rs with slug/title
- [x] Add error types
- [x] Expose modules in lib.rs

### Phase 2: Auto-Growth
- [ ] Add auto_grow, max_blocks fields to Cartridge
- [ ] Implement ensure_capacity() method
- [ ] Implement grow() method
- [ ] Add CartridgeFile::extend()
- [ ] Add HybridAllocator::extend_capacity()
- [ ] Update create_file() to call ensure_capacity()
- [ ] Update write_file() to call ensure_capacity()

### Phase 3: Slug/Title Integration
- [ ] Update Cartridge::create() signature (slug, title params)
- [ ] Add manifest creation in create()
- [ ] Update Cartridge::open() to load manifest
- [ ] Add read_manifest(), write_manifest() methods
- [ ] Add slug(), title() convenience methods

### Phase 4: High-Level API
- [ ] Update cartridge-rs Cartridge::create()
- [ ] Update CartridgeBuilder with slug/title
- [ ] Remove blocks() method from builder
- [ ] Add initial_blocks(), max_blocks() methods

### Phase 5: Examples & Docs
- [ ] Update basic_usage.rs
- [ ] Update library_dependency.rs
- [ ] Update README.md
- [ ] Update LIBRARY_USAGE.md
- [ ] Update crates/cartridge-rs/README.md

### Phase 6: Testing
- [ ] Test auto-growth behavior
- [ ] Test manifest creation/loading
- [ ] Test slug validation
- [ ] Test backwards compatibility
- [ ] Run full test suite

---

## Testing Plan

### Auto-Growth Tests

```rust
#[test]
fn test_auto_growth() -> Result<()> {
    let mut cart = Cartridge::create("test", "Test", "Test Container")?;

    // Starts at 3 blocks
    assert_eq!(cart.stats().total_blocks, 3);

    // Add content that exceeds initial capacity
    let large_data = vec![0u8; 100_000]; // ~100KB
    cart.write("large.bin", &large_data)?;

    // Should have grown
    assert!(cart.stats().total_blocks > 3);

    Ok(())
}
```

### Slug/Title Tests

```rust
#[test]
fn test_slug_title_distinction() -> Result<()> {
    let cart = Cartridge::create(
        "us-const",
        "us-const",              // slug
        "U.S. Constitution"      // title
    )?;

    let manifest = cart.read_manifest()?;
    assert_eq!(manifest.slug.as_str(), "us-const");
    assert_eq!(manifest.title, "U.S. Constitution");

    Ok(())
}
```

---

## Notes

- Users should NEVER specify `.cart` extension - core handles it
- Default auto-growth prevents capacity planning for 99% of use cases
- Advanced users can still control growth with builder pattern
- Backwards compatible: old containers without manifests still open with warnings

---

**End of Plan**
