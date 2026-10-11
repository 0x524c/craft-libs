#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

//! Crash-safe file replacement: the unified atomic save engine for the Craft suite.
//!
//! [`atomic_write`] never truncates the destination file in place. It writes a temporary file
//! in the **same directory** as the destination, flushes it to non-volatile storage (`sync_all`),
//! renames it over the destination atomically, and on Unix, flushes the directory so the rename
//! survives a power loss. If any step fails, the destination retains its original bytes and
//! the temporary staging file is removed.
//!
//! ### Platform Characteristics
//! - **Windows**: Antivirus scanners, search indexers, and backup utilities briefly hold files
//!   open with sharing violations. The rename is retried with exponential backoff (`RenameRetry`).
//! - **Cross-Device (`EXDEV`)**: Staging files are always created beside the destination in the
//!   same folder, guaranteeing same-filesystem replace semantics.
//! - **Apple Network Shares (macOS)**: `sync_all` issues `F_FULLFSYNC`. Where SMB shares or
//!   virtual filesystems reject this with `ENOTSUP` or `ENOTTY`, [`sync_file`] falls back to standard
//!   `fsync`.
//! - **Web / WASM**: On `wasm32`, returns standard unsupported `io::Error` rather than panicking.
//! - **Failure Injection**: [`atomic_write_custom`] accepts an [`AtomicFs`] implementation so
//!   callers and test suites can inject disk-full, lock-contention, or permission errors at each stage.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// File-system operations performed by atomic writes.
/// [`RealFs`] provides the production implementation; test suites implement this to inject faults.
pub trait AtomicFs {
    /// Create `tmp` (which must not exist), write all `bytes`, and flush to disk.
    fn write_new(&self, tmp: &Path, bytes: &[u8]) -> io::Result<()>;
    /// Rename `from` over `to`, replacing it atomically.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// Remove a file.
    fn remove(&self, path: &Path) -> io::Result<()>;
    /// Flush a directory entry change to durable storage (Unix); no-op elsewhere.
    fn sync_dir(&self, dir: &Path) -> io::Result<()>;
    /// Copy the destination's permissions onto the temporary file before replacement.
    fn copy_permissions(&self, from: &Path, to: &Path) -> io::Result<()>;
}

/// The production file system implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealFs;

impl AtomicFs for RealFs {
    fn write_new(&self, tmp: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut f = OpenOptions::new().write(true).create_new(true).open(tmp)?;
        f.write_all(bytes)?;
        sync_file(&f)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        #[cfg(unix)]
        {
            sync_file(&File::open(dir)?)
        }
        #[cfg(not(unix))]
        {
            let _ = dir;
            Ok(())
        }
    }

    fn copy_permissions(&self, from: &Path, to: &Path) -> io::Result<()> {
        let perms = std::fs::metadata(from)?.permissions();
        std::fs::set_permissions(to, perms)
    }
}

/// Flush `file` to stable storage (`sync_all`).
/// On Apple platforms, this issues an `F_FULLFSYNC`. Where the file system rejects full sync
/// (such as SMB network shares or virtual file systems), it falls back to a plain `fsync`.
pub fn sync_file(file: &File) -> io::Result<()> {
    #[cfg(target_vendor = "apple")]
    {
        or_plain_fsync(file.sync_all(), || rustix::fs::fsync(file).map_err(io::Error::from))
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        file.sync_all()
    }
}

/// Returns `full`, or calls `fsync()` if `full` failed only because `F_FULLFSYNC` is unsupported.
pub fn or_plain_fsync(full: io::Result<()>, fsync: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
    match full {
        Err(e) if full_flush_unsupported(&e) => fsync(),
        other => other,
    }
}

fn full_flush_unsupported(e: &io::Error) -> bool {
    const ENOTTY: i32 = 25;
    const ENOTSUP: i32 = 45;
    cfg!(target_vendor = "apple") && matches!(e.raw_os_error(), Some(ENOTSUP | ENOTTY))
}

