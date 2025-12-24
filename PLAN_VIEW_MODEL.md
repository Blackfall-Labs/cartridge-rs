# Plan: Add Entry View Model to Cartridge API

**Created**: 2025-11-23
**Status**: Proposed
**Goal**: Provide rich Entry-level API while keeping flat storage model

---

## Current State

### Storage Model (Core)
Cartridge uses a **flat keyspace** like S3/GCS/MinIO:
- Keys: `"research/notes/overview.cml"`, `"logs/session-1.json"`
- No native directory inodes
- Simple, efficient key-value storage

### API Model (Current)
```rust
pub fn list(&self, prefix: &str) -> Result<Vec<String>>
```

Returns flat paths:
```rust
cart.list("research") → [
    "research/notes/overview.cml",
    "research/notes/todo.cml",
    "research/sources/book1.txt",
]
```

**This is correct!** Object stores work this way.

### The Problem
Every consumer (research-engine, CLI tools, future GUI apps) has to:
1. Parse paths to extract names and parents
2. Group by prefix to build hierarchy
3. Infer `is_dir` by checking for children
4. **Duplicate this logic across every app**

---

## Proposed Solution: Entry View Model

### Add Entry Struct
```rust
/// Rich metadata about a file or directory in the archive
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Full path in the archive (e.g., "research/notes/overview.cml")
    pub path: String,

    /// Just the name (e.g., "overview.cml")
    pub name: String,

    /// Parent directory path (e.g., "research/notes")
    /// Empty string for root-level entries
    pub parent: String,

    /// True if this is a directory (has children under this prefix)
    pub is_dir: bool,

    // Future additions:
    // pub size: Option<u64>,
    // pub modified: Option<SystemTime>,
    // pub content_type: Option<String>,
}
```

### New API Methods
```rust
impl Cartridge {
    // Existing (keep as-is)
    pub fn list(&self, prefix: &str) -> Result<Vec<String>> { ... }

    // NEW: Rich entry metadata
    pub fn list_entries(&self, prefix: &str) -> Result<Vec<Entry>> {
        let paths = self.list(prefix)?;
        Ok(paths_to_entries(&paths, prefix))
    }

    // NEW: Directory-style listing (immediate children only)
    pub fn list_children(&self, parent: &str) -> Result<Vec<Entry>> {
        let all_entries = self.list_entries(parent)?;

        // Filter to immediate children (no deeper nesting)
        Ok(all_entries
            .into_iter()
            .filter(|e| e.parent == parent)
            .collect())
    }

    // NEW: Check if a path is a directory
    pub fn is_dir(&self, path: &str) -> Result<bool> {
        let prefix = format!("{}/", path);
        let paths = self.list(&prefix)?;
        Ok(!paths.is_empty())
    }
}
```

### Helper Implementation
```rust
/// Convert flat paths to Entry objects
fn paths_to_entries(paths: &[String], prefix: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut seen_dirs = std::collections::HashSet::new();

    for path in paths {
        // Extract name and parent
        let name = path.rsplit('/').next().unwrap_or(path);
        let parent = if let Some(idx) = path.rfind('/') {
            &path[..idx]
        } else {
            ""
        };

        // Add the file/document entry
        entries.push(Entry {
            path: path.clone(),
            name: name.to_string(),
            parent: parent.to_string(),
            is_dir: false,
        });

        // Add parent directories (if not already seen)
        let mut current_parent = parent;
        while !current_parent.is_empty() {
            if seen_dirs.insert(current_parent.to_string()) {
                let parent_name = current_parent.rsplit('/').next().unwrap_or(current_parent);
                let grandparent = if let Some(idx) = current_parent.rfind('/') {
                    &current_parent[..idx]
                } else {
                    ""
                };

                entries.push(Entry {
                    path: current_parent.to_string(),
                    name: parent_name.to_string(),
                    parent: grandparent.to_string(),
                    is_dir: true,
                });
            }

            // Move up the tree
            if let Some(idx) = current_parent.rfind('/') {
                current_parent = &current_parent[..idx];
            } else {
                break;
            }
        }
    }

    // Sort: directories first, then alphabetically
    entries.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        }
    });

    entries
}
```

---

## Benefits

### For Library Consumers
✅ **No duplication** - Tree-building logic lives in one place
✅ **Rich metadata** - Get name, parent, is_dir without parsing
✅ **Multiple views** - Use `list_entries()` or `list_children()` as needed
✅ **Type safety** - `Entry` struct instead of parsing strings
✅ **Future-proof** - Can add size, modified, etc. without breaking API

### For Cartridge Core
✅ **No storage changes** - Still flat keyspace internally
✅ **Backward compatible** - `list()` stays the same
✅ **Simple implementation** - Just path parsing, no inode tables
✅ **Single source of truth** - One `list()` call powers all views

### Example Usage

**Before (every app does this):**
```rust
// In research-engine, CLI, GUI app, etc.
let paths = cart.list("research")?;
let mut entries = Vec::new();

for path in paths {
    let name = path.rsplit('/').next().unwrap();
    let parent = path.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
    let is_dir = paths.iter().any(|p| p.starts_with(&format!("{}/", path)));

    entries.push(MyEntry { path, name, parent, is_dir });
}
```

