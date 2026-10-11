# Craft Atomic

`craft-atomic` provides crash-safe atomic file writing and replacement for the Craft suite.

Extracted from the storage engine in **PhotoCraft** (refs #2, #14), it guarantees that a failed save or abnormal termination never leaves user documents truncated, corrupted, or missing.

## Core Guarantees: Atomicity vs. Durability
 
1. **Atomicity (All-or-Nothing Replacement)**: `atomic_write` never truncates or overwrites the destination in place. It stages content in a hidden temporary file beside the destination (`.{stem}.{pid}-{n}.craft_tmp`), guaranteeing same-volume rename semantics (`EXDEV`-free). An external reader never sees a partial write. If writing fails, times out, or the process crashes mid-stream, the destination retains its prior bytes and the staging file is unlinked.
2. **Durability (Power-Loss & Crash Persistence)**:
   - **Content Flush**: Before rename, staging file bytes are flushed to physical storage media (`sync_file`, issuing `F_FULLFSYNC` on macOS/APFS with SMB `fsync` fallback).
   - **Directory Flush**: After rename, on Unix platforms, the parent directory table is explicitly synced (`sync_dir`) so the new directory entry survives sudden power loss.
3. **Transient Lock Backoff (Windows)**: Antivirus scanners and search indexers hold newly written files momentarily. `RenameRetry` automatically retries renames encountering `PermissionDenied`, sharing violations (32), or lock violations (33) with exponential backoff capped at 200ms.
4. **Permission & Symlink Invariants**: On Unix, preserves destination permissions (e.g. `0600`). Symlink chains are resolved with a hop limit, writing through the symlink to the ultimate target without replacing the link itself.
5. **Safe Exclusive Creation (`write_new`)**: Uses hard links where supported, or exclusive create (`O_EXCL`), guaranteeing that an existing file is never replaced under race conditions.
6. **Fault-Injection Testability**: The `AtomicFs` trait enables unit and integration test suites to simulate disk-full, locked files, and permission failures without physical disk I/O.

A replaced file is a new file (a new inode), so its owner and group, extended attributes and ACLs, Finder tags, alternate data streams and hard links are not kept; a crash between the write and the rename can leave a `.<name>.<pid>-<n>.craft_tmp` file beside the destination.

## Usage

### One-Shot Atomic Write

```rust
use std::path::Path;
use craft_atomic::atomic_write;

let path = Path::new("project.craft");
let data = b"precious document state";

// Atomically replaces project.craft. If writing fails, project.craft retains
// its prior bytes and all temporary staging files are unlinked.
atomic_write(path, data).expect("safe atomic write");
```

### Streaming Atomic Writer

```rust
use std::io::Write;
use std::path::Path;
use craft_atomic::atomic_write_with;

let path = Path::new("large_project.craft");

atomic_write_with(path, |file| {
    file.write_all(b"header...")?;
    file.write_all(b"payload...")?;
    Ok(())
}).expect("streaming atomic write");
```

### Exclusive Creation (`write_new`)

```rust
use std::path::Path;
use craft_atomic::write_new;

let path = Path::new("export.png");

// Writes only if the destination does not already exist; fails with AlreadyExists
// without modifying existing content.
write_new(path, b"png bytes...").expect("exclusive write");
```

### Fault Injection & Testing

```rust
use craft_atomic::{AtomicFs, RenameRetry, atomic_write_custom};

// Inject simulated failures (e.g. storage full mid-stream)
struct FaultyFs;
impl AtomicFs for FaultyFs {
    // ... custom failure simulation ...
}
```

## Guarantees & Doctrines

- **Zero App Code**: Zero dependencies on application crates.
- **Never Crash**: `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.
- **No Unsafe**: the workspace lints forbid `unsafe` code.
