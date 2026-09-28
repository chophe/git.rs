//! Two-phase ref transactions: all-or-nothing multi-ref updates.
//!
//! A port of the `refs/files-backend.c` transaction prepare/commit path
//! (`lock_ref_for_update`, `ref_update` old-value checks, D/F collision
//! rules). [`Transaction::prepare`] acquires every `.lock` and runs every
//! validation — old-oid expectations, ref existence, directory/file
//! collisions, symref rules — before any rename; [`Transaction::commit`]
//! then publishes every lock, and any failure before that point aborts with
//! every ref byte-unchanged. Per D-05, no rename occurs inside the
//! validation loop.
//!
//! Error texts are C's, probed against the tree binary (2026-09-28):
//! duplicate ops, batch D/F, FS D/F both directions, the four old-value
//! outcomes, and the `unable to resolve reference` lock-time rule. The
//! single-ref `update_ref failed for ref` wrapper is added by
//! [`RefStore::update`](super::RefStore::update); batch callers (plan 01-04)
//! surface details unwrapped, like C's `--stdin` path.
//!
//! Known gap: deleting a packed-only ref leaves its stale packed entry
//! (loose removal only); packed-prune on delete belongs to the future
//! `pack-refs` work. Single-ref delete D/F conflicts surface here as fatal
//! 128 while C's `-d` path reports error/1 — the full matrix (with `t/t1404`
//! verification) belongs to plan 01-04.

use std::path::Path;

use git_hash::Oid;

use super::lock::{ensure_within, LockFile};
use super::{RefError, RefStore};

/// Maximum symref hops followed while resolving a deref op.
const MAX_DEREF_DEPTH: usize = 10;

/// One queued ref operation. `deref` mirrors C's default (follow symrefs);
/// [`RefStore::update`](super::RefStore::update) queues with `deref: false`
/// to preserve its literal-path semantics.
#[derive(Debug, Clone)]
pub enum TxnOp {
    /// Set to `new`; `old` is verified when present (`None` = no check).
    Set { name: String, new: Oid, old: Option<Oid>, deref: bool },
    /// Create only; fails when anything is present.
    Create { name: String, new: Oid, deref: bool },
    /// Delete; `old` is verified when present (`None` = unconditional).
    Delete { name: String, old: Option<Oid>, deref: bool },
    /// Check only, no write.
    Verify { name: String, old: Oid },
}

impl TxnOp {
    fn name(&self) -> &str {
        match self {
            TxnOp::Set { name, .. }
            | TxnOp::Create { name, .. }
            | TxnOp::Delete { name, .. }
            | TxnOp::Verify { name, .. } => name,
        }
    }

    fn deref(&self) -> bool {
        match self {
            TxnOp::Set { deref, .. } | TxnOp::Create { deref, .. } | TxnOp::Delete { deref, .. } => *deref,
            TxnOp::Verify { .. } => true,
        }
    }

    /// The old-oid expectation, if any. `Create` always expects absence.
    fn expected_old(&self, null: &Oid) -> Option<Expectation> {
        match self {
            TxnOp::Set { old, .. } | TxnOp::Delete { old, .. } => old.as_ref().map(|o| {
                if o == null {
                    Expectation::Absent
                } else {
                    Expectation::At(*o)
                }
            }),
            TxnOp::Create { .. } => Some(Expectation::Absent),
            TxnOp::Verify { old, .. } => Some(if old == null {
                Expectation::Absent
            } else {
                Expectation::At(*old)
            }),
        }
    }

