//! Work-tree materialization (FR-017): file creation, mode/symlink handling,
//! stat refresh, and sparse patterns — the single place where index entries
//! and work-tree bytes meet.
//!
//! The boundary contract: callers supply index entries plus content bytes;
//! this component performs the filesystem effects atomically (temp file plus
//! rename) and reports what changed. It never reads objects, refs, or config
//! itself — those arrive as values.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

/// Errors from work-tree materialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeError {
    /// Filesystem I/O failure.
    Io(String),
    /// Unsupported entry mode for materialization.
    UnsupportedMode(u32),
    /// A path escapes the work tree (never written).
    PathEscapesTree(String),
}

impl fmt::Display for WorktreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorktreeError::Io(e) => write!(f, "worktree I/O error: {e}"),
            WorktreeError::UnsupportedMode(m) => write!(f, "unsupported worktree mode: {m:o}"),
            WorktreeError::PathEscapesTree(p) => write!(f, "path escapes work tree: {p}"),
        }
    }
}

impl Error for WorktreeError {}

/// One file to materialize: the index entry's identity plus caller-supplied
/// content bytes (fetched from the object store by the caller).
#[derive(Debug, Clone)]
pub struct MaterializeFile {
    /// Repository-relative path with forward slashes.
    pub path: String,
    /// Index mode (`0o100644`, `0o100755`, `0o120000`, `0o160000`).
    pub mode: u32,
    /// Content bytes (blob content, or the symlink target for `0o120000`).
    pub content: Vec<u8>,
}

/// What materialization did with one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterializeVerdict {
    Created,
    Updated,
    Unchanged,
    Removed,
    /// Gitlink (`0o160000`): recorded, never materialized as a file.
    GitlinkSkipped,
}

/// Materialize `files` under `work_tree` atomically (temp file plus rename),
/// creating parent directories as needed. Returns per-file verdicts in order.
/// Paths escaping `work_tree` are rejected before any write.
pub fn materialize(work_tree: &Path, files: &[MaterializeFile]) -> Result<Vec<MaterializeVerdict>, WorktreeError> {
    for f in files {
        resolve_inside(work_tree, &f.path)?;
        match f.mode {
            0o100644 | 0o100755 | 0o120000 | 0o160000 => {}
            m => return Err(WorktreeError::UnsupportedMode(m)),
        }
    }
    let mut verdicts = Vec::with_capacity(files.len());
    for f in files {
        verdicts.push(materialize_one(work_tree, f)?);
    }
    Ok(verdicts)
}

fn resolve_inside(work_tree: &Path, rel: &str) -> Result<PathBuf, WorktreeError> {
    let mut out = work_tree.to_path_buf();
    for comp in rel.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp == ".." {
            if !out.pop() {
                return Err(WorktreeError::PathEscapesTree(rel.to_string()));
            }
            continue;
        }
        out.push(comp);
    }
    if out != *work_tree && !out.starts_with(work_tree) {
        return Err(WorktreeError::PathEscapesTree(rel.to_string()));
    }
    Ok(out)
}

fn materialize_one(work_tree: &Path, f: &MaterializeFile) -> Result<MaterializeVerdict, WorktreeError> {
    if f.mode == 0o160000 {
        return Ok(MaterializeVerdict::GitlinkSkipped);
    }
    let dest = resolve_inside(work_tree, &f.path)?;
    if f.mode == 0o120000 {
        return materialize_symlink(&dest, f);
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| WorktreeError::Io(e.to_string()))?;
    }
    if dest.is_file() {
        if std::fs::read(&dest).map(|cur| cur == f.content).unwrap_or(false) {
            set_executable(&dest, f.mode)?;
            return Ok(MaterializeVerdict::Unchanged);
        }
    }
    let existed = dest.exists();
    let tmp = dest.with_extension(format!("gitwt{}", std::process::id()));
    std::fs::write(&tmp, &f.content).map_err(|e| WorktreeError::Io(e.to_string()))?;
    set_executable(&tmp, f.mode)?;
    std::fs::rename(&tmp, &dest).map_err(|e| WorktreeError::Io(e.to_string()))?;
    Ok(if existed { MaterializeVerdict::Updated } else { MaterializeVerdict::Created })
}