/// Retry configuration for renames that encounter transient file locks (`PermissionDenied`).
#[derive(Debug, Clone, Copy)]
pub struct RenameRetry {
    /// Number of retries after the initial attempt (0 = no retry).
    pub attempts: u32,
    /// Delay before the first retry in milliseconds; doubles on each subsequent attempt.
    pub first_delay_ms: u64,
}

impl RenameRetry {
    /// Platform default: retry on Windows (7 attempts with 10ms exponential backoff), no-op elsewhere.
    pub fn platform() -> Self {
        if cfg!(windows) { Self { attempts: 7, first_delay_ms: 10 } } else { Self { attempts: 0, first_delay_ms: 0 } }
    }
}

/// Atomically writes `bytes` to `path`, replacing any existing content.
///
/// If write or rename fails, the target file is untouched and any temporary staging file is removed.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    atomic_write_custom(&RealFs, RenameRetry::platform(), path, bytes)
}

/// Atomically writes to `path` using a streaming writer callback `F`.
pub fn atomic_write_with<F>(path: &Path, write_fn: F) -> io::Result<()>
where
    F: FnOnce(&mut File) -> io::Result<()>,
{
    let target = resolve_symlink(path);
    let name = target.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("{}: not a file path", path.display())))?;
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    std::fs::create_dir_all(&dir).map_err(|e| context(e, format!("cannot create the folder {}", dir.display())))?;

    let mut tmp = None;
    for _ in 0..8 {
        let candidate = dir.join(temp_name(&name.to_string_lossy()));
        match OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(mut f) => {
                let res = write_fn(&mut f).and_then(|()| sync_file(&f));
                drop(f);
                if let Err(e) = res {
                    let _ = std::fs::remove_file(&candidate);
                    return Err(write_error(e, path, &dir));
                }
                tmp = Some(candidate);
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(write_error(e, path, &dir)),
        }
    }

    let tmp = tmp.ok_or_else(|| io::Error::new(io::ErrorKind::AlreadyExists, format!("{}: could not create a temporary file", dir.display())))?;

    if target.exists() {
        let _ = RealFs.copy_permissions(&target, &tmp);
    }

    if let Err(e) = retry_rename(RenameRetry::platform(), || std::fs::rename(&tmp, &target)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(rename_error(e, path));
    }

    let _ = RealFs.sync_dir(&dir);
    Ok(())
}

/// Atomically writes `bytes` to `path` through custom [`AtomicFs`] and [`RenameRetry`].
pub fn atomic_write_custom(fs: &dyn AtomicFs, retry: RenameRetry, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = resolve_symlink(path);
    let name = target.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("{}: not a file path", path.display())))?;
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    std::fs::create_dir_all(&dir).map_err(|e| context(e, format!("cannot create the folder {}", dir.display())))?;

    let mut tmp = None;
    for _ in 0..8 {
        let candidate = dir.join(temp_name(&name.to_string_lossy()));
        match fs.write_new(&candidate, bytes) {
            Ok(()) => {
                tmp = Some(candidate);
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                let _ = fs.remove(&candidate);
                return Err(write_error(e, path, &dir));
            }
        }
    }
    let tmp = tmp.ok_or_else(|| io::Error::new(io::ErrorKind::AlreadyExists, format!("{}: could not create a temporary file", dir.display())))?;

    if target.exists() {
        let _ = fs.copy_permissions(&target, &tmp);
    }

    if let Err(e) = retry_rename(retry, || fs.rename(&tmp, &target)) {
        let _ = fs.remove(&tmp);
        return Err(rename_error(e, path));
    }

    let _ = fs.sync_dir(&dir);
    Ok(())
}

/// Exclusively creates and writes `path` without replacing existing files.
pub fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_new_with(path, |f| f.write_all(bytes))
}

