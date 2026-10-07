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
    /// Point a symref at `target` (C `symref-update`); `old_oid` is
    /// compared against the resolved value, `old_target` against the
    /// symref value itself, when present. In deref mode the chain is
    /// followed and the resolved ref is rewritten (even a onelevel name
    /// like C does for `symref-update sym other`); no-deref rewrites the
    /// named symref literally.
    SymrefUpdate {
        name: String,
        target: String,
        old_oid: Option<Oid>,
        old_target: Option<String>,
        deref: bool,
    },
    /// Create a symref (C `symref-create`); fails when anything is present.
    SymrefCreate { name: String, target: String, deref: bool },
    /// Delete a symref literally (C `symref-delete`, no-deref only);
    /// `old_target` is compared against the literal target when present.
    SymrefDelete { name: String, old_target: Option<String> },
    /// Check a symref literally (C `symref-verify`, no-deref only); no
    /// `old_target` means the ref must not exist.
    SymrefVerify { name: String, old_target: Option<String> },
}

impl TxnOp {
    fn name(&self) -> &str {
        match self {
            TxnOp::Set { name, .. }
            | TxnOp::Create { name, .. }
            | TxnOp::Delete { name, .. }
            | TxnOp::Verify { name, .. }
            | TxnOp::SymrefUpdate { name, .. }
            | TxnOp::SymrefCreate { name, .. }
            | TxnOp::SymrefDelete { name, .. }
            | TxnOp::SymrefVerify { name, .. } => name,
        }
    }

    fn deref(&self) -> bool {
        match self {
            TxnOp::Set { deref, .. }
            | TxnOp::Create { deref, .. }
            | TxnOp::Delete { deref, .. }
            | TxnOp::SymrefUpdate { deref, .. }
            | TxnOp::SymrefCreate { deref, .. } => *deref,
            TxnOp::Verify { .. } => true,
            // Delete/verify of symrefs always address the symref itself
            // (C passes the name literally; deref/no-deref only gates
            // *whether* they run).
            _ => false,
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
            TxnOp::SymrefUpdate { old_oid, .. } => old_oid.as_ref().map(|o| {
                if o == null {
                    Expectation::Absent
                } else {
                    Expectation::At(*o)
                }
            }),
            TxnOp::SymrefCreate { .. } => Some(Expectation::Absent),
            TxnOp::SymrefDelete { .. } => None,
            // Bare `symref-verify` (no old target) asserts absence (C
            // `ref_transaction_verify` with a null oid); anything present
            // fails like a create conflict.
            TxnOp::SymrefVerify { old_target: None, .. } => Some(Expectation::Absent),
            TxnOp::SymrefVerify { .. } => None,
        }
    }

    fn is_write(&self) -> bool {
        !matches!(self, TxnOp::Verify { .. } | TxnOp::SymrefVerify { .. })
    }

    /// The literal symref-target expectation, if any.
    fn expected_target(&self) -> Option<&str> {
        match self {
            TxnOp::SymrefUpdate { old_target, .. }
            | TxnOp::SymrefDelete { old_target, .. }
            | TxnOp::SymrefVerify { old_target, .. } => old_target.as_deref(),
            _ => None,
        }
    }

    /// Whether this op writes a symref file (content `ref: <target>`).
    fn symref_content(&self) -> Option<Vec<u8>> {
        match self {
            TxnOp::SymrefUpdate { target, .. } | TxnOp::SymrefCreate { target, .. } => {
                Some(format!("ref: {target}\n").into_bytes())
            }
            _ => None,
        }
    }

    fn is_delete(&self) -> bool {
        matches!(self, TxnOp::Delete { .. } | TxnOp::SymrefDelete { .. })
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
    /// Packed entry to prune on commit (deletes only).
    packed_prune: Option<String>,
}

/// An all-or-nothing batch over one [`RefStore`].
pub struct Transaction<'a> {
    store: &'a RefStore,
    ops: Vec<TxnOp>,
    prepared: Vec<Prepared>,
    /// Every queued op name (C `transaction->refnames`), stashed at
    /// `prepare` for the batch-internal D/F walk.
    all_names: Vec<String>,
}

