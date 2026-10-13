# Craft Atomic

`craft-atomic` provides crash-safe atomic file writing and replacement for the Craft suite.

Extracted from the storage engine in **PhotoCraft** (refs #2, #14), it guarantees that a failed save or abnormal termination never leaves user documents truncated, corrupted, or missing.

## Core Guarantees: Atomicity vs. Durability
 
1. **Atomicity (All-or-Nothing Replacement)**: `atomic_write` never truncates or overwrites the destination in place. It stages content in a hidden temporary file beside the destination (`.{stem}.{pid}-{n}.craft_tmp`), guaranteeing same-volume rename semantics (`EXDEV`-free). An external reader never sees a partial write. If an error occurs during staging or rename, the destination retains its prior bytes and the staging file is unlinked in user space. An abnormal process exit (e.g. `SIGKILL`, crash, or power loss) bypasses user-space cleanup and leaves the staging file on disk; sweeping orphaned staging files is delegated to consuming application recovery routines. The destination file itself is never corrupted.
2. **Durability & Ordering Boundaries**:
   - **Content Flush**: Before rename, staging file bytes are flushed to physical storage media (`sync_file`, issuing `F_FULLFSYNC` on macOS/APFS with SMB `fsync` fallback).
   - **Directory Flush (Unix)**: After rename, syncing the parent directory table (`sync_dir`) is attempted on a best-effort basis. If directory sync fails, the error is ignored because the rename has already committed and the destination has already been replaced.
   - **Hardware Durability Limits**: The sequence `sync_file < rename < sync_dir` proves ordering and kernel dispatch. Software tests do not prove survival across physical power interruption or unbuffered drive controller write caches.
3. **Transient Lock Backoff (Windows)**: Antivirus scanners and search indexers hold newly written files momentarily. `RenameRetry` automatically retries renames encountering `PermissionDenied`, sharing violations (32), or lock violations (33) with exponential backoff capped at 200ms.
4. **Permission & Symlink Invariants**: On Unix, staging files are created with mode `0600` (restricted by the process umask), so the payload starts private even in a shared folder. Existing destination permissions are restored after the streaming callback. New files remain private by default; a callback may select their final permissions, which are kept both with hard links and the exclusive-copy fallback. Symlink chains are resolved with a hop limit, writing through the symlink to the ultimate target without replacing the link itself.
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
// its prior bytes and temporary staging files are unlinked in user space.
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

### Normal Permissions for Documents and Exports

The free functions and `WriteOptions::default()` keep new files private. Settings and recovery
backups can continue using them. For a new user document or export that should follow normal
file-creation permissions, opt in explicitly:

```rust
use std::path::Path;
use craft_atomic::{NewFilePermissions, WriteOptions};

let documents = WriteOptions { new_file_permissions: NewFilePermissions::PlatformDefault };
documents.atomic_write(Path::new("project.craft"), b"document bytes")?;
documents.write_new(Path::new("export.pdf"), b"pdf bytes")?;
# Ok::<(), std::io::Error>(())
```

`atomic_write_with` and `write_new_with` on these options also support streaming. On Unix,
the library samples normal creation with mode `0666` using a separate, empty file beside the
destination. The OS applies the process umask or the directory's default ACL; process-global
umask is never changed. The empty probe is removed before a distinct `0600` payload stage is
created. Even an open handle to the probe never receives document bytes. This follows the
[Qt QSaveFile approach](https://github.com/qt/qtbase/blob/72b2a0c2f59014a34c2c433fc1fb1d18cff7ec48/src/corelib/io/qsavefile.cpp).

After writing finishes, the selected final permissions are applied before the file flush and
publication. On replacement, the existing Unix mode always takes precedence. On new files,
`PlatformDefault` takes precedence over callback-selected modes; the private default keeps
the original callback behavior. The exclusive-copy fallback still starts private and applies
final permissions after copying. Other platforms keep their existing creation behavior.

Sampling adds one empty-file create/stat/remove per new file with this policy, independent of
payload size. A sampling, probe-cleanup or permission-setting failure aborts the save; the library
does not guess permissions. As with ordinary file creation, this samples the directory policy
at creation time; later umask or directory-policy changes are not tracked. Matching a sampled
Unix mode does not promise preservation of all ACLs or other metadata across replacements.

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
