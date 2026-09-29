//! Reference storage: the files backend and packed-refs reading.
//!
//! A port of the loose-refs + packed-refs reader from `refs.c` /
//! `refs/files-backend.c`. Loose refs are files containing a hex oid or a
//! `ref: <target>` symref; `packed-refs` holds the packed ones. Reftable
//! support is deferred.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use git_core::Repository;
use git_hash::{HashAlgorithm, Oid};

pub mod lock;
pub mod packed;
pub mod reflog;
pub mod transaction;

const MAX_SYMREF_DEPTH: usize = 10;

/// C `is_per_worktree_ref`: `refs/worktree/`, `refs/bisect/` and
/// `refs/rewritten/` live in the worktree's own git dir.
fn is_per_worktree_ref(name: &str) -> bool {
    name.starts_with("refs/worktree/") || name.starts_with("refs/bisect/") || name.starts_with("refs/rewritten/")
}

/// Errors from ref operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefError {
    Io(String),
    InvalidName(String),
    NotFound,
    /// Lock creation failed: carries C's `Unable to create '<lock>': ...`
    /// detail (naming the lock file); the transaction layer wraps it in
    /// `cannot lock ref` / `update_ref failed for ref` text.
    LockContention(String),
    /// Transaction rejected: the full C detail (`cannot lock ref ...`,
    /// `multiple updates ...`, `cannot process ...`); single-ref callers
    /// add the `update_ref failed for ref` wrapper.
    Transaction(String),
}

impl fmt::Display for RefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RefError::Io(e) => write!(f, "ref I/O error: {e}"),
            RefError::InvalidName(n) => write!(f, "invalid ref name '{n}'"),
            RefError::NotFound => write!(f, "ref not found"),
            RefError::LockContention(d) => write!(f, "cannot lock ref: {d}"),
            RefError::Transaction(d) => write!(f, "{d}"),
        }
    }
}

impl Error for RefError {}

/// What a loose ref file points at.
#[derive(Debug, Clone)]
enum RefTarget {
    Oid(Oid),
    SymRef(String),
}

/// A ref store rooted at a repository.
#[derive(Debug, Clone)]
pub struct RefStore {
    git_dir: PathBuf,
    common_dir: PathBuf,
    algo: HashAlgorithm,
}

impl RefStore {
    pub fn from_repo(repo: &Repository) -> RefStore {
        RefStore {
            git_dir: repo.git_dir.clone(),
            common_dir: repo.common_dir.clone(),
            algo: repo.hash_algo,
        }
    }

    /// Resolve a ref name (following symrefs) to an object id.
    pub fn resolve(&self, name: &str) -> Option<Oid> {
        let mut cur = name.to_string();
        for _ in 0..MAX_SYMREF_DEPTH {
            let target = self.read_loose(&cur).or_else(|| {
                self.packed().and_then(|p| p.get(&cur).copied().map(RefTarget::Oid))
            })?;
            match target {
                RefTarget::Oid(oid) => return Some(oid),
                RefTarget::SymRef(next) => cur = next,
            }
        }
        None
    }

    /// The refname `HEAD` points at, if it is a symbolic ref.
    pub fn head_symbolic_target(&self) -> Option<String> {
        match self.read_loose("HEAD")? {
            RefTarget::SymRef(t) => Some(t),
            _ => None,
        }
    }

    /// All refs (loose overrides packed), sorted by refname.
    pub fn list(&self) -> Vec<(String, Oid)> {
        let mut map: HashMap<String, Oid> = HashMap::new();
        if let Some(packed) = self.packed() {
            for (k, v) in packed {
                map.insert(k, v);
            }
        }
        for (name, oid) in self.list_loose() {
            map.insert(name, oid);
        }
        let mut refs: Vec<(String, Oid)> = map.into_iter().collect();
        refs.sort_by(|a, b| a.0.cmp(&b.0));
        refs
    }

