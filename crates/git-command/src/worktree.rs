//! Shared worktree inspection used by `status` and `commit`: hashing worktree
//! files, detecting unstaged changes, and listing untracked/ignored paths.

use std::collections::HashSet;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use git_core::Repository;
use git_hash::{HashAlgorithm, Oid};
use git_index::Index;
use git_object::{Object, ObjectKind};

/// Hash a worktree file (following git's symlink/mode rules); `None` if absent
/// or not a regular file/symlink.
pub(crate) fn worktree_blob(
    work_tree: &Path,
    rel: &str,
    algo: HashAlgorithm,
    filemode: bool,
) -> Option<(Oid, u32)> {
    let full = work_tree.join(rel);
    let md = std::fs::symlink_metadata(&full).ok()?;
    let ft = md.file_type();
    if ft.is_symlink() {
        let t = std::fs::read_link(&full).ok()?;
        let oid = Object::from_data(ObjectKind::Blob, t.to_string_lossy().into_owned().into_bytes())
            .compute_id(algo);
        Some((oid, 0o120000))
    } else if ft.is_file() {
        let data = std::fs::read(&full).ok()?;
        let mode = if filemode && (md.permissions().mode() & 0o111) != 0 { 0o100755 } else { 0o100644 };
        Some((Object::from_data(ObjectKind::Blob, data).compute_id(algo), mode))
    } else {
        None
    }
}

/// Tracked paths whose worktree content differs from the index (or is gone):
/// `'M'` modified, `'D'` deleted; sorted by path. Unmerged (stage > 0) entries
/// are reported as `'U'`.
pub(crate) fn unstaged_changes(
    work_tree: &Path,
    index: &Index,
    algo: HashAlgorithm,
    filemode: bool,
) -> Vec<(char, String)> {
    let mut out = Vec::new();
    for e in &index.entries {
        if e.stage != 0 {
            out.push(('U', e.name.clone()));
            continue;
        }
        match worktree_blob(work_tree, &e.name, algo, filemode) {
            None => out.push(('D', e.name.clone())),
            Some((oid, mode)) => {
                if oid != e.oid || mode != e.mode {
                    out.push(('M', e.name.clone()));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Walk the worktree, returning `(untracked, ignored)` path lists. Untracked
/// directories are collapsed to `dir/`. `ignored` is only populated when
/// `include_ignored` is set (traditional mode: ignored directories are
/// collapsed).
pub(crate) fn untracked_and_ignored(
    repo: &Repository,
    index: &Index,
    include_ignored: bool,
) -> (Vec<String>, Vec<String>) {
    let work_tree = match repo.work_tree.clone() {
        Some(w) => w,
        None => return (Vec::new(), Vec::new()),
    };
    let tracked: HashSet<String> = index.entries.iter().map(|e| e.name.clone()).collect();
    let mut engine = crate::ignore_util::build_engine(repo);
    let mut untracked = Vec::new();
    let mut ignored = Vec::new();
    walk(
        &work_tree,
        "",
        &tracked,
        &mut engine,
        include_ignored,
        &mut untracked,
        &mut ignored,
        false,
    );
    untracked.sort();
    ignored.sort();
    (untracked, ignored)
}

#[allow(clippy::too_many_arguments)]
fn walk(
    dir: &Path,
    prefix: &str,
    tracked: &HashSet<String>,
    engine: &mut git_attributes::ignore::IgnoreEngine,
    include_ignored: bool,
    untracked: &mut Vec<String>,
    ignored: &mut Vec<String>,
    ancestor_ignored: bool,
) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let Ok(md) = std::fs::symlink_metadata(e.path()) else { continue };
        let is_dir = md.is_dir();
        let ignored_here = ancestor_ignored || engine.is_excluded(&rel, is_dir).is_some();
        if ignored_here {
            if include_ignored {
                ignored.push(if is_dir { format!("{rel}/") } else { rel });
            }
            continue;
        }
        if is_dir {
            let has_tracked = tracked.iter().any(|t| t.starts_with(&format!("{rel}/")));
            if has_tracked {
                walk(&e.path(), &rel, tracked, engine, include_ignored, untracked, ignored, false);
            } else {
                untracked.push(format!("{rel}/"));
            }
        } else if !tracked.contains(&rel) {
            untracked.push(rel);
        }
    }
}
