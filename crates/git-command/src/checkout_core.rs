//! Shared worktree/index machinery for `reset`, `checkout`, `switch`, and
//! `restore` (a port of the relevant parts of `unpack-trees.c`,
//! `builtin/reset.c`, and `builtin/checkout.c`).
//!
//! The workhorse is a two-way tree comparison (`old` = HEAD tree, `new` =
//! target tree) with `verify_uptodate`-style safety checks, plus index
//! rebuild and worktree write-out helpers. Three-way merging (`checkout -m`,
//! `switch --merge`, `reset --merge/--keep`) is not implemented yet; those
//! options fail with an explicit error in the command modules.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::{CommandError, RepoContext};
use git_hash::{HashAlgorithm, Oid};
use git_index::{Index, IndexEntry};
use git_object::{parse_commit, parse_tag, parse_tree, Object, ObjectKind};
use git_odb::Odb;

/// One flattened tree entry with its blob payload.
#[derive(Debug, Clone)]
pub(crate) struct BlobInfo {
    pub mode: u32,
    pub oid: Oid,
    pub data: Vec<u8>,
}

/// A tree flattened to `path -> blob`, in byte order.
pub(crate) type TreeMap = BTreeMap<String, BlobInfo>;

/// The state of `HEAD`: a symref target and/or a resolved oid.
#[derive(Debug, Clone, Default)]
pub(crate) struct HeadState {
    pub symref: Option<String>,
    pub oid: Option<Oid>,
}

/// Read `HEAD` (symref target via the loose file, oid via discovery).
pub(crate) fn read_head(repo: &git_core::Repository) -> HeadState {
    let symref = git_refs::RefStore::from_repo(repo).head_symbolic_target();
    HeadState { symref, oid: repo.resolve_head() }
}

/// Peel `oid` to a commit (following tags). Returns the commit id, or an
/// error describing the blocking object type.
pub(crate) fn peel_to_commit(
    odb: &Odb,
    oid: &Oid,
) -> Result<Oid, (Oid, &'static str)> {
    let mut cur = *oid;
    loop {
        match odb.read(&cur) {
            Ok(o) if o.kind == ObjectKind::Commit => return Ok(cur),
            Ok(o) if o.kind == ObjectKind::Tag => {
                match parse_tag(&o.data, odb.algorithm()) {
                    Ok(t) => cur = t.object,
                    Err(_) => return Err((*oid, "tag")),
                }
            }
            Ok(o) => {
                let kind = match o.kind {
                    ObjectKind::Blob => "blob",
                    ObjectKind::Tree => "tree",
                    ObjectKind::Tag => "tag",
                    ObjectKind::Commit => "commit",
                };
                return Err((*oid, kind));
            }
            Err(_) => return Err((*oid, "bad")),
        }
    }
}

/// The tree of a commit (or the object itself if it is a tree).
/// Blobs and missing objects fail with "unable to read tree".
pub(crate) fn commit_or_tree_to_tree(
    odb: &Odb,
    algo: HashAlgorithm,
    oid: &Oid,
) -> Result<Oid, CommandError> {
    match odb.read(oid) {
        Ok(o) if o.kind == ObjectKind::Commit => match parse_commit(&o.data, algo) {
            Ok(c) => Ok(c.tree),
            Err(e) => Err(CommandError::fatal(format!("fatal: unable to read tree ({oid}): {e}"))),
        },
        Ok(o) if o.kind == ObjectKind::Tree => Ok(*oid),
        Ok(o) if o.kind == ObjectKind::Tag => match parse_tag(&o.data, algo) {
            Ok(t) => commit_or_tree_to_tree(odb, algo, &t.object),
            Err(_) => Err(CommandError::fatal(format!("fatal: unable to read tree ({oid})"))),
        },
        _ => Err(CommandError::fatal(format!("fatal: unable to read tree ({oid})"))),
    }
}

/// The well-known empty-tree oid for an algorithm.
pub(crate) fn empty_tree_oid(algo: HashAlgorithm) -> Oid {
    match algo {
        HashAlgorithm::Sha1 => {
            Oid::from_hex("4b825dc642cb6eb9a060e54bf8d69288fbee4904", algo).unwrap()
        }
        HashAlgorithm::Sha256 => Oid::from_hex(
            "6ef19b412cc8e048e9cf35a6268637a49cf071ff0e594c7bef9cf9d0f94fff9",
            algo,
        )
        .unwrap(),
    }
}