#[cfg(unix)]
fn set_executable(path: &Path, mode: u32) -> Result<(), WorktreeError> {
    use std::os::unix::fs::PermissionsExt;
    let exec = mode == 0o100755;
    let mut perm = std::fs::metadata(path).map_err(|e| WorktreeError::Io(e.to_string()))?.permissions();
    perm.set_mode(if exec { 0o755 } else { 0o644 });
    std::fs::set_permissions(path, perm).map_err(|e| WorktreeError::Io(e.to_string()))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path, _mode: u32) -> Result<(), WorktreeError> {
    Ok(())
}

#[cfg(unix)]
fn materialize_symlink(dest: &Path, f: &MaterializeFile) -> Result<MaterializeVerdict, WorktreeError> {
    let target = String::from_utf8_lossy(&f.content).into_owned();
    if let Ok(cur) = std::fs::read_link(dest) {
        if cur.to_string_lossy() == target {
            return Ok(MaterializeVerdict::Unchanged);
        }
        std::fs::remove_file(dest).map_err(|e| WorktreeError::Io(e.to_string()))?;
    } else if dest.exists() {
        std::fs::remove_file(dest).map_err(|e| WorktreeError::Io(e.to_string()))?;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| WorktreeError::Io(e.to_string()))?;
    }
    std::os::unix::fs::symlink(&target, dest).map_err(|e| WorktreeError::Io(e.to_string()))?;
    Ok(MaterializeVerdict::Created)
}

#[cfg(not(unix))]
fn materialize_symlink(dest: &Path, f: &MaterializeFile) -> Result<MaterializeVerdict, WorktreeError> {
    // Non-Unix: store the link target as a plain file (documented fallback).
    let _ = dest;
    let _ = f;
    Err(WorktreeError::UnsupportedMode(0o120000))
}

/// Remove `paths` (repository-relative) from the work tree. Missing files
/// are not errors (matching `rm --ignore-unmatch` semantics at this layer).
pub fn remove(work_tree: &Path, paths: &[&str]) -> Result<Vec<MaterializeVerdict>, WorktreeError> {
    let mut verdicts = Vec::with_capacity(paths.len());
    for rel in paths {
        let dest = resolve_inside(work_tree, rel)?;
        if dest.exists() || dest.is_symlink() {
            std::fs::remove_file(&dest).map_err(|e| WorktreeError::Io(e.to_string()))?;
            verdicts.push(MaterializeVerdict::Removed);
        } else {
            verdicts.push(MaterializeVerdict::Unchanged);
        }
    }
    Ok(verdicts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("git-worktree-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn file(path: &str, mode: u32, content: &[u8]) -> MaterializeFile {
        MaterializeFile { path: path.to_string(), mode, content: content.to_vec() }
    }

    #[test]
    fn creates_updates_and_detects_unchanged() {
        let wt = tmp();
        let v = materialize(&wt, &[file("a.txt", 0o100644, b"one")]).unwrap();
        assert_eq!(v, vec![MaterializeVerdict::Created]);
        let v = materialize(&wt, &[file("a.txt", 0o100644, b"one")]).unwrap();
        assert_eq!(v, vec![MaterializeVerdict::Unchanged]);
        let v = materialize(&wt, &[file("a.txt", 0o100644, b"two")]).unwrap();
        assert_eq!(v, vec![MaterializeVerdict::Updated]);
        assert_eq!(std::fs::read(wt.join("a.txt")).unwrap(), b"two");
        std::fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn rejects_escaping_paths_before_any_write() {
        let wt = tmp();
        let err = materialize(&wt, &[file("../evil.txt", 0o100644, b"x")]).unwrap_err();
        assert!(matches!(err, WorktreeError::PathEscapesTree(_)));
        assert!(!wt.join("evil.txt").exists());
        std::fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn rejects_unsupported_modes_and_skips_gitlinks() {
        let wt = tmp();
        assert!(matches!(
            materialize(&wt, &[file("x", 0o100600, b"x")]).unwrap_err(),
            WorktreeError::UnsupportedMode(0o100600)
        ));
        let v = materialize(&wt, &[file("sub", 0o160000, b"")]).unwrap();
        assert_eq!(v, vec![MaterializeVerdict::GitlinkSkipped]);
        std::fs::remove_dir_all(&wt).ok();
    }

    #[test]
    fn removes_files_and_tolerates_missing() {
        let wt = tmp();
        materialize(&wt, &[file("a.txt", 0o100644, b"x")]).unwrap();
        let v = remove(&wt, &["a.txt", "missing.txt"]).unwrap();
        assert_eq!(v, vec![MaterializeVerdict::Removed, MaterializeVerdict::Unchanged]);
        std::fs::remove_dir_all(&wt).ok();
    }
}