    /// Create or update a ref through a single-op transaction (C-exact
    /// dot-lock lifecycle plus old-value/D/F validation; failures carry
    /// the `update_ref failed for ref` wrapper C's single-ref path emits).
    /// Deletes take the lock too (contention must fail, never tear).
    pub fn update(&self, name: &str, oid: Option<&Oid>) -> Result<(), RefError> {
        let mut tx = transaction::Transaction::begin(self);
        match oid {
            Some(new) => tx.queue(transaction::TxnOp::Set {
                name: name.to_string(),
                new: *new,
                old: None,
                deref: false,
            }),
            None => tx.queue(transaction::TxnOp::Delete {
                name: name.to_string(),
                old: None,
                deref: false,
            }),
        }
        tx.prepare().map_err(|e| self.wrap_single(name, e))?;
        tx.commit().map_err(|e| self.wrap_single(name, e))
    }

    /// Add C's single-ref `update_ref failed for ref '<name>': ...` wrapper
    /// (C `refs_update_ref`, DIE_ON_ERR for the `update-ref` builtin).
    fn wrap_single(&self, name: &str, e: RefError) -> RefError {
        match e {
            RefError::Transaction(d) => {
                RefError::Transaction(format!("update_ref failed for ref '{name}': {d}"))
            }
            other => RefError::Transaction(format!(
                "update_ref failed for ref '{name}': cannot lock ref '{name}': {other}"
            )),
        }
    }

    /// Loose-ref path for `name` (C files-backend placement: `HEAD`
    /// and per-worktree refs live in the worktree git dir, everything
    /// else in the common dir).
    pub(crate) fn loose_path(&self, name: &str) -> PathBuf {
        if name == "HEAD" || is_per_worktree_ref(name) {
            self.git_dir.join(name)
        } else {
            self.common_dir.join(name)
        }
    }

    /// The common dir (transaction escape checks and the expire ref lock
    /// anchor here).
    pub(crate) fn common_path(&self) -> &Path {
        &self.common_dir
    }

    /// The common dir, for command-layer locks that must live next to the
    /// ref files (e.g. the `reflog expire` ref lock, C `lock_ref_oid_basic`).
    pub fn common_dir(&self) -> &Path {
        &self.common_dir
    }

    /// The worktree git dir (per-worktree `logs/` live here; C resolves
    /// `@{...}` selectors against it).
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// Follow symrefs to the terminal write location (depth-capped; C's
    /// deref walk for `update-ref`). With `deref` false, or when the chain
    /// cannot be followed, returns the name itself.
    pub fn deref_name_opt(&self, name: &str, deref: bool) -> String {
        if !deref {
            return name.to_string();
        }
        let mut cur = name.to_string();
        for _ in 0..MAX_SYMREF_DEPTH {
            let content = match self.read_loose(&cur) {
                Some(RefTarget::SymRef(next)) => next,
                _ => return cur,
            };
            cur = content;
        }
        cur
    }

    /// Raw loose content without symref following (transaction layer).
    pub(crate) fn read_raw(&self, name: &str) -> Option<transaction::RawRef> {
        match self.read_loose(name)? {
            RefTarget::Oid(oid) => Some(transaction::RawRef::Oid(oid)),
            RefTarget::SymRef(t) => Some(transaction::RawRef::Symref(t)),
        }
    }

    /// Packed fallback for the transaction's under-lock read.
    pub(crate) fn packed_oid(&self, name: &str) -> Option<Oid> {
        self.packed()?.get(name).copied()
    }

    /// Packed refnames, for the transaction's D/F checks (C checks packed
    /// refs too) and delete-time packed pruning.
    pub(crate) fn packed_refs(&self) -> Option<Vec<String>> {
        Some(self.packed()?.into_keys().collect())
    }