/// Recursively flatten a tree object into a path map (blob payloads included).
/// Gitlinks (mode 160000) are skipped: submodule checkouts are out of scope.
/// The empty tree needs no object lookup.
pub(crate) fn tree_to_map(
    odb: &Odb,
    algo: HashAlgorithm,
    tree_oid: &Oid,
) -> Result<TreeMap, CommandError> {
    if *tree_oid == empty_tree_oid(algo) {
        return Ok(TreeMap::new());
    }
    let mut out = TreeMap::new();
    let mut stack: Vec<(Oid, String)> = vec![(*tree_oid, String::new())];
    let mut seen = HashSet::new();
    while let Some((oid, prefix)) = stack.pop() {
        if !seen.insert(oid) {
            continue;
        }
        let obj = odb
            .read(&oid)
            .map_err(|_| CommandError::fatal(format!("fatal: unable to read tree ({oid})")))?;
        if obj.kind != ObjectKind::Tree {
            return Err(CommandError::fatal(format!("fatal: unable to read tree ({oid})")));
        }
        let entries = parse_tree(&obj.data, algo)
            .map_err(|e| CommandError::fatal(format!("fatal: unable to read tree ({oid}): {e}")))?;
        for e in entries {
            let name = String::from_utf8_lossy(&e.name).into_owned();
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let mode = u32::from_str_radix(&e.mode, 8).unwrap_or(0);
            if e.is_dir() {
                stack.push((e.oid.clone(), path));
            } else if mode == 0o160000 {
                continue; // gitlink: submodules are out of scope.
            } else {
                let blob = odb.read(&e.oid).map_err(|_| {
                    CommandError::fatal(format!("fatal: unable to read tree ({oid})"))
                })?;
                out.insert(path, BlobInfo { mode, oid: e.oid.clone(), data: blob.data });
            }
        }
    }
    Ok(out)
}

/// Read the index, or an empty one when no index file exists yet.
pub(crate) fn read_index_or_empty(repo: &git_core::Repository) -> Result<Index, CommandError> {
    match Index::read(&repo.index_file(), repo.hash_algo) {
        Ok(ix) => Ok(ix),
        Err(git_index::IndexError::Io(_)) => Ok(Index::default()),
        Err(e) => Err(CommandError::fatal(format!("fatal: {e}"))),
    }
}

/// True when a merge (or am/rebase/cherry-pick/revert) is in progress.
pub(crate) fn merge_in_progress(git_dir: &Path) -> bool {
    git_dir.join("MERGE_HEAD").exists()
}

/// Operation-in-progress state for `switch`'s
/// `die_if_some_operation_in_progress`.
pub(crate) enum OpInProgress {
    None,
    Merge,
    Am,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
}

pub(crate) fn operation_in_progress(git_dir: &Path) -> OpInProgress {
    if git_dir.join("MERGE_HEAD").exists() {
        OpInProgress::Merge
    } else if git_dir.join("rebase-merge").is_dir() || git_dir.join("rebase-apply").is_dir() {
        OpInProgress::Rebase
    } else if git_dir.join("CHERRY_PICK_HEAD").exists() {
        OpInProgress::CherryPick
    } else if git_dir.join("REVERT_HEAD").exists() {
        OpInProgress::Revert
    } else if git_dir.join("BISECT_LOG").exists() {
        OpInProgress::Bisect
    } else {
        OpInProgress::None
    }
}

/// Write `HEAD` as a symref, atomically (temp file + rename).
pub(crate) fn write_head_symref(repo: &git_core::Repository, target: &str) -> Result<(), CommandError> {
    write_file_atomic(&repo.git_dir.join("HEAD"), format!("ref: {target}\n").as_bytes())
}

/// Write `HEAD` as a detached oid, atomically.
pub(crate) fn write_head_detached(
    repo: &git_core::Repository,
    oid: &Oid,
) -> Result<(), CommandError> {
    write_file_atomic(&repo.git_dir.join("HEAD"), format!("{oid}\n").as_bytes())
}

fn write_file_atomic(path: &Path, content: &[u8]) -> Result<(), CommandError> {
    let tmp = path.with_extension(format!("lock.{}", std::process::id()));
    std::fs::write(&tmp, content).map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
    std::fs::rename(&tmp, path).map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
    Ok(())
}