/// Exclusively creates and writes `path` with a streaming writer callback `F`.
pub fn write_new_with<F>(path: &Path, write_fn: F) -> io::Result<()>
where
    F: FnOnce(&mut File) -> io::Result<()>,
{
    let target = resolve_symlink(path);
    if target.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{}: file already exists", path.display())));
    }

    let name = target.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("{}: not a file path", path.display())))?;
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    std::fs::create_dir_all(&dir).map_err(|e| context(e, format!("cannot create the folder {}", dir.display())))?;

    let tmp = dir.join(temp_name(&name.to_string_lossy()));
    let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    let filled = write_fn(&mut f).and_then(|()| sync_file(&f));
    drop(f);

    if let Err(e) = filled {
        let _ = std::fs::remove_file(&tmp);
        return Err(write_error(e, path, &dir));
    }

    #[cfg(unix)]
    let placed = std::fs::hard_link(&tmp, &target).or_else(|e| {
        if e.kind() == io::ErrorKind::AlreadyExists {
            Err(e)
        } else {
            // Hard link unsupported; fall back to atomic rename if target still missing
            std::fs::rename(&tmp, &target)
        }
    });
    #[cfg(not(unix))]
    let placed = std::fs::rename(&tmp, &target);

    let _ = std::fs::remove_file(&tmp);
    placed
}