    /// Read a loose ref file (oid or symref). Per-worktree refs of OTHER
    /// worktrees (sitting in the common dir) are invisible here, like C.
    fn read_loose(&self, name: &str) -> Option<RefTarget> {
        for dir in [&self.git_dir, &self.common_dir] {
            if dir == &self.common_dir && self.git_dir != self.common_dir && is_per_worktree_ref(name) {
                continue;
            }
            let p = dir.join(name);
            let content = std::fs::read_to_string(&p).ok()?;
            let t = content.trim();
            if let Some(target) = t.strip_prefix("ref:") {
                return Some(RefTarget::SymRef(target.trim().to_string()));
            }
            if let Ok(oid) = Oid::from_hex(t, self.algo) {
                return Some(RefTarget::Oid(oid));
            }
        }
        None
    }

    /// The packed-refs map, if the file exists.
    fn packed(&self) -> Option<HashMap<String, Oid>> {
        let path = self.common_dir.join("packed-refs");
        let content = std::fs::read_to_string(&path).ok()?;
        let mut map = HashMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            let mut it = line.splitn(2, ' ');
            let oid_s = it.next()?;
            let name = it.next()?;
            if let Ok(oid) = Oid::from_hex(oid_s, self.algo) {
                map.insert(name.to_string(), oid);
            }
        }
        Some(map)
    }

    /// Walk the loose refs under `refs/`: the common dir fully, plus the
    /// worktree git dir when it differs (its per-worktree refs). Common
    /// per-worktree names stay out unless this IS the main worktree.
    fn list_loose(&self) -> Vec<(String, Oid)> {
        let mut out = Vec::new();
        let main_view = self.git_dir == self.common_dir;
        self.walk_refs_filtered(&self.common_dir.join("refs"), "", &mut out, main_view);
        if !main_view {
            self.walk_refs(&self.git_dir.join("refs"), "", &mut out);
        }
        out
    }

    /// Walk refs, skipping per-worktree names unless `include_per_worktree`.
    fn walk_refs_filtered(&self, dir: &Path, prefix: &str, out: &mut Vec<(String, Oid)>, include_per_worktree: bool) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let full = if prefix.is_empty() {
                format!("refs/{name}")
            } else {
                format!("{prefix}/{name}")
            };
            if e.path().is_dir() {
                // Prune per-worktree subtrees early when excluded.
                if !include_per_worktree && is_per_worktree_ref(&format!("{full}/")) {
                    continue;
                }
                self.walk_refs_filtered(&e.path(), &full, out, include_per_worktree);
            } else {
                if !include_per_worktree && is_per_worktree_ref(&full) {
                    continue;
                }
                if let Some(RefTarget::Oid(oid)) = self.read_loose(&full) {
                    out.push((full, oid));
                }
            }
        }
    }

    fn walk_refs(&self, dir: &Path, prefix: &str, out: &mut Vec<(String, Oid)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let full = if prefix.is_empty() {
                format!("refs/{name}")
            } else {
                format!("{prefix}/{name}")
            };
            if e.path().is_dir() {
                self.walk_refs(&e.path(), &full, out);
            } else if let Some(RefTarget::Oid(oid)) = self.read_loose(&full) {
                out.push((full, oid));
            }
        }
    }
}

/// Validate a ref name against git's rules: `HEAD` plus the `refs/`
/// hierarchy (C one-level rule: no other bare single-component names),
/// never a bare `@` (C bare-at rule), never `@{`, `.lock`, or reflog-suffix
/// shapes, plus the component character rules.
pub fn validate_refname(name: &str) -> Result<(), RefError> {
    if name == "@" {
        return Err(RefError::InvalidName(name.to_string()));
    }
    if name != "HEAD" && !name.starts_with("refs/") {
        return Err(RefError::InvalidName(name.to_string()));
    }
    check_refname_components(name)
}

