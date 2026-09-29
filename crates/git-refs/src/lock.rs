//! C-exact dot-lock lifecycle for ref updates.
//!
//! A port of the `lockfile.c` naming and lifecycle used by
//! `refs/files-backend.c`: the lock lives at the literal ref path plus a
//! `.lock` suffix (never the old `.lock.<pid>` temp scheme), creation fails
//! when the lock already exists, content is written plus fsynced, commit is
//! an atomic rename, and rollback (or drop) unlinks the lock.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::RefError;

/// The `.lock` path for a ref (or any other file under the git dir).
pub fn lock_path_for(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".lock");
    PathBuf::from(s)
}

/// Reject lock targets that escape `root` (T-01-01: a symlinked `refs/heads`
/// directory pointing outside `.git` must not let a lock — or the rename
/// that follows — write outside the repository).
///
/// Both sides are canonicalized so symlink aliases (`/var` vs
/// `/private/var`, macOS `/tmp` links) compare equal.
pub fn ensure_within(root: &Path, path: &Path) -> Result<(), RefError> {
    let root_canon = root
        .canonicalize()
        .map_err(|e| RefError::Io(format!("cannot resolve git dir '{}': {e}", root.display())))?;
    let anchor = path.parent().unwrap_or(path);
    let anchor_canon = anchor.canonicalize().map_err(|e| {
        RefError::Io(format!("cannot resolve lock directory '{}': {e}", anchor.display()))
    })?;
    if !anchor_canon.starts_with(&root_canon) {
        return Err(RefError::Io(format!(
            "refusing to lock '{}': resolves outside the repository",
            path.display()
        )));
    }
    Ok(())
}

/// Detail text for a failed lock creation, in C's shape:
/// `Unable to create '<lock>': <os message>.` plus the stale-lock advisory
/// C prints when the lock already exists.
fn create_detail(lock: &Path, err: &std::io::Error) -> String {
    let mut d = format!("Unable to create '{}': {}", lock.display(), lock_os_message(err));
    if err.kind() == std::io::ErrorKind::AlreadyExists {
        d.push_str(
            ".\n\nAnother git process seems to be running in this repository, or the lock file may be stale",
        );
    }
    d
}

/// The OS message without a trailing period (C's `%s` from `strerror`
/// carries none; Rust's `ErrorKind` display sometimes implies one).
fn lock_os_message(err: &std::io::Error) -> String {
    match err.kind() {
        std::io::ErrorKind::AlreadyExists => "File exists".to_string(),
        _ => {
            let s = err.to_string();
            // `ErrorKind`-derived messages read like "File exists (os error 17)";
            // prefer the bare kind display for stability across platforms.
            match err.kind() {
                std::io::ErrorKind::NotFound => "No such file or directory".to_string(),
                std::io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
                _ => s,
            }
        }
    }
}

/// An acquired dot-lock: `<path>.lock` exists and is owned by us.
///
/// The file handle is never held between calls (C keeps only one
/// lockfile open at a time so large transactions cannot burst the open
/// file limit, `t/t1400` "does not burst open file limit"): the lock is
/// the `.lock` file's existence, writes reopen it briefly.
pub struct LockFile {
    path: PathBuf,
    lock: PathBuf,
    file: Option<std::fs::File>,
    committed: bool,
}

impl LockFile {
    /// Create `<path>.lock`, failing when it already exists (a concurrent
    /// second writer gets [`RefError::LockContention`] naming the lock).
    /// The handle is closed immediately; only the path is retained.
    pub fn acquire(path: &Path) -> Result<LockFile, RefError> {
        let lock = lock_path_for(path);
        match OpenOptions::new().write(true).create_new(true).open(&lock) {
            Ok(_) => Ok(LockFile { path: path.to_path_buf(), lock, file: None, committed: false }),
            Err(e) => Err(RefError::LockContention(create_detail(&lock, &e))),
        }
    }