impl<'a> Transaction<'a> {
    pub fn begin(store: &'a RefStore) -> Transaction<'a> {
        Transaction { store, ops: Vec::new(), prepared: Vec::new(), all_names: Vec::new() }
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
        // Names first, with the `update-ref` one-level allowance (C
        // `parse_refname` uses REFNAME_ALLOW_ONELEVEL): `PSEUDOREF`,
        // `ORIG_HEAD` and friends are storable refs. The transaction
        // backstop text matches C's name check (`refusing to update ref
        // with bad name`); the batch parser rejects bad names earlier
        // with `invalid ref format`.
        for op in &self.ops {
            if super::validate_refname_allow_onelevel(op.name()).is_err() {
                return Err(RefError::Transaction(format!(
                    "refusing to update ref with bad name '{}'",
                    op.name()
                )));
            }
        }
        // Duplicate ops in one batch are rejected: exact name twins
        // first (C `ref_update_reject_duplicates`), then write-target
        // collisions through symrefs (C's symref-split duplicates, with
        // the HEAD-specific wording).
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
        // Resolved write targets for the symref-split check (errors fall
        // back to the literal name; they surface later if real).
        let keys: Vec<String> = self
            .ops
            .iter()
            .map(|op| {
                Self::resolve_target(self.store, op).unwrap_or_else(|_| op.name().to_string())
            })
            .collect();
        // A literal (no-deref) HEAD update collides with any update to
        // HEAD's referent (C's HEAD-split duplicates; order-independent,
        // and HEAD-specific: non-HEAD literal symref updates do not
        // collide, probed on the tree binary).
        if let Some(referent) = self.store.read_raw("HEAD").and_then(|r| match r {
            RawRef::Symref(t) => Some(t),
            _ => None,
        }) {
            for (i, op) in self.ops.iter().enumerate() {
                if op.name() == "HEAD" && !op.deref() {
                    for (j, other) in self.ops.iter().enumerate() {
                        if i != j
                            && Self::resolve_target(self.store, other)
                                .unwrap_or_else(|_| other.name().to_string())
                                == referent
                        {
                            return Err(RefError::Transaction(format!(
                                "multiple updates for 'HEAD' (including one via its referent '{referent}') are not allowed"
                            )));
                        }
                    }
                }
            }
        }
        for i in 0..self.ops.len() {
            for j in (i + 1)..self.ops.len() {
                if keys[i] != keys[j] {
                    continue;
                }
                let (a, b) = (self.ops[i].name(), self.ops[j].name());
                // A deref update through HEAD collides under the
                // referent's name (C's HEAD-split wording).
                if a == "HEAD" || b == "HEAD" {
                    return Err(RefError::Transaction(format!(
                        "multiple updates for '{}' (including one via symref 'HEAD') are not allowed",
                        keys[i]
                    )));
                }
                // Name the symref side (literal symref file); fall back
                // to the plain form when neither side is one.
                let sym = if self.store.read_raw(a).is_some_and(|r| matches!(r, RawRef::Symref(_))) {
                    a
                } else if self.store.read_raw(b).is_some_and(|r| matches!(r, RawRef::Symref(_))) {
                    b
                } else {
                    return Err(RefError::Transaction(format!(
                        "multiple updates for ref '{}' not allowed",
                        keys[i]
                    )));
                };
                return Err(RefError::Transaction(format!(
                    "multiple updates for '{}' (including one via symref '{}') are not allowed",
                    keys[i], sym
                )));
            }
        }

        let null: Oid = *self.store.algo.null_oid();
        self.all_names = self.ops.iter().map(|op| op.name().to_string()).collect();
        for op in std::mem::take(&mut self.ops) {
            self.prepare_one(op, &null)?;
        }
        Ok(())
    }

    /// Prepare each queued op independently, holding successful locks
    /// and returning per-op rejections (C `ALLOW_FAILURE` semantics for
    /// `--batch-updates`: one bad op rejects while the rest still land).
    /// Survivors stay queued for [`commit`](Transaction::commit).
    /// `ignorecase` selects the case-conflict categorization for lock
    /// collisions (C checks `core.ignorecase` + a case-variant update).
    pub fn prepare_lenient(&mut self, ignorecase: bool) -> Vec<OpRejection> {
        self.all_names = self.ops.iter().map(|op| op.name().to_string()).collect();
        let mut rejected = Vec::new();
        for (index, op) in std::mem::take(&mut self.ops).into_iter().enumerate() {
            let display = op.name().to_string();
            match self.prepare_one_lenient(op, ignorecase) {
                Ok(()) => {}
                Err((detail, msg)) => rejected.push(OpRejection { index, display, detail, msg }),
            }
        }
        rejected
    }

    /// One lenient op: like [`prepare_one`](Transaction::prepare_one) but
    /// returning the C rejection class instead of dying.
    fn prepare_one_lenient(&mut self, op: TxnOp, ignorecase: bool) -> Result<(), (String, &'static str)> {
        let target = match Self::resolve_target(self.store, &op) {
            Ok(t) => t,
            Err(e) => return Err((e.to_string(), classify_detail(&e.to_string()))),
        };
        // Snapshot the pre-lock read for the exists-skip below; the full
        // prepare_one re-reads under lock identically (single-threaded).
        match self.prepare_one(op, &*self.store.algo.null_oid()) {
            Ok(()) => Ok(()),
            Err(e) => {
                let detail = e.to_string();
                // Case-insensitive lock collision (C `CASE_CONFLICT`):
                // contention plus a case-variant sibling update.
                if detail.contains("Unable to create") && ignorecase && self.has_case_variant(&target) {
                    return Err((detail, "reference conflict due to case-insensitive filesystem"));
                }
                Err((detail.clone(), classify_detail(&detail)))
            }
        }
    }

    /// Whether another batch op targets a case-variant of `target`.
    fn has_case_variant(&self, target: &str) -> bool {
        let lower = target.to_ascii_lowercase();
        self.all_names.iter().any(|n| n != target && n.to_ascii_lowercase() == lower)
    }

    /// Publish every lock (all contents written first, then all renames).
    /// Consumes the transaction; `abort` (drop) unlinks instead.
    pub fn commit(mut self) -> Result<(), RefError> {
        // Deletes prune the packed entry too (C drops packed refs on
        // delete, so `show-ref --verify` stays silent afterwards).
        // Collected before the drain below consumes `prepared`.
        let prune: Vec<String> = self
            .prepared
            .iter()
            .filter_map(|p| p.packed_prune.clone())
            .collect();
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
        // Deletes prune the packed entry too (C drops packed refs on
        // delete): rewrite packed-refs without the deleted names.
        if !prune.is_empty() {
            let names: Vec<&str> = prune.iter().map(String::as_str).collect();
            if let Err(e) =
                super::packed::prune_entries(self.store.common_path(), self.store.algo, &names)
            {
                return Err(RefError::Transaction(format!("cannot prune packed-refs: {e}")));
            }
        }
        Ok(())
    }

    /// Abandon the batch, unlinking every held lock. (Dropping works too.)
    pub fn abort(mut self) {
        // Collect lock parent dirs, release the locks (unlinking the
        // `.lock` files), then prune newly-empty dirs (C removes empty
        // dirs on abort; `t/t1400` "empty directories are pruned...").
        let mut dirs: Vec<std::path::PathBuf> = self
            .prepared
            .iter()
            .filter_map(|p| p.lock.path().parent().map(|d| d.to_path_buf()))
            .collect();
        drop(std::mem::take(&mut self.prepared));
        drop(std::mem::take(&mut self.ops));
        // Deepest first so nested empties collapse fully; best-effort
        // (non-empty dirs simply stay).
        dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
        dirs.dedup();
        for d in dirs {
            let _ = std::fs::remove_dir(&d);
        }
    }

    /// Lock one op's target and validate it. Pushes onto `prepared`.
    fn prepare_one(&mut self, op: TxnOp, null: &Oid) -> Result<(), RefError> {
        let display = op.name().to_string();
        let target = Self::resolve_target(self.store, &op)?;
        let path = self.store.loose_path(&target);

        // Current value first: C (`lock_raw_ref`) skips the D/F verify
        // when the ref was successfully read. Deletes always verify
        // (a deleted `foo` must not strand `foo/bar`), but only against
        // the filesystem, never the batch (t/t1404 df_test matrix).
        let current = self.read_current(&target);
        let current_exists = !matches!(current, Current::Missing);
        if op.is_write() {
            let check_extras = !op.is_delete() && !current_exists;
            self.check_df(&op, &target, check_extras)?;
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
        self.check_expectation(&op, &current, &target, null)?;
        // Literal symref-target expectations (C compares the symref value
        // itself for `symref-update/-delete/-verify` with an old target).
        self.check_target_expectation(&op, &target)?;

        let (content, is_delete) = match &op {
            TxnOp::Set { new, .. } | TxnOp::Create { new, .. } => (Some(format!("{new}\n").into_bytes()), false),
            TxnOp::Delete { .. } | TxnOp::SymrefDelete { .. } => (None, true),
            TxnOp::Verify { .. } | TxnOp::SymrefVerify { .. } => (None, false),
            TxnOp::SymrefUpdate { .. } | TxnOp::SymrefCreate { .. } => {
                (op.symref_content(), false)
            }
        };
        // C files-backend: a non-symref oid update whose locked old
        // value already equals the new one sets neither NEEDS_COMMIT
        // bit — `update-ref <ref> <same-sha>` writes nothing and logs
        // nothing (a lock is still taken, then closed, unobservable).
        let noop = match (&op, &current) {
            (TxnOp::Set { new, .. }, Current::Oid(old)) => new != null && old == new,
            _ => false,
        };
        let content = if noop { None } else { content };
        // Deletes prune the packed entry too (C removes packed refs on
        // delete); remembered for the commit phase.
        let packed_prune = if op.is_delete() { Some(target.clone()) } else { None };
        self.prepared.push(Prepared { display, lock, content, is_delete, packed_prune });
        Ok(())
    }

    /// Follow symrefs for deref ops (depth-capped); literal name otherwise.
    fn resolve_target(store: &RefStore, op: &TxnOp) -> Result<String, RefError> {
        if !op.deref() {
            return Ok(op.name().to_string());
        }
        let mut cur = op.name().to_string();
        for _ in 0..MAX_DEREF_DEPTH {
            match store.read_raw(&cur) {
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
    /// `unable to resolve reference` rule). `target` is the resolved write
    /// location, named by the "unable to resolve" errors (C names the
    /// dereferenced ref, e.g. t/t1404's indirect-update case).
    fn check_expectation(
        &self,
        op: &TxnOp,
        current: &Current,
        target: &str,
        null: &Oid,
    ) -> Result<(), RefError> {
        match (op.expected_old(null), current) {
            (None, _) => Ok(()),
            (Some(Expectation::Absent), Current::Missing) => Ok(()),
            // No-deref creation over a dangling symref preserves it (C
            // files-backend "dangling symref already exists"); over a
            // live symref it is a plain exists conflict.
            (Some(Expectation::Absent), Current::Symref) if !op.deref() => {
                if self.store.resolve(op.name()).is_none() {
                    Err(Self::fail(op, "dangling symref already exists"))
                } else {
                    Err(Self::fail(op, "reference already exists"))
                }
            }
            (Some(Expectation::Absent), _) => {
                Err(Self::fail(op, "reference already exists"))
            }
            (Some(Expectation::At(_)), Current::Missing) => {
                Err(Self::fail(op, format!("unable to resolve reference '{target}'")))
            }
            (Some(Expectation::At(exp)), Current::Oid(cur)) if *cur == exp => Ok(()),
            (Some(Expectation::At(exp)), Current::Oid(cur)) => {
                Err(Self::fail(op, format!("is at {cur} but expected {exp}")))
            }
            // Literal (no-deref) update of a symref: C still compares the
            // old expectation against the value the symref points to; a
            // dangling target reports "missing" (t/t1404 indirect
            // no-deref cases).
            (Some(Expectation::At(exp)), Current::Symref) => {
                match self.store.resolve(op.name()) {
                    None => Err(Self::fail(op, format!("reference is missing but expected {exp}"))),
                    Some(at) if at == exp => Ok(()),
                    Some(at) => Err(Self::fail(op, format!("is at {at} but expected {exp}"))),
                }
            }
        }
    }

    /// Literal symref-target expectations for the symref verbs, with C's
    /// exact error shapes (probed on the tree binary):
    /// - missing literal + expected target: `unable to resolve reference`;
    /// - regular file + expected target: `expected symref with target`;
    /// - symref with another target: `verifying symref target: ... is at
    ///   ... but expected ...`.
    fn check_target_expectation(&self, op: &TxnOp, target: &str) -> Result<(), RefError> {
        let Some(expected) = op.expected_target() else { return Ok(()) };
        match self.store.read_raw(target) {
            Some(RawRef::Symref(t)) if t == expected => Ok(()),
            Some(RawRef::Symref(t)) => Err(Self::fail(
                op,
                format!("verifying symref target: '{}': is at {t} but expected {expected}", op.name()),
            )),
            Some(RawRef::Oid(_)) => Err(Self::fail(
                op,
                format!("expected symref with target '{expected}': but is a regular ref"),
            )),
            None => Err(Self::fail(op, format!("unable to resolve reference '{target}'"))),
        }
    }

    /// D/F vs the filesystem, plus the batch-internal collision check (C
    /// `refs_verify_refname_available` with `extras` = the batch names):
    /// a child ref blocks creating/writing this path, a blocking file
    /// ancestor blocks the descent, and a queued ancestor/descendant
    /// collides with "cannot process ... at the same time". Deletes check
    /// the filesystem only (t/t1404 df_test: the create's FS error wins).
    fn check_df(&self, op: &TxnOp, target: &str, check_extras: bool) -> Result<(), RefError> {
        // A child ref (loose file or packed name under `target/`) blocks.
        if let Some(child) = self.first_child(target) {
            return Err(Self::fail(op, format!("'{child}' exists; cannot create '{target}'")));
        }
        // A blocking file ancestor blocks the descent.
        if let Some(blocker) = self.blocking_ancestor(target) {
            return Err(Self::fail(op, format!("'{blocker}' exists; cannot create '{target}'")));
        }
        if check_extras {
            // Ancestors first (shortest first), then descendants
            // (lexicographically first), like C's dirname walk.
            let mut ancestors: Vec<String> = Vec::new();
            let mut rel = Path::new(target);
            while let Some(parent) = rel.parent() {
                if parent.as_os_str().is_empty() {
                    break;
                }
                ancestors.push(parent.to_string_lossy().into_owned());
                rel = parent;
            }
            ancestors.reverse();
            for anc in &ancestors {
                if self.batch_names().iter().any(|n| n == anc) {
                    return Err(Self::fail(
                        op,
                        format!("cannot process '{target}' and '{anc}' at the same time"),
                    ));
                }
            }
            let prefix = format!("{target}/");
            let mut desc: Vec<&String> =
                self.batch_names().iter().filter(|n| n.starts_with(&prefix)).collect();
            desc.sort();
            if let Some(first) = desc.into_iter().next() {
                return Err(Self::fail(
                    op,
                    format!("cannot process '{target}' and '{first}' at the same time"),
                ));
            }
        }
        Ok(())
    }

    /// Every op name queued in this batch (C `transaction->refnames`).
    fn batch_names(&self) -> &[String] {
        &self.all_names
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

    /// Lexicographically-first child ref under `target/`, loose or packed
    /// (C checks packed refs too: t/t1404's packed D/F cases).
    fn first_child(&self, target: &str) -> Option<String> {
        let base = self.store.loose_path(target);
        let mut children: Vec<String> = Vec::new();
        self.collect_files(&base, &format!("{target}/"), &mut children);
        let prefix = format!("{target}/");
        if let Some(packed) = self.store.packed_refs() {
            children.extend(packed.iter().filter(|n| n.starts_with(&prefix)).cloned());
        }
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

    /// Deepest ancestor of `target` that exists as a file, loose or
    /// packed (C checks packed refs too).
    fn blocking_ancestor(&self, target: &str) -> Option<String> {
        let packed = self.store.packed_refs();
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
            if let Some(names) = &packed {
                if names.iter().any(|n| n == &anc) {
                    return Some(anc);
                }
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

/// One rejected op from [`Transaction::prepare_lenient`]: the queued
/// index (for log correlation), the display name, C's detail text, and
/// C's rejection class (`ref_transaction_error_msg`).
#[derive(Debug, Clone)]
pub struct OpRejection {
    pub index: usize,
    pub display: String,
    pub detail: String,
    pub msg: &'static str,
}

/// Map a transaction detail to C's rejection class by its stable
/// message shapes (all probed against the tree binary).
fn classify_detail(detail: &str) -> &'static str {
    if detail.contains("unable to resolve reference") {
        "reference does not exist"
    } else if detail.contains("is at ") && detail.contains(" but expected ") {
        "incorrect old value provided"
    } else if detail.contains("reference already exists") || detail.contains("dangling symref already exists") {
        "reference already exists"
    } else if detail.contains("exists; cannot create") || detail.contains("cannot process") {
        "refname conflict"
    } else if detail.contains("verifying symref target") || detail.contains("expected symref") {
        "expected symref but found regular ref"
    } else if detail.contains("Unable to create") {
        // Non-case lock contention categorizes as exists (C
        // `CREATE_EXISTS` fallback); the case variant is detected by
        // the caller, which overrides with the filesystem message.
        "reference already exists"
    } else {
        // C's default (`unknown failure`) for unmapped kinds.
        "unknown failure"
    }
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