    fn is_write(&self) -> bool {
        !matches!(self, TxnOp::Verify { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expectation {
    Absent,
    At(Oid),
}

/// What lives at a resolved ref path right now.
#[derive(Debug, Clone)]
enum Current {
    Missing,
    Oid(Oid),
    /// A symref that survived deref (only with `deref: false`).
    Symref,
}

/// A lock held from prepare through commit, with its pending write.
struct Prepared {
    display: String,
    lock: LockFile,
    /// Content to publish on commit (`None` = unlink target for deletes,
    /// no-op for verify).
    content: Option<Vec<u8>>,
    is_delete: bool,
}

/// An all-or-nothing batch over one [`RefStore`].
pub struct Transaction<'a> {
    store: &'a RefStore,
    ops: Vec<TxnOp>,
    prepared: Vec<Prepared>,
}

impl<'a> Transaction<'a> {
    pub fn begin(store: &'a RefStore) -> Transaction<'a> {
        Transaction { store, ops: Vec::new(), prepared: Vec::new() }
    }

    pub fn queue(&mut self, op: TxnOp) {
        self.ops.push(op);
    }

    fn fail(op: &TxnOp, detail: impl Into<String>) -> RefError {
        RefError::Transaction(format!("cannot lock ref '{}': {}", op.name(), detail.into()))
    }

    /// Validate everything and acquire every lock. No rename happens here;
    /// any `Err` leaves every ref byte-unchanged (held locks unlink on
    /// drop).
    pub fn prepare(&mut self) -> Result<(), RefError> {
        // Names first (C validates before locking).
        for op in &self.ops {
            if let Err(e) = super::validate_refname(op.name()) {
                return Err(RefError::Transaction(format!(
                    "cannot lock ref '{}': {e}",
                    op.name()
                )));
            }
        }
        // Duplicate ops in one batch are rejected (C: "multiple updates").
        for i in 0..self.ops.len() {
            for other in &self.ops[..i] {
                if other.name() == self.ops[i].name() {
                    return Err(RefError::Transaction(format!(
                        "multiple updates for ref '{}' not allowed",
                        self.ops[i].name()
                    )));
                }
            }
        }
        // Batch-internal D/F collisions (C: "cannot process ... at the same time").
        for i in 0..self.ops.len() {
            for j in (i + 1)..self.ops.len() {
                let (a, b) = (self.ops[i].name(), self.ops[j].name());
                if is_path_prefix(a, b) || is_path_prefix(b, a) {
                    return Err(RefError::Transaction(format!(
                        "cannot process '{a}' and '{b}' at the same time"
                    )));
                }
            }
        }

        let null: Oid = *self.store.algo.null_oid();
        for op in std::mem::take(&mut self.ops) {
            self.prepare_one(op, &null)?;
        }
        Ok(())
    }

    /// Publish every lock (all contents written first, then all renames).
    /// Consumes the transaction; `abort` (drop) unlinks instead.
    pub fn commit(mut self) -> Result<(), RefError> {
        for p in &mut self.prepared {
            if p.is_delete {
                // Unlink under the held lock; a missing file is already gone.
                let _ = std::fs::remove_file(p.lock.path());
            } else if let Some(content) = &p.content {
                p.lock.write_and_fsync(content).map_err(|e| {
                    RefError::Transaction(format!("cannot lock ref '{}': {e}", p.display))
                })?;
            }
        }
        for p in self.prepared.drain(..) {
            if p.is_delete || p.content.is_none() {
                p.lock.rollback();
            } else if let Err(e) = p.lock.commit() {
                return Err(RefError::Transaction(format!(
                    "cannot lock ref '{}': {e}",
                    p.display
                )));
            }
        }
        Ok(())
    }

    /// Abandon the batch, unlinking every held lock. (Dropping works too.)
    pub fn abort(self) {}

    /// Lock one op's target and validate it. Pushes onto `prepared`.
    fn prepare_one(&mut self, op: TxnOp, null: &Oid) -> Result<(), RefError> {
        let display = op.name().to_string();
        let target = self.resolve_target(&op)?;
        let path = self.store.loose_path(&target);

        // FS D/F: a child ref blocks creating/writing this path, and a
        // blocking file ancestor blocks descending to it.
        if op.is_write() {
            self.check_df(&op, &target)?;
        }

        if let Some(dir) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                return Err(self.df_error(&op, &target, &e));
            }
        }
        // A stale empty directory at the target (leftover from a deleted
        // `foo/bar` ref) is removed so `foo` can be created; C moves such
        // dirs out of the way, which is observationally identical when
        // they hold no entries (`t/t1410` "stale dirs").
        if path.is_dir() {
            let empty = std::fs::read_dir(&path)
                .map(|mut rd| rd.next().is_none())
                .unwrap_or(false);
            if empty {
                let _ = std::fs::remove_dir(&path);
            }
        }
        ensure_within(self.store.common_path(), &path).map_err(|e| self.wrap_update(&op, e))?;

        let lock = match LockFile::acquire(&path) {
            Ok(l) => l,
            Err(RefError::LockContention(d)) => {
                return Err(RefError::Transaction(format!(
                    "cannot lock ref '{}': {d}",
                    op.name()
                )));
            }
            Err(e) => return Err(self.wrap_update(&op, e)),
        };

        // Current value under lock: loose first, then packed (loose wins on
        // read, so a packed fallback only matters when loose is missing).
        let current = self.read_current(&target);
        self.check_expectation(&op, &current, null)?;

        let (content, is_delete) = match &op {
            TxnOp::Set { new, .. } | TxnOp::Create { new, .. } => (Some(format!("{new}\n").into_bytes()), false),
            TxnOp::Delete { .. } => (None, true),
            TxnOp::Verify { .. } => (None, false),
        };
        self.prepared.push(Prepared { display, lock, content, is_delete });
        Ok(())
    }

    /// Follow symrefs for deref ops (depth-capped); literal name otherwise.
    fn resolve_target(&self, op: &TxnOp) -> Result<String, RefError> {
        if !op.deref() {
            return Ok(op.name().to_string());
        }
        let mut cur = op.name().to_string();
        for _ in 0..MAX_DEREF_DEPTH {
            match self.store.read_raw(&cur) {
                Some(RawRef::Symref(next)) => cur = next,
                _ => return Ok(cur),
            }
        }
        Err(Self::fail(op, "too many levels of symbolic links"))
    }

    fn read_current(&self, target: &str) -> Current {
        match self.store.read_raw(target) {
            None => match self.store.packed_oid(target) {
                Some(oid) => Current::Oid(oid),
                None => Current::Missing,
            },
            Some(RawRef::Oid(oid)) => Current::Oid(oid),
            Some(RawRef::Symref(_)) => Current::Symref,
        }
    }

    /// Old-oid / existence rules (C `verify_old_values` + the lock-time
    /// `unable to resolve reference` rule).
    fn check_expectation(&self, op: &TxnOp, current: &Current, null: &Oid) -> Result<(), RefError> {
        let name = op.name();
        match (op.expected_old(null), current) {
            (None, _) => Ok(()),
            (Some(Expectation::Absent), Current::Missing) => Ok(()),
            (Some(Expectation::Absent), Current::Symref) if !op.deref() => {
                Err(Self::fail(op, "dangling symref already exists"))
            }
            (Some(Expectation::Absent), _) => {
                Err(Self::fail(op, "reference already exists"))
            }
            (Some(Expectation::At(_)), Current::Missing) => {
                Err(Self::fail(op, format!("unable to resolve reference '{name}'")))
            }
            (Some(Expectation::At(exp)), Current::Oid(cur)) if *cur == exp => Ok(()),
            (Some(Expectation::At(exp)), Current::Oid(cur)) => {
                Err(Self::fail(op, format!("is at {cur} but expected {exp}")))
            }
            // Literal (no-deref) update of a symref: C still compares the
            // old expectation against the value the symref points to.
            (Some(Expectation::At(exp)), Current::Symref) => {
                let at = self.store.resolve(name).unwrap_or(*null);
                if at == exp {
                    Ok(())
                } else {
                    Err(Self::fail(op, format!("is at {at} but expected {exp}")))
                }
            }
        }
    }

    /// D/F vs the filesystem: children block the path, blocking files
    /// block the descent. Deletes honor the same rule (C reports the same
    /// text even for `-d`).
    fn check_df(&self, op: &TxnOp, target: &str) -> Result<(), RefError> {
        // A child ref (loose file or packed name under `target/`) blocks.
        if let Some(child) = self.first_child(target) {
            return Err(Self::fail(op, format!("'{child}' exists; cannot create '{target}'")));
        }
        // A blocking file ancestor blocks the descent.
        if let Some(blocker) = self.blocking_ancestor(target) {
            return Err(Self::fail(op, format!("'{blocker}' exists; cannot create '{target}'")));
        }
        Ok(())
    }

    /// Map a `create_dir_all` failure to the C blocking-file text when an
    /// ancestor is a file, else a plain transaction error.
    fn df_error(&self, op: &TxnOp, target: &str, e: &std::io::Error) -> RefError {
        if let Some(blocker) = self.blocking_ancestor(target) {
            Self::fail(op, format!("'{blocker}' exists; cannot create '{target}'"))
        } else {
            Self::fail(op, e.to_string())
        }
    }

    /// Lexicographically-first loose child file under `target/`, if any.
    fn first_child(&self, target: &str) -> Option<String> {
        let base = self.store.loose_path(target);
        let mut children: Vec<String> = Vec::new();
        self.collect_files(&base, &format!("{target}/"), &mut children);
        children.sort();
        children.into_iter().next()
    }

    fn collect_files(&self, dir: &Path, prefix: &str, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut names: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        names.sort();
        for p in names {
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
            if p.is_dir() {
                self.collect_files(&p, &format!("{prefix}{name}/"), out);
            } else {
                out.push(format!("{prefix}{name}"));
            }
        }
    }

    /// Deepest ancestor of `target` that exists as a file.
    fn blocking_ancestor(&self, target: &str) -> Option<String> {
        let mut rel = Path::new(target);
        let mut stack: Vec<String> = Vec::new();
        while let Some(parent) = rel.parent() {
            if parent.as_os_str().is_empty() {
                break;
            }
            stack.push(parent.to_string_lossy().into_owned());
            rel = parent;
        }
        for anc in stack {
            let p = self.store.loose_path(&anc);
            if p.is_file() {
                return Some(anc);
            }
        }
        None
    }

    fn wrap_update(&self, op: &TxnOp, e: RefError) -> RefError {
        match e {
            RefError::Transaction(_) => e,
            other => RefError::Transaction(format!("cannot lock ref '{}': {other}", op.name())),
        }
    }
}

/// `a` is a path-prefix (directory ancestor) of `b`.
fn is_path_prefix(a: &str, b: &str) -> bool {
    b.len() > a.len() && b.starts_with(a) && b.as_bytes()[a.len()] == b'/'
}

/// The raw content of a loose ref file, without symref following.
#[derive(Debug, Clone)]
pub(super) enum RawRef {
    Oid(Oid),
    Symref(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_hash::HashAlgorithm;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn repo() -> (RefStore, PathBuf, Oid) {
        use git_core::{RepoEnv, Repository};
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("git-txn-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let git = dir.join(".git");
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let repo = Repository::discover_from(&dir, &RepoEnv::default()).unwrap();
        let store = RefStore::from_repo(&repo);
        let oid = *HashAlgorithm::Sha1.empty_blob();
        (store, dir, oid)
    }

    fn set(store: &RefStore, name: &str, new: &Oid, old: Option<&Oid>) -> Result<(), RefError> {
        let mut tx = Transaction::begin(store);
        tx.queue(TxnOp::Set { name: name.to_string(), new: *new, old: old.copied(), deref: false });
        tx.prepare()?;
        tx.commit()
    }

    fn refs_snapshot(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        let mut stack = vec![dir.join(".git/refs")];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Ok(data) = std::fs::read(&p) {
                    let rel = p.strip_prefix(dir).unwrap().to_string_lossy().into_owned();
                    out.push((rel, data));
                }
            }
        }
        out.sort();
        out
    }

    #[test]
    fn bad_old_oid_leaves_everything_byte_identical() {
        let (store, dir, oid) = repo();
        let algo = HashAlgorithm::Sha1;
        let other = Oid::from_hex("2222222222222222222222222222222222222222", algo).unwrap();
        let wrong = Oid::from_hex("1111111111111111111111111111111111111111", algo).unwrap();
        set(&store, "refs/heads/a", &oid, None).unwrap();
        set(&store, "refs/heads/b", &oid, None).unwrap();
        let before = refs_snapshot(&dir);
        // One good op + one bad old-oid: the whole batch must abort before
        // any rename, leaving the set byte-identical with no stray locks.
        let mut tx = Transaction::begin(&store);
        tx.queue(TxnOp::Set { name: "refs/heads/a".to_string(), new: other, old: None, deref: false });
        tx.queue(TxnOp::Set { name: "refs/heads/b".to_string(), new: other, old: Some(wrong), deref: false });
        let err = tx.prepare().unwrap_err();
        assert!(err.to_string().contains(&format!("is at {oid} but expected {wrong}")), "{err}");
        drop(tx); // held locks unlink on drop, like C's transaction free
        assert_eq!(refs_snapshot(&dir), before, "failed batch changed refs");
        assert!(refs_snapshot(&dir).iter().all(|(n, _)| !n.ends_with(".lock")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn old_oid_rules_match_c_text() {
        let (store, dir, oid) = repo();
        let null = *HashAlgorithm::Sha1.null_oid();
        let wrong = Oid::from_hex("1111111111111111111111111111111111111111", HashAlgorithm::Sha1).unwrap();
        set(&store, "refs/heads/a", &oid, None).unwrap();
        // Create-like (null old) on existing: "reference already exists".
        let e = set(&store, "refs/heads/a", &oid, Some(&null)).unwrap_err();
        assert!(e.to_string().contains("reference already exists"), "{e}");
        // Wrong non-null old: "is at X but expected Y".
        let e = set(&store, "refs/heads/a", &oid, Some(&wrong)).unwrap_err();
        assert!(e.to_string().contains(&format!("is at {oid} but expected {wrong}")), "{e}");
        // Missing with non-null expectation: "unable to resolve reference".
        let e = set(&store, "refs/heads/nope", &oid, Some(&oid)).unwrap_err();
        assert!(e.to_string().contains("unable to resolve reference 'refs/heads/nope'"), "{e}");
        // Missing with null expectation creates; matching old updates.
        set(&store, "refs/heads/new", &oid, Some(&null)).unwrap();
        set(&store, "refs/heads/a", &null, Some(&oid)).unwrap();
        assert_eq!(store.resolve("refs/heads/a"), Some(null));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn duplicates_and_batch_df_are_rejected_before_any_rename() {
        let (store, dir, oid) = repo();
        let mut tx = Transaction::begin(&store);
        tx.queue(TxnOp::Set { name: "refs/heads/a".to_string(), new: oid, old: None, deref: false });
        tx.queue(TxnOp::Set { name: "refs/heads/a".to_string(), new: oid, old: None, deref: false });
        let e = tx.prepare().unwrap_err();
        assert!(e.to_string().contains("multiple updates for ref 'refs/heads/a' not allowed"), "{e}");

        let mut tx = Transaction::begin(&store);
        tx.queue(TxnOp::Set { name: "refs/heads/a".to_string(), new: oid, old: None, deref: false });
        tx.queue(TxnOp::Set { name: "refs/heads/a/b".to_string(), new: oid, old: None, deref: false });
        let e = tx.prepare().unwrap_err();
        assert!(e.to_string().contains("cannot process 'refs/heads/a' and 'refs/heads/a/b'"), "{e}");
        assert!(store.resolve("refs/heads/a").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fs_df_conflicts_use_c_text() {
        let (store, dir, oid) = repo();
        // File in the way of a deeper create.
        set(&store, "refs/heads/a", &oid, None).unwrap();
        let e = set(&store, "refs/heads/a/b", &oid, None).unwrap_err();
        assert!(
            e.to_string().contains("'refs/heads/a' exists; cannot create 'refs/heads/a/b'"),
            "{e}"
        );
        // Directory in the way of a file create.
        set(&store, "refs/heads/d/e", &oid, None).unwrap();
        let e = set(&store, "refs/heads/d", &oid, None).unwrap_err();
        assert!(
            e.to_string().contains("'refs/heads/d/e' exists; cannot create 'refs/heads/d'"),
            "{e}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verify_checks_without_writing() {
        let (store, dir, oid) = repo();
        set(&store, "refs/heads/a", &oid, None).unwrap();
        let mut tx = Transaction::begin(&store);
        tx.queue(TxnOp::Verify { name: "refs/heads/a".to_string(), old: oid });
        tx.prepare().unwrap();
        tx.commit().unwrap();
        let wrong = *HashAlgorithm::Sha1.null_oid();
        let mut tx = Transaction::begin(&store);
        tx.queue(TxnOp::Verify { name: "refs/heads/a".to_string(), old: wrong });
        assert!(tx.prepare().is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn contention_aborts_with_lock_path() {
        let (store, dir, oid) = repo();
        set(&store, "refs/heads/a", &oid, None).unwrap();
        // Squat the lock: the batch must fail naming it, changing nothing.
        let lock = store.loose_path("refs/heads/a");
        let mut s = lock.into_os_string();
        s.push(".lock");
        std::fs::write(Path::new(&s), b"").unwrap();
        let before = refs_snapshot(&dir);
        let e = set(&store, "refs/heads/a", &oid, None).unwrap_err();
        assert!(e.to_string().contains(".lock"), "{e}");
        assert_eq!(refs_snapshot(&dir), before);
        std::fs::remove_dir_all(&dir).ok();
    }
}