**After (library provides it):**
```rust
// In any app
let entries = cart.list_entries("research")?;

for entry in entries {
    if entry.is_dir {
        println!("📁 {}", entry.name);
    } else {
        println!("📄 {}", entry.name);
    }
}
```

---

## Implementation Phases

### Phase 1: Add Entry Struct (1-2 hours)
- Define `Entry` struct in `lib.rs`
- Add basic tests
- Document fields

### Phase 2: Add list_entries() (2-3 hours)
- Implement `paths_to_entries()` helper
- Add `list_entries()` method
- Write comprehensive tests:
  - Flat structure (no nesting)
  - Nested directories (3+ levels)
  - Root-level files
  - Empty results

### Phase 3: Add list_children() (1 hour)
- Filter to immediate children only
- Add tests for directory-style listing

### Phase 4: Add is_dir() Helper (30 minutes)
- Quick check if path is a directory
- Useful for validation and UI logic

### Phase 5: Update Documentation (1 hour)
- Add examples to README
- Document Entry struct
- Show migration path for existing code

**Total Estimated Time**: 6-8 hours

---

## Migration Path for Consumers

### research-engine
**Current**:
```rust
// In cartridge-bridge/src/lib.rs
fn build_tree(cart: &Cartridge, prefix: &str) -> Result<Vec<FileNode>> {
    let all_entries = cart.list(prefix)?;
    // 50+ lines of path parsing and grouping...
}
```

**After**:
```rust
fn build_tree(cart: &Cartridge, prefix: &str) -> Result<Vec<FileNode>> {
    let entries = cart.list_entries(prefix)?;

    // Group by parent to build hierarchy
    let mut groups: HashMap<String, Vec<Entry>> = HashMap::new();
    for entry in entries {
        groups.entry(entry.parent.clone()).or_default().push(entry);
    }

    // Convert to FileNode tree
    build_nodes(&groups, prefix)
}
```

**Reduces 50 lines to 10**, removes all path parsing logic.

### CLI Tools
```rust
// cartridge-cli list --tree
fn list_tree(cart: &Cartridge, path: &str) {
    let entries = cart.list_children(path)?;

    for entry in entries {
        let icon = if entry.is_dir { "📁" } else { "📄" };
        println!("{} {}", icon, entry.name);

        if entry.is_dir {
            list_tree(cart, &entry.path)?; // Recurse
        }
    }
}
```

---

## Non-Goals (Out of Scope)

❌ **Change storage format** - Keep flat keyspace
❌ **Add directory inodes** - Still no native directories
❌ **Breaking changes** - Keep existing `list()` method
❌ **Permissions/ACLs** - Future feature
❌ **Watch/inotify** - Future feature

---

## Success Criteria

### API Completeness
- [x] `Entry` struct with path, name, parent, is_dir
- [x] `list_entries()` returns all entries under prefix
- [x] `list_children()` returns immediate children only
- [x] `is_dir()` checks if path is a directory

### Code Quality
- [x] 100% test coverage for new methods
- [x] Documentation with examples
- [x] No performance regressions on `list()`
- [x] Backward compatible (no breaking changes)

### Consumer Benefits
- [x] research-engine can remove 50+ lines of tree building
- [x] CLI tools can use rich Entry metadata
- [x] Future GUI apps get directory view for free

---

## ✅ Future Enhancements (IMPLEMENTED v0.2.0)

### ✅ Metadata Expansion (COMPLETE)
```rust
pub struct Entry {
    // ... existing fields
    pub size: Option<u64>,           // ✅ File size in bytes
    pub created: Option<u64>,        // ✅ Creation timestamp
    pub modified: Option<u64>,       // ✅ Last modified timestamp
    pub content_type: Option<String>, // ✅ MIME type
    pub file_type: FileType,         // ✅ File/Directory/Symlink
    pub compressed_size: Option<u64>, // ✅ Size on disk (NEW in v0.2.0)
}
```

### ✅ Virtual Filesystem Trait (COMPLETE)
```rust
pub trait Vfs {
    fn list_entries(&self, prefix: &str) -> Result<Vec<Entry>>;
    fn list_children(&self, parent: &str) -> Result<Vec<Entry>>;
    fn read(&self, path: &str) -> Result<Vec<u8>>;
    fn write(&mut self, path: &str, data: &[u8]) -> Result<()>;
    fn delete(&mut self, path: &str) -> Result<()>;
    fn exists(&self, path: &str) -> Result<bool>;
    fn is_dir(&self, path: &str) -> Result<bool>;
    fn metadata(&self, path: &str) -> Result<FileMetadata>;
}

impl Vfs for Cartridge { ... }
```

**Now enables:**
- ✅ Mock VFS for testing
- ✅ Generic code that works with any backend
- ✅ Future backends (ZipVfs, TarVfs, S3Vfs, EngramVfs)
- ✅ Unified interface across storage types

**See examples:**
- `cargo run --example vfs_trait` - Generic VFS code
- `cargo run --example compression_analysis` - compressed_size usage

---

## Recommendation

**Start with Phase 1-3** (4-6 hours):
1. Add `Entry` struct
2. Implement `list_entries()`
3. Add `list_children()`

Then **update research-engine** to use the new API (saves 50+ lines of code).

This provides immediate value without redesigning the storage layer. Future enhancements can be added incrementally based on real usage patterns.

---

**Ready to implement?** This is a high-value, low-risk improvement that benefits all Cartridge consumers.
