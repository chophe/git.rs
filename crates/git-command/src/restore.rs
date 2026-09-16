//! `git restore`: restore worktree files and/or index entries.
//!
//! Port of `builtin/checkout.c` (`cmd_restore`): `--source`, `--staged` /
//! `--worktree`, `--overlay` / `--no-overlay` (default off), `--quiet`,
//! `--ignore-unmerged`, `--ours` / `--theirs`. Deferred with explicit
//! errors: `-p` / `--patch`, `--merge`, `--conflict`.

use std::collections::{HashMap, HashSet};
use std::io::Write;

use crate::checkout_core::{self, read_index_or_empty, write_index};
use crate::{Command, CommandError, RepoContext};
use git_odb::Odb;

pub struct Restore;

impl Command for Restore {
    fn name(&self) -> &'static str {
        "restore"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], _out: &mut dyn Write) -> Result<(), CommandError> {
        let mut source: Option<String> = None;
        // -1 default-off, -2 default-on (C's OPT_BOOL triple-state); plain
        // bools here with explicit defaults applied below.
        let mut staged = false;
        let mut staged_given = false;
        let mut worktree = false;
        let mut worktree_given = false;
        let mut overlay = false; // restore defaults to no-overlay.
        let mut quiet = false;
        let mut patch = false;
        let mut merge = false;
        let mut conflict: Option<String> = None;
        let mut ours = false;
        let mut theirs = false;
        let mut ignore_unmerged = false;
        let mut paths: Vec<String> = Vec::new();

        let mut i = 0usize;
        while i < args.len() {
            let a = &args[i];
            match a.as_str() {
                "-s" => {
                    i += 1;
                    source = Some(
                        args.get(i)
                            .ok_or_else(|| {
                                CommandError::usage("error: option `source' requires a value")
                            })?
                            .clone(),
                    );
                }
                "-S" | "--staged" => {
                    staged = true;
                    staged_given = true;
                }
                "-W" | "--worktree" => {
                    worktree = true;
                    worktree_given = true;
                }
                "--overlay" => overlay = true,
                "--no-overlay" => overlay = false,
                "-q" | "--quiet" => quiet = true,
                "-p" | "--patch" => patch = true,
                "--merge" => merge = true,
                "--ours" => ours = true,
                "--theirs" => theirs = true,
                "--ignore-unmerged" => ignore_unmerged = true,
                s if s.starts_with("--source=") => {
                    source = Some(s["--source=".len()..].to_string())
                }
                "--source" => {
                    i += 1;
                    source = Some(
                        args.get(i)
                            .ok_or_else(|| {
                                CommandError::usage("error: option `source' requires a value")
                            })?
                            .clone(),
                    );
                }
                s if s.starts_with("--conflict=") => {
                    conflict = Some(s["--conflict=".len()..].to_string())
                }
                "--conflict" => {
                    i += 1;
                    conflict = Some(
                        args.get(i)
                            .ok_or_else(|| {
                                CommandError::usage("error: option `conflict' requires a value")
                            })?
                            .clone(),
                    );
                }
                "--" => {
                    paths.extend(args[i + 1..].iter().cloned());
                    break;
                }
                "--staged=" | "--worktree=" => {
                    return Err(CommandError::usage(format!(
                        "error: unknown option `{a}'\nusage: git restore [<options>] [--source=<branch>] <file>..."
                    )));
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    // Combined shorts (-sW) are not supported by C either
                    // (only single-char bundling for bools); report unknown.
                    if s.starts_with("--") {
                        let name = s[2..].split('=').next().unwrap_or("");
                        return Err(CommandError::usage(format!(
                            "error: unknown option `{name}'\nusage: git restore [<options>] [--source=<branch>] <file>..."
                        )));
                    }
                    return Err(CommandError::usage(format!(
                        "error: unknown switch `{c}'\nusage: git restore [<options>] [--source=<branch>] <file>...",
                        c = s.chars().nth(1).unwrap_or('?')
                    )));
                }
                s => paths.push(s.to_string()),
            }
            i += 1;
        }

        if patch {
            return Err(CommandError::fatal("fatal: restore: --patch is not supported yet"));
        }
        if merge {
            return Err(CommandError::fatal("fatal: restore: --merge is not supported yet"));
        }
        if conflict.is_some() {
            return Err(CommandError::fatal("fatal: restore: --conflict is not supported yet"));
        }
        if ours && theirs {
            // C keeps a single writeout_stage; last option wins. Parse order
            // above already gives that (theirs checked second below).
        }
        if staged && (ours || theirs) {
            return Err(CommandError::fatal(
                "fatal: '--ours' or '--theirs' cannot be used with --staged",
            ));
        }
        // Defaults: worktree on unless only --staged was given... precisely:
        // both default (staged off, worktree on); explicit --staged and/or
        // --worktree widen accordingly. `--staged` alone keeps worktree off.
        if !staged_given && !worktree_given {
            worktree = true;
        }
        if staged && !worktree_given && source.is_none() {
            // (worktree stays off)
        }
        if paths.is_empty() {
            return Err(CommandError::fatal("fatal: you must specify path(s) to restore"));
        }

        let repo = ctx.repository()?;
        let algo = repo.hash_algo;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;

        // "restore --staged [--worktree]" without --source reads from HEAD.
        let effective_source: Option<String> = match (&source, staged) {
            (None, true) => Some("HEAD".to_string()),
            (s, _) => s.clone(),
        };
        let source_map: Option<checkout_core::TreeMap> = match effective_source {
            None => None,
            Some(s) => {
                let rev = if s == "@" { "HEAD".to_string() } else { s.clone() };
                let oid = crate::resolve_arg(&repo, &rev)
                    .map_err(|_| CommandError::fatal(format!("fatal: could not resolve {s}")))?;
                let tree_oid =
                    checkout_core::commit_or_tree_to_tree(&odb, algo, &oid).map_err(|_| {
                        CommandError::fatal(format!("fatal: reference is not a tree: {s}"))
                    })?;
                Some(checkout_core::tree_to_map(&odb, algo, &tree_oid)?)
            }
        };

        let resolved_paths: Vec<String> = paths
            .iter()
            .map(|p| checkout_core::resolve_path_arg(ctx, &repo, p))
            .collect();
        let matched = |path: &str| {
            resolved_paths.iter().any(|s| checkout_core::spec_matches(s, path))
        };

        let mut index = read_index_or_empty(&repo)?;

        // Every pathspec must match (index, worktree, or source).
        for (orig, spec) in paths.iter().zip(resolved_paths.iter()) {
            let hit = index.entries.iter().any(|e| checkout_core::spec_matches(spec, &e.name))
                || source_map
                    .as_ref()
                    .is_some_and(|t| t.keys().any(|k| checkout_core::spec_matches(spec, k)))
                || repo
                    .work_tree
                    .as_ref()
                    .is_some_and(|wt| std::fs::symlink_metadata(wt.join(spec)).is_ok());
            if !hit {
                return Err(CommandError::error(format!(
                    "error: pathspec '{orig}' did not match any file(s) known to git"
                )));
            }
        }

        // Unmerged handling.
        let mut unmerged_names: Vec<String> = Vec::new();
        for e in index.entries.iter().filter(|e| e.stage != 0 && matched(&e.name)) {
            if !unmerged_names.contains(&e.name) {
                unmerged_names.push(e.name.clone());
            }
        }
        let writeout = ours || theirs;
        if !unmerged_names.is_empty() {
            if ignore_unmerged {
                if !quiet {
                    for n in &unmerged_names {
                        eprintln!("warning: path '{n}' is unmerged");
                    }
                }
            } else if writeout {
                // --ours/--theirs resolve below.
            } else if staged {
                for n in &unmerged_names {
                    eprintln!("error: path '{n}' is unmerged");
                }
                return Err(CommandError::silent(1));
            } else if source_map.is_some() {
                // Restoring the worktree from a tree: unmerged entries are
                // left alone (only stage-0 writes happen)... C actually
                // errors here too unless --merge/--ours/--theirs. Match the
                // index-restore rule: error.
                for n in &unmerged_names {
                    eprintln!("error: path '{n}' is unmerged");
                }
                return Err(CommandError::silent(1));
            } else {
                // Worktree from index: same error.
                for n in &unmerged_names {
                    eprintln!("error: path '{n}' is unmerged");
                }
                return Err(CommandError::silent(1));
            }
        }
        let skip_unmerged: HashSet<String> = if ignore_unmerged {
            unmerged_names.iter().cloned().collect()
        } else {
            HashSet::new()
        };

        // --staged: index := source for matched paths (no-overlay by
        // default: entries absent from the source are removed).
        if staged {
            if let Some(src) = &source_map {
                index.entries.retain(|e| {
                    if !matched(&e.name) {
                        return true;
                    }
                    if e.stage != 0 {
                        return true;
                    }
                    overlay && !src.contains_key(&e.name)
                });
                for (path, blob) in src {
                    if matched(path) {
                        // Drop any existing stage-0 entry (unmerged stages
                        // stay unless overwritten... C replaces them via
                        // read_tree_some? For --staged from a tree, unmerged
                        // entries errored above (no --ours/--theirs allowed
                        // with --staged), so only stage-0 can exist here.
                        index.entries.retain(|e| !(e.stage == 0 && e.name == *path));
                        index.entries.push(git_index::IndexEntry::bare(
                            blob.oid,
                            blob.mode,
                            path.clone(),
                        ));
                    }
                }
            }
        }

        // --worktree: write matched files.
        if worktree {
            let work_tree = repo.work_tree.clone().ok_or_else(|| {
                CommandError::fatal("fatal: this operation must be run in a work tree")
            })?;
            let filemode = repo.config.get_bool("core", "filemode").unwrap_or(true);
            let symlinks = repo.config.get_bool("core", "symlinks").unwrap_or(true);
            // Blob source per path: explicit source tree, else the index
            // (post---staged state), else stage for --ours/--theirs.
            let want_stage = if theirs {
                Some(3u8)
            } else if ours {
                Some(2u8)
            } else {
                None
            };
            let mut stages: HashMap<&str, Vec<(u8, u32, git_hash::Oid)>> = HashMap::new();
            for e in index.entries.iter() {
                if e.stage != 0 && matched(&e.name) && !skip_unmerged.contains(&e.name) {
                    stages.entry(e.name.as_str()).or_default().push((e.stage, e.mode, e.oid));
                }
            }
            let mut to_write: Vec<(String, u32, git_hash::Oid)> = Vec::new();
            let mut staged_names: HashSet<&str> = HashSet::new();
            if let Some(want) = want_stage {
                for (name, list) in &stages {
                    if let Some((_, mode, oid)) = list.iter().find(|(s, _, _)| *s == want) {
                        staged_names.insert(*name);
                        to_write.push((name.to_string(), *mode, *oid));
                    }
                }
            }
            // Stage-0 source: explicit tree wins over the index.
            if let Some(src) = &source_map {
                for (path, blob) in src {
                    if matched(path) && !skip_unmerged.contains(path) && !staged_names.contains(path.as_str()) {
                        to_write.push((path.clone(), blob.mode, blob.oid));
                    }
                }
                if !overlay {
                    // No-overlay: remove worktree files under the pathspec
                    // that are absent from the source.
                    let mut victims: Vec<String> = Vec::new();
                    // From the index's perspective...
                    for e in index.entries.iter().filter(|e| e.stage == 0 && matched(&e.name)) {
                        if !src.contains_key(&e.name)
                            && !skip_unmerged.contains(&e.name)
                            && !staged_names.contains(e.name.as_str())
                        {
                            victims.push(e.name.clone());
                        }
                    }
                    // ...and stray worktree files matching the pathspec.
                    collect_strays(&work_tree, &resolved_paths, src, &mut victims);
                    for v in &victims {
                        let full = work_tree.join(v);
                        if std::fs::symlink_metadata(&full)
                            .is_ok_and(|m| !m.file_type().is_dir())
                        {
                            let _ = std::fs::remove_file(&full);
                        }
                    }
                    checkout_core::prune_empty_dirs(&work_tree);
                }
            } else {
                for e in index.entries.iter().filter(|e| {
                    e.stage == 0 && matched(&e.name) && !skip_unmerged.contains(&e.name)
                }) {
                    if !staged_names.contains(e.name.as_str()) {
                        to_write.push((e.name.clone(), e.mode, e.oid));
                    }
                }
            }
            for (name, mode, oid) in &to_write {
                let data = odb.read(oid).map(|o| o.data).unwrap_or_default();
                let blob = checkout_core::BlobInfo { mode: *mode, oid: *oid, data };
                checkout_core::write_one_to_worktree(&work_tree, name, &blob, filemode, symlinks)?;
            }
            // Refresh stat for written entries.
            for e in index.entries.iter_mut() {
                if e.stage == 0 && matched(&e.name) && to_write.iter().any(|(n, _, _)| n == &e.name)
                {
                    if let Ok(md) = std::fs::symlink_metadata(work_tree.join(&e.name)) {
                        checkout_core::fill_stat(e, &md);
                    }
                }
            }
        }

        // Rebuild the cache-tree when the index changed (cheap parity with
        // C, which refreshes it on write).
        if staged {
            let triples: Vec<(u32, git_hash::Oid, String)> =
                index.entries.iter().map(|e| (e.mode, e.oid, e.name.clone())).collect();
            if let Ok(ct) = crate::treeobj::cache_tree_from_entries(&triples, algo) {
                index.cache_tree = Some(ct);
            }
        }
        write_index(&repo, &index)?;
        Ok(())
    }
}

/// Collect worktree files under `specs` that are absent from `src` (for
/// no-overlay worktree deletion).
fn collect_strays(
    work_tree: &std::path::Path,
    specs: &[String],
    src: &checkout_core::TreeMap,
    out: &mut Vec<String>,
) {
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
                stack.push(p);
                continue;
            }
            let rel = match p.strip_prefix(work_tree) {
                Ok(r) => r.to_string_lossy().into_owned(),
                Err(_) => continue,
            };
            if specs.iter().any(|s| checkout_core::spec_matches(s, &rel))
                && !src.contains_key(&rel)
                && !out.contains(&rel)
            {
                out.push(rel);
            }
        }
    }
}