/// Update `ORIG_HEAD` to the previous HEAD (or delete it when there was no
/// previous HEAD but a stale `ORIG_HEAD` exists), like `reset_refs()`.
/// Update `ORIG_HEAD` to the previous HEAD (or delete it when there was no
/// previous HEAD but a stale `ORIG_HEAD` exists), like `reset_refs()`.
/// ORIG_HEAD is a pseudo-ref (no `refs/` prefix), so it bypasses
/// `validate_refname` and is written directly to `$GIT_DIR/ORIG_HEAD`.
pub(crate) fn update_orig_head(repo: &git_core::Repository, old_head: Option<Oid>) {
    let path = repo.git_dir.join("ORIG_HEAD");
    match old_head {
        Some(oid) => {
            let tmp = path.with_extension(format!("lock.{}", std::process::id()));
            if std::fs::write(&tmp, format!("{oid}\n")).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
        None => {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Whether ref updates should be logged (`core.logallrefupdates`, default
/// true except in bare repos).
pub(crate) fn log_all_ref_updates(repo: &git_core::Repository) -> bool {
    repo.config.get_bool("core", "logallrefupdates").unwrap_or(!repo.bare)
}

/// Append one line to `logs/<refname>` (creating parent directories).
pub(crate) fn reflog_append(
    repo: &git_core::Repository,
    refname: &str,
    old: &Oid,
    new: &Oid,
    ident: &str,
    message: &str,
) {
    let (who, when) = match ident.find('>') {
        Some(gt) => (&ident[..=gt], ident[gt + 1..].trim()),
        None => (ident, ""),
    };
    let line = format!("{old} {new} {who} {when}\t{message}\n");
    let path = repo.git_dir.join("logs").join(refname);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// The committer ident line for reflog entries.
pub(crate) fn committer_ident(repo: &git_core::Repository) -> Result<String, CommandError> {
    crate::ident::user_ident(repo, false)
}

/// Unique abbreviation (minimum 7 chars) for display, via the shared resolver.
pub(crate) fn short_oid(repo: &git_core::Repository, oid: &Oid) -> String {
    let len = git_revision::Resolver::new(repo)
        .map(|r| r.unique_abbrev_len(oid, 7))
        .unwrap_or(7);
    let hex = oid.to_string();
    hex.chars().take(len.max(7)).collect()
}

/// The subject (first line) of a commit message.
pub(crate) fn commit_subject(odb: &Odb, algo: HashAlgorithm, oid: &Oid) -> String {
    odb.read(oid)
        .ok()
        .and_then(|o| {
            if o.kind != ObjectKind::Commit {
                return None;
            }
            parse_commit(&o.data, algo).ok()
        })
        .map(|c| {
            String::from_utf8_lossy(&c.message)
                .lines()
                .next()
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default()
}

/// Remove the sequencer/merge state files (`MERGE_HEAD`, `MERGE_MSG`,
/// `MERGE_MODE`, `SQUASH_MSG`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`), like
/// C's `remove_branch_state()`.
pub(crate) fn remove_branch_state(git_dir: &Path) {
    for name in [
        "MERGE_HEAD",
        "MERGE_MSG",
        "MERGE_MODE",
        "SQUASH_MSG",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
    ] {
        let _ = std::fs::remove_file(git_dir.join(name));
    }
}

// ---------------------------------------------------------------------------
// Two-way safety checks (unpack-trees `verify_uptodate`).
// ---------------------------------------------------------------------------

/// Worktree state of a single path.
enum WtState {
    Missing,
    File(Oid, u32),
    Dir,
}

fn wt_state(
    work_tree: &Path,
    rel: &str,
    algo: HashAlgorithm,
    filemode: bool,
) -> WtState {
    match std::fs::symlink_metadata(work_tree.join(rel)) {
        Err(_) => WtState::Missing,
        Ok(md) => {
            if md.file_type().is_dir() {
                WtState::Dir
            } else {
                match crate::worktree::worktree_blob(work_tree, rel, algo, filemode) {
                    Some((oid, mode)) => WtState::File(oid, mode),
                    None => WtState::Missing,
                }
            }
        }
    }
}

/// Check that updating the worktree from `old` to `new` is safe.
///
/// Returns `(local_changes, untracked)` path lists (sorted, deduped). The
/// caller renders C's two error texts and exits 1 when either is non-empty.
pub(crate) fn verify_uptodate(
    work_tree: &Path,
    old: &TreeMap,
    new: &TreeMap,
    algo: HashAlgorithm,
    filemode: bool,
) -> (Vec<String>, Vec<String>) {
    let mut local = Vec::new();
    let mut untracked = Vec::new();
    let mut paths: HashSet<&String> = HashSet::new();
    paths.extend(old.keys());
    paths.extend(new.keys());
    let mut paths: Vec<&String> = paths.into_iter().collect();
    paths.sort();

    for path in paths {
        let o = old.get(path);
        let n = new.get(path);
        let same = match (o, n) {
            (Some(a), Some(b)) => a.oid == b.oid && a.mode == b.mode,
            (None, None) => true,
            _ => false,
        };
        if same {
            continue;
        }
        match wt_state(work_tree, path, algo, filemode) {
            WtState::Missing => {}
            WtState::File(oid, mode) => {
                match o {
                    Some(old_blob) if old_blob.oid == oid && old_blob.mode == mode => {}
                    Some(_) => local.push(path.clone()),
                    None => untracked.push(path.clone()),
                }
            }
            WtState::Dir => {
                // A directory blocks a file write (or a file removal that
                // must replace it). It is clean only when every file under
                // it matches `old` (or is absent from both trees).
                if dir_is_clean(work_tree, path, old, algo, filemode) {
                    // Clean: C removes the directory contents as part of
                    // the update. Untracked leftovers still block.
                    if dir_has_untracked(work_tree, path, old) {
                        untracked.push(path.clone());
                    }
                } else if o.is_none() {
                    untracked.push(path.clone());
                } else {
                    local.push(path.clone());
                }
            }
        }
    }
    local.sort();
    local.dedup();
    untracked.sort();
    untracked.dedup();
    (local, untracked)
}

/// Every tracked file under directory `dir` matches `old` (recursively).
/// Untracked files are ignored here (reported separately).
fn dir_is_clean(
    work_tree: &Path,
    dir: &str,
    old: &TreeMap,
    algo: HashAlgorithm,
    filemode: bool,
) -> bool {
    let mut stack = vec![work_tree.join(dir)];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && !p.is_symlink() {
                stack.push(p);
                continue;
            }
            let rel = match p.strip_prefix(work_tree) {
                Ok(r) => r.to_string_lossy().into_owned(),
                Err(_) => continue,
            };
            if let Some(b) = old.get(&rel) {
                match crate::worktree::worktree_blob(work_tree, &rel, algo, filemode) {
                    Some((oid, mode)) if oid == b.oid && mode == b.mode => {}
                    _ => return false,
                }
            }
        }
    }
    true
}

/// Whether anything under `dir` exists in the worktree but not in `old`.
fn dir_has_untracked(work_tree: &Path, dir: &str, old: &TreeMap) -> bool {
    let mut stack = vec![work_tree.join(dir)];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && !p.is_symlink() {
                stack.push(p);
                continue;
            }
            let rel = match p.strip_prefix(work_tree) {
                Ok(r) => r.to_string_lossy().into_owned(),
                Err(_) => continue,
            };
            if !old.contains_key(&rel) {
                return true;
            }
        }
    }
    false
}

/// Render C's two checkout-safety errors (both to stderr, exit 1).
pub(crate) fn checkout_safety_error(local: &[String], untracked: &[String]) -> CommandError {
    let mut msg = String::new();
    if !local.is_empty() {
        msg.push_str("error: Your local changes to the following files would be overwritten by checkout:\n");
        for p in local {
            msg.push_str(&format!("\t{p}\n"));
        }
        msg.push_str("Please commit your changes or stash them before you switch branches.\nAborting");
    }
    if !untracked.is_empty() {
        if !msg.is_empty() {
            msg.push('\n');
        }
        msg.push_str(
            "error: The following untracked working tree files would be overwritten by checkout:\n",
        );
        for p in untracked {
            msg.push_str(&format!("\t{p}\n"));
        }
        msg.push_str("Please move or remove them before you switch branches.\nAborting");
    }
    CommandError::error(msg)
}

// ---------------------------------------------------------------------------
// Worktree write-out.
// ---------------------------------------------------------------------------

/// Write `new_map` to the worktree: create/update files from `old_map`,
/// remove paths absent from `new_map`, and prune newly-empty directories.
/// (`verify_uptodate` must have passed first, unless forced.)
///
/// Returns the number of files written (for "Updated N paths" reporting).
pub(crate) fn apply_tree_to_worktree(
    repo: &git_core::Repository,
    new_map: &TreeMap,
    old_map: &TreeMap,
) -> Result<usize, CommandError> {
    apply_tree_to_worktree_inner(repo, new_map, old_map, false)
}

/// Like [`apply_tree_to_worktree`], but force-write every target entry
/// (used by `--hard` reset, where the "old" map is the index which may
/// already match the target and would otherwise be skipped).
pub(crate) fn apply_tree_to_worktree_forced(
    repo: &git_core::Repository,
    new_map: &TreeMap,
    old_map: &TreeMap,
) -> Result<usize, CommandError> {
    apply_tree_to_worktree_inner(repo, new_map, old_map, true)
}

fn apply_tree_to_worktree_inner(
    repo: &git_core::Repository,
    new_map: &TreeMap,
    old_map: &TreeMap,
    force: bool,
) -> Result<usize, CommandError> {
    let work_tree = repo
        .work_tree
        .clone()
        .ok_or_else(|| CommandError::fatal("fatal: this operation must be run in a work tree"))?;
    let filemode = repo.config.get_bool("core", "filemode").unwrap_or(true);
    let symlinks = repo.config.get_bool("core", "symlinks").unwrap_or(true);
    let mut written = 0usize;

    // Remove paths that disappear (files, symlinks, or whole directories).
    let mut removals: Vec<&String> = old_map.keys().filter(|p| !new_map.contains_key(*p)).collect();
    removals.sort();
    for path in removals {
        remove_worktree_path(&work_tree, path);
    }

    // Create/update files.
    let mut creations: Vec<&String> = new_map
        .keys()
        .filter(|p| {
            if force {
                return true;
            }
            match old_map.get(*p) {
                Some(o) => {
                    let n = &new_map[*p];
                    o.oid != n.oid || o.mode != n.mode
                }
                None => true,
            }
        })
        .collect();
    creations.sort();
    for path in creations {
        let blob = &new_map[path];
        write_worktree_file(&work_tree, path, blob, filemode, symlinks)?;
        written += 1;
    }

    prune_empty_dirs(&work_tree);
    Ok(written)
}

/// Remove one worktree path (file, symlink, or directory tree).
fn remove_worktree_path(work_tree: &Path, rel: &str) {
    let full = work_tree.join(rel);
    match std::fs::symlink_metadata(&full) {
        Ok(md) if md.file_type().is_dir() && !md.file_type().is_symlink() => {
            let _ = std::fs::remove_dir_all(&full);
        }
        Ok(_) => {
            let _ = std::fs::remove_file(&full);
        }
        Err(_) => {}
    }
}

/// Write one blob to the worktree, creating parent directories.
pub(crate) fn write_one_to_worktree(
    work_tree: &Path,
    rel: &str,
    blob: &BlobInfo,
    filemode: bool,
    symlinks: bool,
) -> Result<(), CommandError> {
    write_worktree_file(work_tree, rel, blob, filemode, symlinks)
}

/// Write one blob to the worktree, creating parent directories.
fn write_worktree_file(
    work_tree: &Path,
    rel: &str,
    blob: &BlobInfo,
    filemode: bool,
    symlinks: bool,
) -> Result<(), CommandError> {
    let full = work_tree.join(rel);
    if let Some(parent) = full.parent() {
        // A blocking file/symlink where a directory is needed is replaced.
        match std::fs::symlink_metadata(parent) {
            Ok(md) if !md.file_type().is_dir() => {
                let _ = std::fs::remove_file(parent);
            }
            _ => {}
        }
        std::fs::create_dir_all(parent)
            .map_err(|e| CommandError::fatal(format!("fatal: unable to create directory '{}': {e}", parent.display())))?;
    }
    // A blocking directory where a file goes is removed (the safety check
    // already ensured it holds nothing precious).
    if let Ok(md) = std::fs::symlink_metadata(&full) {
        if md.file_type().is_dir() && !md.file_type().is_symlink() {
            let _ = std::fs::remove_dir_all(&full);
        } else {
            let _ = std::fs::remove_file(&full);
        }
    }
    if blob.mode == 0o120000 && symlinks {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            use std::os::unix::fs::symlink;
            symlink(std::ffi::OsStr::from_bytes(&blob.data), &full)
                .map_err(|e| {
                    CommandError::fatal(format!("fatal: unable to create symlink '{rel}': {e}"))
                })?;
            return Ok(());
        }
    }
    std::fs::write(&full, &blob.data)
        .map_err(|e| CommandError::fatal(format!("fatal: unable to write file '{rel}': {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let exec = filemode && blob.mode == 0o100755;
        let mode = if exec { 0o755 } else { 0o644 };
        if let Ok(md) = std::fs::symlink_metadata(&full) {
            if !md.file_type().is_symlink() {
                let cur = md.permissions().mode() & !0o777;
                let _ =
                    std::fs::set_permissions(&full, std::fs::Permissions::from_mode(cur | mode));
            }
        }
    }
    Ok(())
}

/// Remove newly-empty directories under the worktree (never the root, never
/// `.git`).
pub(crate) fn prune_empty_dirs(work_tree: &Path) {
    let mut dirs = Vec::new();
    let mut stack = vec![work_tree.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && !p.is_symlink() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name == ".git" {
                    continue;
                }
                stack.push(p.clone());
                dirs.push(p);
            }
        }
    }
    dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for d in dirs {
        let _ = std::fs::remove_dir(d);
    }
}

// ---------------------------------------------------------------------------
// Index rebuild helpers.
// ---------------------------------------------------------------------------

/// Build stage-0 index entries from a tree map with zeroed stat data.
pub(crate) fn bare_entries(new_map: &TreeMap) -> Vec<IndexEntry> {
    new_map
        .iter()
        .map(|(path, b)| IndexEntry::bare(b.oid, b.mode, path.clone()))
        .collect()
}

/// Fill stat fields for entries whose worktree content matches (C's
/// `refresh_index` semantics); others keep zeroed stat.
pub(crate) fn refresh_stat(
    work_tree: &Path,
    entries: &mut [IndexEntry],
    algo: HashAlgorithm,
    filemode: bool,
) {
    for e in entries.iter_mut() {
        match crate::worktree::worktree_blob(work_tree, &e.name, algo, filemode) {
            Some((oid, mode)) if oid == e.oid && mode == e.mode => {
                if let Ok(md) = std::fs::symlink_metadata(work_tree.join(&e.name)) {
                    fill_stat(e, &md);
                }
            }
            _ => {}
        }
    }
}

/// Fill stat fields of a single entry from its worktree metadata.
pub(crate) fn fill_stat(e: &mut IndexEntry, md: &std::fs::Metadata) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        e.ctime_sec = md.ctime() as u32;
        e.ctime_nsec = md.ctime_nsec() as u32;
        e.mtime_sec = md.mtime() as u32;
        e.mtime_nsec = md.mtime_nsec() as u32;
        e.dev = md.dev() as u32;
        e.ino = md.ino() as u32;
        e.uid = md.uid();
        e.gid = md.gid();
        e.size = md.size() as u32;
    }
    #[cfg(not(unix))]
    {
        let _ = (e, md);
    }
}

/// Rebuild the index from `new_map`, preserving nothing. `refresh` fills
/// stat where the worktree matches (mixed-reset semantics); otherwise stat
/// is filled for freshly written files by the caller.
pub(crate) fn rebuild_index(
    repo: &git_core::Repository,
    new_map: &TreeMap,
    refresh: bool,
) -> Result<Index, CommandError> {
    let algo = repo.hash_algo;
    let filemode = repo.config.get_bool("core", "filemode").unwrap_or(true);
    let mut entries = bare_entries(new_map);
    if refresh {
        if let Some(wt) = &repo.work_tree {
            refresh_stat(wt, &mut entries, algo, filemode);
        }
    }
    let triples: Vec<(u32, Oid, String)> =
        entries.iter().map(|e| (e.mode, e.oid, e.name.clone())).collect();
    let cache_tree = crate::treeobj::cache_tree_from_entries(&triples, algo)
        .map_err(CommandError::from)?;
    Ok(Index { version: 2, entries, cache_tree: Some(cache_tree) })
}

/// Write the index back to its file.
pub(crate) fn write_index(repo: &git_core::Repository, index: &Index) -> Result<(), CommandError> {
    index
        .write(&repo.index_file(), repo.hash_algo)
        .map_err(|e| CommandError::fatal(format!("fatal: {e}")))
}

/// The `(mode, oid)` view of stage-0 index entries.
pub(crate) fn index_tree_view(index: &Index) -> BTreeMap<String, (u32, Oid)> {
    index
        .entries
        .iter()
        .filter(|e| e.stage == 0)
        .map(|e| (e.name.clone(), (e.mode, e.oid)))
        .collect()
}

/// Whether the index has any unmerged (non-zero stage) entries.
pub(crate) fn has_unmerged(index: &Index) -> bool {
    index.entries.iter().any(|e| e.stage != 0)
}

// ---------------------------------------------------------------------------
// Ahead/behind for branch tracking output.
// ---------------------------------------------------------------------------

/// Count commits reachable from `a` but not `b` by walking parents.
fn count_only(odb: &Odb, algo: HashAlgorithm, a: &Oid, exclude: &HashSet<Oid>) -> usize {
    let mut seen = HashSet::new();
    let mut stack = vec![*a];
    let mut n = 0usize;
    while let Some(oid) = stack.pop() {
        if !seen.insert(oid) || exclude.contains(&oid) {
            continue;
        }
        n += 1;
        if let Ok(o) = odb.read(&oid) {
            if o.kind == ObjectKind::Commit {
                if let Ok(c) = parse_commit(&o.data, algo) {
                    stack.extend(c.parents);
                }
            }
        }
    }
    n
}

/// Reachable set from `roots` (commit walk).
fn reachable(odb: &Odb, algo: HashAlgorithm, roots: &[Oid]) -> HashSet<Oid> {
    let mut seen = HashSet::new();
    let mut stack: Vec<Oid> = roots.to_vec();
    while let Some(oid) = stack.pop() {
        if !seen.insert(oid) {
            continue;
        }
        if let Ok(o) = odb.read(&oid) {
            if o.kind == ObjectKind::Commit {
                if let Ok(c) = parse_commit(&o.data, algo) {
                    stack.extend(c.parents);
                }
            }
        }
    }
    seen
}

/// Ahead/behind of `branch_oid` relative to `upstream_oid`.
/// Returns `(ahead, behind)`.
pub(crate) fn ahead_behind(
    odb: &Odb,
    algo: HashAlgorithm,
    branch_oid: &Oid,
    upstream_oid: &Oid,
) -> (usize, usize) {
    if branch_oid == upstream_oid {
        return (0, 0);
    }
    let up_reachable = reachable(odb, algo, &[*upstream_oid]);
    let ahead = count_only(odb, algo, branch_oid, &up_reachable);
    let br_reachable = reachable(odb, algo, &[*branch_oid]);
    let behind = count_only(odb, algo, upstream_oid, &br_reachable);
    (ahead, behind)
}

/// `report_tracking` output for a branch with configured upstream, or `None`
/// when there is nothing to report. Written to stdout by the caller.
pub(crate) fn tracking_status(
    repo: &git_core::Repository,
    odb: &Odb,
    branch: &str,
) -> Option<String> {
    let remote = repo.config.get_in("branch", Some(branch), "remote")?;
    let merge = repo.config.get_in("branch", Some(branch), "merge")?;
    // Resolve the upstream: remote "." means local tracking branch.
    let upstream_name = if remote == "." {
        merge.strip_prefix("refs/heads/").unwrap_or(&merge).to_string()
    } else {
        // Remote-tracking ref: refs/remotes/<remote>/<branch-part>.
        let short = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
        format!("{remote}/{short}")
    };
    let store = git_refs::RefStore::from_repo(repo);
    let branch_oid = store.resolve(&format!("refs/heads/{branch}"))?;
    let upstream_oid = store
        .resolve(&format!("refs/remotes/{upstream_name}"))
        .or_else(|| {
            if remote == "." {
                store.resolve(&merge)
            } else {
                None
            }
        })?;
    let display = if remote == "." {
        merge.strip_prefix("refs/heads/").unwrap_or(&merge).to_string()
    } else {
        upstream_name
    };
    let (ahead, behind) = ahead_behind(odb, repo.hash_algo, &branch_oid, &upstream_oid);
    let out = match (ahead, behind) {
        (0, 0) => format!("Your branch is up to date with '{display}'."),
        (a, 0) => {
            let s = if a == 1 { "" } else { "s" };
            format!(
                "Your branch is ahead of '{display}' by {a} commit{s}.\n  (use \"git push\" to publish your local commits)"
            )
        }
        (0, b) => {
            let s = if b == 1 { "" } else { "s" };
            format!(
                "Your branch is behind '{display}' by {b} commit{s}, and can be fast-forwarded.\n  (use \"git pull\" to update your local branch)"
            )
        }
        (a, b) => format!(
            "Your branch and '{display}' have diverged,\nand have {a} and {b} different commits each, respectively.\n  (use \"git pull\" to merge the remote branch into yours)"
        ),
    };
    Some(out)
}

/// Read `$GIT_REFLOG_ACTION` (used as the reflog message verbatim when set).
pub(crate) fn reflog_action(default_msg: String) -> String {
    match std::env::var("GIT_REFLOG_ACTION") {
        Ok(a) if !a.is_empty() => a,
        _ => default_msg,
    }
}

/// Map a pathspec operand (CLI form, relative to the invoking cwd) to a
/// repo-relative literal, like `add`'s resolver (no globs: checkout/reset
/// pathspecs are matched literally with directory-prefix semantics).
pub(crate) fn resolve_path_arg(ctx: &RepoContext, repo: &git_core::Repository, op: &str) -> String {
    let cwd_rel = ctx
        .cwd
        .canonicalize()
        .ok()
        .and_then(|c| {
            repo.work_tree
                .as_ref()
                .and_then(|wt| c.strip_prefix(wt).ok().map(|p| p.to_string_lossy().into_owned()))
        })
        .unwrap_or_default();
    let joined = if Path::new(op).is_absolute() {
        repo.work_tree
            .as_ref()
            .and_then(|wt| Path::new(op).strip_prefix(wt).ok().map(|p| p.to_string_lossy().into_owned()))
            .unwrap_or_else(|| op.to_string())
    } else if cwd_rel.is_empty() {
        op.to_string()
    } else {
        format!("{cwd_rel}/{op}")
    };
    // Lexical normalization (a/./b, a/../b, trailing /).
    let mut parts: Vec<&str> = Vec::new();
    for comp in Path::new(&joined).components() {
        match comp {
            std::path::Component::Normal(c) => parts.push(c.to_str().unwrap_or("")),
            std::path::Component::ParentDir => {
                parts.pop();
            }
            _ => {}
        }
    }
    parts.join("/")
}

/// Does repo-relative path `path` fall under pathspec `spec` (literal file
/// or directory prefix)?
pub(crate) fn spec_matches(spec: &str, path: &str) -> bool {
    path == spec || path.starts_with(&format!("{spec}/"))
}

/// Validate a new branch name and return its full refname.
pub(crate) fn new_branch_ref(name: &str) -> Result<String, CommandError> {
    let full = format!("refs/heads/{name}");
    git_refs::validate_refname(&full)
        .map_err(|_| CommandError::fatal(format!("fatal: '{name}' is not a valid branch name")))?;
    Ok(full)
}

/// The previous branch from `logs/HEAD` ("checkout: moving from X to Y" —
/// returns X of the newest such entry), for `checkout -` / `switch -`.
pub(crate) fn previous_branch(repo: &git_core::Repository) -> Option<String> {
    let data = std::fs::read(repo.git_dir.join("logs/HEAD")).ok()?;
    let text = String::from_utf8_lossy(&data);
    for line in text.lines().rev() {
        if let Some(idx) = line.find("checkout: moving from ") {
            let rest = &line[idx + "checkout: moving from ".len()..];
            if let Some(to) = rest.find(" to ") {
                let from = rest[..to].trim().to_string();
                if !from.is_empty() {
                    return Some(from);
                }
            }
        }
    }
    None
}

/// Look up a local branch (short or full name) to its full refname + tip.
pub(crate) fn lookup_local_branch(
    store: &git_refs::RefStore,
    name: &str,
) -> Option<(String, Oid)> {
    let full = if let Some(short) = name.strip_prefix("refs/heads/") {
        format!("refs/heads/{short}")
    } else {
        format!("refs/heads/{name}")
    };
    store.resolve(&full).map(|oid| (full, oid))
}