/// Retries an operation with exponential backoff while it fails with `PermissionDenied`.
pub fn retry_rename(retry: RenameRetry, mut rename: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    let mut delay = retry.first_delay_ms;
    let mut left = retry.attempts;
    loop {
        match rename() {
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && left > 0 => {
                left -= 1;
                sleep_ms(delay);
                delay = delay.saturating_mul(2);
            }
            r => return r,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn sleep_ms(ms: u64) {
    if ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}
#[cfg(target_arch = "wasm32")]
fn sleep_ms(_ms: u64) {}

fn resolve_symlink(path: &Path) -> PathBuf {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
        _ => path.to_path_buf(),
    }
}

/// Generates a unique, hidden temporary file name beside `file`.
pub fn temp_name(file: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let stem: String = file.chars().take(64).collect();
    format!(".{stem}.{}-{n}.craft_tmp", std::process::id())
}

fn context(e: io::Error, msg: String) -> io::Error {
    io::Error::new(e.kind(), format!("{msg}: {e}"))
}

const UNCHANGED: &str = "the original file was not changed";

fn write_error(e: io::Error, path: &Path, dir: &Path) -> io::Error {
    let msg = match e.kind() {
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => {
            format!("cannot save {}: the folder {} is not writable ({e}); {UNCHANGED}", path.display(), dir.display())
        }
        io::ErrorKind::StorageFull => format!("cannot save {}: the disk is full ({e}); {UNCHANGED}", path.display()),
        _ => format!("cannot save {}: {e}; {UNCHANGED}", path.display()),
    };
    io::Error::new(e.kind(), msg)
}

fn rename_error(e: io::Error, path: &Path) -> io::Error {
    let msg = if is_cross_device(&e) {
        format!("cannot save {}: the temporary file is on a different volume than the destination ({e}); {UNCHANGED}", path.display())
    } else if e.kind() == io::ErrorKind::PermissionDenied {
        format!("cannot save {}: the file is locked or read-only ({e}); {UNCHANGED}", path.display())
    } else {
        format!("cannot save {}: {e}; {UNCHANGED}", path.display())
    };
    io::Error::new(e.kind(), msg)
}

fn is_cross_device(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::CrossesDevices || (cfg!(unix) && e.raw_os_error() == Some(18)) || (cfg!(windows) && e.raw_os_error() == Some(17))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let d = std::env::temp_dir().join(format!("craft-atomic-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).ok();
            Self(d)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
            }
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn assert_no_temp(dir: &Path) {
        let leftovers: Vec<_> = std::fs::read_dir(dir)
            .ok()
            .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".craft_tmp")).collect())
            .unwrap_or_default();
        assert!(leftovers.is_empty(), "temporary files left behind: {leftovers:?}");
    }

    const ORIGINAL: &[u8] = b"the user's precious original document";

    #[derive(Default)]
    struct Faulty {
        fail_write_after: Option<usize>,
        fail_rename: Option<io::ErrorKind>,
        locked_renames: Cell<u32>,
        renames: Cell<u32>,
    }

    impl AtomicFs for Faulty {
        fn write_new(&self, tmp: &Path, bytes: &[u8]) -> io::Result<()> {
            if let Some(n) = self.fail_write_after {
                let mut f = OpenOptions::new().write(true).create_new(true).open(tmp)?;
                f.write_all(&bytes[..n.min(bytes.len())])?;
                return Err(io::Error::new(io::ErrorKind::StorageFull, "injected: no space left on device"));
            }
            RealFs.write_new(tmp, bytes)
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.renames.set(self.renames.get() + 1);
            if let Some(kind) = self.fail_rename {
                return Err(io::Error::new(kind, "injected rename failure"));
            }
            if self.locked_renames.get() > 0 {
                self.locked_renames.set(self.locked_renames.get() - 1);
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "injected: sharing violation"));
            }
            RealFs.rename(from, to)
        }
        fn remove(&self, path: &Path) -> io::Result<()> {
            RealFs.remove(path)
        }
        fn sync_dir(&self, dir: &Path) -> io::Result<()> {
            RealFs.sync_dir(dir)
        }
        fn copy_permissions(&self, from: &Path, to: &Path) -> io::Result<()> {
            RealFs.copy_permissions(from, to)
        }
    }

    const NO_RETRY: RenameRetry = RenameRetry { attempts: 0, first_delay_ms: 0 };

    #[test]
    fn replaces_and_creates() {
        let d = TempDir::new("ok");
        let p = d.0.join("doc.craft");
        atomic_write(&p, b"first").ok();
        assert_eq!(std::fs::read(&p).ok(), Some(b"first".to_vec()));
        atomic_write(&p, b"second, longer").ok();
        assert_eq!(std::fs::read(&p).ok(), Some(b"second, longer".to_vec()));
        assert_no_temp(&d.0);
    }

    #[test]
    fn creates_missing_parent_folders() {
        let d = TempDir::new("parent");
        let p = d.0.join("a/b/out.png");
        atomic_write(&p, b"x").ok();
        assert_eq!(std::fs::read(&p).ok(), Some(b"x".to_vec()));
    }

    #[test]
    fn write_failure_halfway_keeps_original() {
        let d = TempDir::new("write");
        let p = d.0.join("doc.craft");
        std::fs::write(&p, ORIGINAL).ok();
        let fs = Faulty { fail_write_after: Some(5), ..Default::default() };
        let e = atomic_write_custom(&fs, NO_RETRY, &p, &[7u8; 4096]);
        assert!(e.is_err());
        assert_eq!(std::fs::read(&p).ok(), Some(ORIGINAL.to_vec()));
        assert_eq!(fs.renames.get(), 0);
        assert_no_temp(&d.0);
    }

    #[test]
    fn rename_failure_keeps_original() {
        let d = TempDir::new("rename");
        let p = d.0.join("doc.craft");
        std::fs::write(&p, ORIGINAL).ok();
        let fs = Faulty { fail_rename: Some(io::ErrorKind::Other), ..Default::default() };
        assert!(atomic_write_custom(&fs, NO_RETRY, &p, b"new").is_err());
        assert_eq!(std::fs::read(&p).ok(), Some(ORIGINAL.to_vec()));
        assert_no_temp(&d.0);
    }

    #[test]
    fn locked_target_rename_is_retried() {
        let d = TempDir::new("locked");
        let p = d.0.join("doc.craft");
        std::fs::write(&p, ORIGINAL).ok();
        let fs = Faulty { locked_renames: Cell::new(2), ..Default::default() };
        atomic_write_custom(&fs, RenameRetry { attempts: 3, first_delay_ms: 1 }, &p, b"new").ok();
        assert_eq!(std::fs::read(&p).ok(), Some(b"new".to_vec()));
        assert_eq!(fs.renames.get(), 3);
        assert_no_temp(&d.0);
    }

    #[test]
    fn write_new_never_overwrites_existing() {
        let d = TempDir::new("new");
        let p = d.0.join("file.craft");
        write_new(&p, b"first").ok();
        assert_eq!(std::fs::read(&p).ok(), Some(b"first".to_vec()));
        let res = write_new(&p, b"second");
        assert!(res.is_err());
        assert_eq!(std::fs::read(&p).ok(), Some(b"first".to_vec()));
        assert_no_temp(&d.0);
    }
}