/// Validate a ref name the way `update-ref --stdin`, `reflog exists`, and
/// `symbolic-ref` do (C `check_refname_format` with
/// `REFNAME_ALLOW_ONELEVEL`): single-component names like `PSEUDOREF` or
/// `ORIG_HEAD` are accepted alongside the full `refs/` hierarchy and
/// `HEAD`. Everything else matches [`validate_refname`].
pub fn validate_refname_allow_onelevel(name: &str) -> Result<(), RefError> {
    if name == "@" {
        return Err(RefError::InvalidName(name.to_string()));
    }
    if name != "HEAD" && !name.starts_with("refs/") && name.contains('/') {
        return Err(RefError::InvalidName(name.to_string()));
    }
    check_refname_components(name)
}

/// Component character rules shared by both validators (C
/// `check_or_sanitize_refname` component checks: `..`, `@{`, `.lock`,
/// trailing/double slashes, `~^:?*[\\`, controls, spaces).
fn check_refname_components(name: &str) -> Result<(), RefError> {
    if name.contains("..")
        || name.contains("@{")
        || name.contains(".lock")
        || name.ends_with('/')
        || name.contains("//")
        || name.contains('~')
        || name.contains('^')
        || name.contains(':')
        || name.contains('?')
        || name.contains('*')
        || name.contains('[')
        || name.contains('\\')
    {
        return Err(RefError::InvalidName(name.to_string()));
    }
    if name.bytes().any(|b| b.is_ascii_control() || b == b' ') {
        return Err(RefError::InvalidName(name.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_core::RepoEnv;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn repo() -> (Repository, PathBuf) {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("git-refs-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let git = dir.join(".git");
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        std::fs::create_dir_all(git.join("refs/tags")).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let repo = Repository::discover_from(&dir, &RepoEnv::default()).unwrap();
        (repo, dir)
    }

    #[test]
    fn resolves_and_updates_refs() {
        let (repo, dir) = repo();
        let store = RefStore::from_repo(&repo);
        let oid = *HashAlgorithm::Sha1.empty_blob();
        store.update("refs/heads/main", Some(&oid)).unwrap();
        assert_eq!(store.resolve("refs/heads/main"), Some(oid));
        // Symref: HEAD -> refs/heads/main.
        assert_eq!(store.resolve("HEAD"), Some(oid));
        assert_eq!(store.head_symbolic_target().as_deref(), Some("refs/heads/main"));
        // Delete.
        store.update("refs/heads/main", None).unwrap();
        assert_eq!(store.resolve("refs/heads/main"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lists_refs() {
        let (repo, dir) = repo();
        let store = RefStore::from_repo(&repo);
        let oid = *HashAlgorithm::Sha1.empty_blob();
        store.update("refs/heads/a", Some(&oid)).unwrap();
        store.update("refs/heads/b", Some(&oid)).unwrap();
        store.update("refs/tags/t", Some(&oid)).unwrap();
        let refs = store.list();
        assert_eq!(
            refs,
            vec![
                ("refs/heads/a".to_string(), oid),
                ("refs/heads/b".to_string(), oid),
                ("refs/tags/t".to_string(), oid),
            ]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reads_packed_refs() {
        let (repo, dir) = repo();
        let oid = *HashAlgorithm::Sha1.empty_blob();
        std::fs::write(
            dir.join(".git/packed-refs"),
            format!("# pack-refs with: peeled fully-peeled sorted\n{oid} refs/heads/packed\n"),
        )
        .unwrap();
        let store = RefStore::from_repo(&repo);
        assert_eq!(store.resolve("refs/heads/packed"), Some(oid));
        assert!(store.list().iter().any(|(n, _)| n == "refs/heads/packed"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn validates_names() {
        assert!(validate_refname("refs/heads/main").is_ok());
        assert!(validate_refname("refs/heads/feature/x").is_ok());
        assert!(validate_refname("HEAD").is_ok());
        assert!(validate_refname("@").is_err());
        assert!(validate_refname("refs/heads/main..evil").is_err());
        assert!(validate_refname("refs/heads/").is_err());
        assert!(validate_refname("main").is_err());
    }
}