    /// The path this lock will commit to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The lock file path (`<path>.lock`).
    pub fn lock_path(&self) -> &Path {
        &self.lock
    }

    /// Write content to the lock, fsyncing before it can be committed
    /// (C `write_ref_to_lockfile` crash-safety). Reopens the lock file
    /// briefly; no handle is retained.
    pub fn write_and_fsync(&mut self, content: &[u8]) -> Result<(), RefError> {
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.lock)
            .map_err(|e| RefError::Io(format!("cannot write lock '{}': {e}", self.lock.display())))?;
        file.write_all(content)
            .map_err(|e| RefError::Io(format!("cannot write lock '{}': {e}", self.lock.display())))?;
        file.sync_all()
            .map_err(|e| RefError::Io(format!("cannot fsync lock '{}': {e}", self.lock.display())))?;
        Ok(())
    }

    /// Atomically publish the lock content (fsync, then rename).
    /// Consumes the lock so [`Drop`] cannot unlink the published file.
    pub fn commit(mut self) -> Result<(), RefError> {
        {
            let file = OpenOptions::new()
                .read(true)
                .open(&self.lock)
                .map_err(|e| RefError::Io(format!("cannot fsync lock '{}': {e}", self.lock.display())))?;
            file.sync_all()
                .map_err(|e| RefError::Io(format!("cannot fsync lock '{}': {e}", self.lock.display())))?;
        }
        self.file = None;
        std::fs::rename(&self.lock, &self.path)
            .map_err(|e| RefError::Io(format!("cannot commit lock '{}': {e}", self.lock.display())))?;
        self.committed = true;
        Ok(())
    }

    /// Abandon the lock, unlinking `<path>.lock`.
    pub fn rollback(mut self) {
        self.file = None;
        let _ = std::fs::remove_file(&self.lock);
        self.committed = true; // Drop must not unlink twice; nothing to publish.
    }
}

impl Drop for LockFile {
    fn drop(&mut self) {
        if !self.committed {
            self.file = None;
            let _ = std::fs::remove_file(&self.lock);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn scratch() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("git-lock-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn acquire_write_commit_publishes() {
        let dir = scratch();
        let target = dir.join("refs/heads/main");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let mut lock = LockFile::acquire(&target).unwrap();
        assert_eq!(lock.lock_path(), &lock_path_for(&target));
        lock.write_and_fsync(b"abc\n").unwrap();
        assert!(lock.lock_path().exists());
        lock.commit().unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "abc\n");
        assert!(!lock_path_for(&target).exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn second_holder_gets_contention_naming_lock() {
        let dir = scratch();
        let target = dir.join("refs/heads/main");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let _first = LockFile::acquire(&target).unwrap();
        let err = match LockFile::acquire(&target) {
            Ok(_) => panic!("second acquire must fail"),
            Err(e) => e,
        };
        let text = err.to_string();
        assert!(text.contains(&lock_path_for(&target).to_string_lossy().into_owned()), "error names the lock file: {text}");
        assert!(matches!(err, RefError::LockContention(_)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rollback_and_drop_unlink_the_lock() {
        let dir = scratch();
        let target = dir.join("refs/heads/main");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let lock = LockFile::acquire(&target).unwrap();
        let lock_path = lock.lock_path().to_path_buf();
        lock.rollback();
        assert!(!lock_path.exists());
        assert!(!target.exists());
        // Drop without commit also cleans up.
        let lock2 = LockFile::acquire(&target).unwrap();
        let lock_path2 = lock2.lock_path().to_path_buf();
        drop(lock2);
        assert!(!lock_path2.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn escape_outside_root_is_rejected() {
        let dir = scratch();
        let root = dir.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let outside = dir.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        // A path whose parent canonicalizes outside `root` is refused even
        // though creation itself would succeed.
        let err = ensure_within(&root, &outside.join("x")).unwrap_err();
        assert!(err.to_string().contains("outside the repository"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
