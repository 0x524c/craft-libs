# Craft Atomic

`craft-atomic` provides crash-safe atomic file writing and replacement for the Craft suite.

Extracted from the storage engine in **PhotoCraft** (refs #2, #14), it guarantees that a failed save or abnormal termination never leaves user documents truncated, corrupted, or missing.

## Core Guarantees

1. **Non-Destructive Replacement**: `atomic_write` never truncates the destination file in place. It writes a temporary file in the **same directory** as the destination, flushes it to non-volatile storage, renames it over the destination atomically, and flushes the directory so the rename survives power failure.
2. **Guaranteed Same-Directory Staging**: Staging files are allocated beside the destination (`.{stem}.{pid}-{n}.craft_tmp`), guaranteeing same-filesystem volume rename semantics and eliminating `EXDEV` / `ERROR_NOT_SAME_DEVICE`.
3. **Transient Lock Backoff (Windows)**: Antivirus scanners and search indexers hold newly written files momentarily. `RenameRetry` automatically retries renames encountering `PermissionDenied`, sharing violations or lock violations with exponential backoff.
4. **Apple SMB / Network Share Fallback**: On macOS, `sync_all` issues `F_FULLFSYNC`. Where network shares or virtual file systems reject full flush with `ENOTSUP` (45) or `ENOTTY` (25), it falls back to standard `fsync`.
5. **Permission Preservation**: On Unix, replaces files while preserving the original file's mode and permissions (e.g. `0600`).
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
