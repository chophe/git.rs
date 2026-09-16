//! `git reset`: reset the ref, index, and/or worktree.
//!
//! Port of `builtin/reset.c` for `--soft` / `--mixed` (default) / `--hard`
//! plus the paths form (`reset [<tree-ish>] [--] <paths>`). Deferred with
//! explicit errors: `-p`/`--patch`, `--merge`, `--merge`, `-N`
//! (intent-to-add needs index extensions), `--pathspec-from-file`, and the
//! interactive diff options.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::checkout_core::{
    self, bare_entries, commit_or_tree_to_tree, index_tree_view, read_head, read_index_or_empty,
    rebuild_index, refresh_stat, remove_branch_state, update_orig_head, write_index,
};
use crate::{Command, CommandError, RepoContext};
use git_index::{Index, IndexEntry};
use git_object::{parse_commit, ObjectKind};
use git_odb::Odb;

pub struct Reset;

/// Full usage text, matching C's parse-options-generated output.
const FULL_USAGE: &str = "usage: git reset [--mixed | --soft | --hard | --merge | --keep] [-q] [<commit>]\n   or: git reset [-q] [<tree-ish>] [--] <pathspec>...\n   or: git reset [-q] [--pathspec-from-file [--pathspec-file-nul]] [<tree-ish>]\n   or: git reset --patch [<tree-ish>] [--] [<pathspec>...]\n\n    -q, --[no-]quiet      be quiet, only report errors\n    --no-refresh          skip refreshing the index after reset\n    --refresh             opposite of --no-refresh\n    --mixed               reset HEAD and index\n    --soft                reset only HEAD\n    --hard                reset HEAD, index and working tree\n    --merge               reset HEAD, index and working tree\n    --keep                reset HEAD but keep local changes\n    --[no-]recurse-submodules[=<reset>]\n                          control recursive updating of submodules\n    -p, --[no-]patch      select hunks interactively\n    -N, --[no-]intent-to-add\n                          record only the fact that removed paths will be added later\n    --[no-]pathspec-from-file <file>\n                          read pathspec from file\n    --[no-]pathspec-file-nul\n                          with --pathspec-from-file, pathspec elements are separated with NUL character\n";

/// `error: ...` + full usage (exit 129), like C's parse-options errors
/// (which terminate the usage text with a blank line).
fn full_usage_error(first_line: String) -> CommandError {
    CommandError::usage(format!("{first_line}\n{FULL_USAGE}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Mixed,
    Soft,
    Hard,
}

fn mode_name(m: Mode) -> &'static str {
    match m {
        Mode::Mixed => "mixed",
        Mode::Soft => "soft",
        Mode::Hard => "hard",
    }
}

impl Command for Reset {
    fn name(&self) -> &'static str {
        "reset"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut mode = Mode::Mixed;
        let mut mode_given = false;
        let mut quiet = false;
        let mut no_refresh = false;
        let mut patch = false;
        let mut merge_opt = false;
        let mut keep_opt = false;
        let mut intent_to_add = false;
        let mut unified: Option<String> = None;
        let mut interhunk: Option<String> = None;
        let mut no_auto_advance = false;
        let mut operands: Vec<String> = Vec::new();

        let mut i = 0usize;
        while i < args.len() {
            let a = &args[i];
            match a.as_str() {
                "-q" | "--quiet" => quiet = true,
                "--no-refresh" => no_refresh = true,
                "--refresh" => no_refresh = false,
                "--mixed" => {
                    mode = Mode::Mixed;
                    mode_given = true;
                }
                "--soft" => {
                    mode = Mode::Soft;
                    mode_given = true;
                }
                "--hard" => {
                    mode = Mode::Hard;
                    mode_given = true;
                }
                "-p" | "--patch" => patch = true,
                "--merge" => merge_opt = true,
                "--keep" => keep_opt = true,
                "-N" | "--intent-to-add" => intent_to_add = true,
                "--no-auto-advance" => no_auto_advance = true,
                "--auto-advance" => {}
                s if s.starts_with("--unified=") => {
                    unified = Some(s["--unified=".len()..].to_string())
                }
                "--unified" => {
                    i += 1;
                    unified = Some(
                        args.get(i)
                            .ok_or_else(|| {
                                CommandError::usage("error: option `unified' requires a value")
                            })?
                            .clone(),
                    );
                }
                s if s.starts_with("--inter-hunk-context=") => {
                    interhunk = Some(s["--inter-hunk-context=".len()..].to_string())
                }
                "--pathspec-from-file" | "--pathspec-file-nul" => {
                    return Err(CommandError::fatal(format!(
                        "fatal: reset: option '{a}' is not supported yet"
                    )));
                }
                "--" => {
                    // Keep the separator: split_reset_args needs to see it
                    // (C's parse_args distinguishes `<rev> -- <paths>`).
                    operands.extend(args[i..].iter().cloned());
                    break;
                }
                s if s.starts_with("--recurse-submodules") => {
                    // Accepted; submodule recursion is out of scope.
                }
                s if s.starts_with("--") => {
                    let name = &s[2..];
                    return Err(full_usage_error(format!("error: unknown option `{name}'")));
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(full_usage_error(format!(
                        "error: unknown switch `{s}'"
                    )));
                }
                s => operands.push(s.to_string()),
            }
            i += 1;
        }

        // Interactive/diff option validation, like C.
        if patch {
            if mode_given {
                return Err(CommandError::fatal(
                    "fatal: options '--patch' and '--{hard,mixed,soft}' cannot be used together",
                ));
            }
            return Err(CommandError::fatal("fatal: reset: --patch is not supported yet"));
        }
        if unified.is_some() {
            return Err(CommandError::fatal(
                "fatal: the option '--unified' requires '--patch'",
            ));
        }
        if interhunk.is_some() {
            return Err(CommandError::fatal(
                "fatal: the option '--inter-hunk-context' requires '--patch'",
            ));
        }
        if no_auto_advance {
            return Err(CommandError::fatal(
                "fatal: the option '--no-auto-advance' requires '--patch'",
            ));
        }
        if merge_opt {
            return Err(CommandError::fatal("fatal: reset: --merge is not supported yet"));
        }
        if keep_opt {
            return Err(CommandError::fatal("fatal: reset: --merge is not supported yet"));
        }
        if intent_to_add && mode != Mode::Mixed {
            return Err(CommandError::fatal("fatal: the option '-N' requires '--mixed'"));
        }
        if intent_to_add {
            return Err(CommandError::fatal("fatal: reset: --intent-to-add is not supported yet"));
        }

        let repo = ctx.repository()?;
        let algo = repo.hash_algo;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;

        // Split operands into [rev] + pathspec, mirroring C's parse_args.
        let (rev, paths) = split_reset_args(ctx, &repo, &operands)?;

        if !paths.is_empty() {
            if mode_given && mode == Mode::Mixed {
                eprintln!(
                    "warning: --mixed with paths is deprecated; use 'git reset -- <paths>' instead."
                );
            } else if mode != Mode::Mixed {
                return Err(CommandError::fatal(format!(
                    "fatal: Cannot do {} reset with paths.",
                    mode_name(mode)
                )));
            }
            return reset_paths(ctx, &repo, &odb, &rev, &paths, quiet, out);
        }

        // No paths: rev must be a commit (tags peel). "@" is HEAD.
        let rev_name = if rev == "@" { "HEAD".to_string() } else { rev.clone() };
        let head_state = read_head(&repo);
        let unborn = rev_name == "HEAD" && head_state.oid.is_none();
        let target: git_hash::Oid = if unborn {
            // Reset on an unborn branch: reset to the empty tree.
            checkout_core::empty_tree_oid(algo)
        } else {
            let oid = resolve_committish(&repo, &rev_name)?;
            // Must be a commit (tags peel); anything else dies like C.
            match checkout_core::peel_to_commit(&odb, &oid) {
                Ok(c) => c,
                Err((bad, kind)) => {
                    if kind != "bad" {
                        eprintln!("error: object {bad} is a {kind}, not a commit");
                    }
                    return Err(CommandError::fatal(format!(
                        "fatal: Could not parse object '{rev_name}'."
                    )));
                }
            }
        };

        match mode {
            Mode::Soft => reset_soft(&repo, &odb, &rev_name, &target, unborn),
            Mode::Mixed => {
                reset_mixed(&repo, &odb, &rev_name, &target, unborn, quiet, no_refresh, out)
            }
            Mode::Hard => reset_hard(&repo, &odb, &rev_name, &target, unborn, quiet, out),
        }
    }
}

/// Split reset operands into `(rev, paths)`, following C's `parse_args`.
fn split_reset_args(
    ctx: &RepoContext,
    repo: &git_core::Repository,
    operands: &[String],
) -> Result<(String, Vec<String>), CommandError> {
    if let Some(dd) = operands.iter().position(|a| a == "--") {
        let (pre, post) = operands.split_at(dd);
        let post = &post[1..];
        if pre.is_empty() {
            return Ok(("HEAD".to_string(), resolve_paths(ctx, repo, post)));
        }
        if pre.len() == 1 {
            // Explicit `<rev> -- <paths>`: C takes argv[0] as the rev
            // unconditionally (no filename verification).
            return Ok((pre[0].clone(), resolve_paths(ctx, repo, post)));
        }
        // `a b -- c`: rev is `a` when treeish, else everything is paths.
        if resolves_as_treeish(repo, &pre[0]) {
            verify_non_filename(ctx, repo, &pre[0])?;
            let mut paths = resolve_paths(ctx, repo, &pre[1..]);
            paths.extend(resolve_paths(ctx, repo, post));
            return Ok((pre[0].clone(), paths));
        }
        verify_filename(ctx, repo, &pre[0])?;
        let mut paths = resolve_paths(ctx, repo, pre);
        paths.extend(resolve_paths(ctx, repo, post));
        return Ok(("HEAD".to_string(), paths));
    }
    match operands.len() {
        0 => Ok(("HEAD".to_string(), Vec::new())),
        1 => {
            let a = &operands[0];
            if resolves_as_committish(repo, a) {
                verify_non_filename(ctx, repo, a)?;
                Ok((a.clone(), Vec::new()))
            } else {
                verify_filename(ctx, repo, a)?;
                Ok(("HEAD".to_string(), vec![resolve_path(ctx, repo, a)]))
            }
        }
        _ => {
            let a = &operands[0];
            if resolves_as_treeish(repo, a) {
                verify_non_filename(ctx, repo, a)?;
                Ok((a.clone(), resolve_paths(ctx, repo, &operands[1..])))
            } else {
                verify_filename(ctx, repo, a)?;
                Ok(("HEAD".to_string(), resolve_paths(ctx, repo, operands)))
            }
        }
    }
}

/// Does `arg` resolve and denote a commit (tags peel to their target)?
fn resolves_as_committish(repo: &git_core::Repository, arg: &str) -> bool {
    let arg = if arg == "@" { "HEAD" } else { arg };
    let odb = match Odb::from_repo(repo) {
        Ok(o) => o,
        Err(_) => return false,
    };
    let oid = match crate::resolve_arg(repo, arg) {
        Ok(o) => o,
        Err(_) => return false,
    };
    checkout_core::peel_to_commit(&odb, &oid).is_ok()
}

/// Does `arg` resolve to a commit, tag, or tree?
fn resolves_as_treeish(repo: &git_core::Repository, arg: &str) -> bool {
    let arg = if arg == "@" { "HEAD" } else { arg };
    let odb = match Odb::from_repo(repo) {
        Ok(o) => o,
        Err(_) => return false,
    };
    let oid = match crate::resolve_arg(repo, arg) {
        Ok(o) => o,
        Err(_) => return false,
    };
    commit_or_tree_to_tree(&odb, repo.hash_algo, &oid).is_ok()
}

/// C's `verify_non_filename`: a rev that is also a worktree path is an error
/// (skipped outside a work tree).
fn verify_non_filename(
    ctx: &RepoContext,
    repo: &git_core::Repository,
    arg: &str,
) -> Result<(), CommandError> {
    let Some(wt) = &repo.work_tree else { return Ok(()) };
    if arg.starts_with('-') {
        return Ok(());
    }
    if is_glob(arg) {
        return Ok(());
    }
    let full = work_tree_path(ctx, repo, wt, arg);
    if std::fs::symlink_metadata(&full).is_ok() {
        return Err(CommandError::fatal(format!(
            "fatal: ambiguous argument '{arg}': both revision and filename\nUse '--' to separate paths from revisions, like this:\n'git <command> [<revision>...] -- [<file>...]'"
        )));
    }
    Ok(())
}

/// C's `verify_filename`: globs and existing paths are fine, anything else
/// dies with the ambiguous-argument error.
fn verify_filename(
    ctx: &RepoContext,
    repo: &git_core::Repository,
    arg: &str,
) -> Result<(), CommandError> {
    if arg.starts_with('-') {
        return Err(CommandError::fatal(format!(
            "fatal: option '{arg}' must come before non-option arguments"
        )));
    }
    if is_glob(arg) {
        return Ok(());
    }
    // Outside a work tree nothing can be a filename.
    let Some(wt) = &repo.work_tree else {
        return Err(ambiguous_filename(arg));
    };
    let full = work_tree_path(ctx, repo, wt, arg);
    if std::fs::symlink_metadata(&full).is_ok() {
        return Ok(());
    }
    Err(ambiguous_filename(arg))
}

fn ambiguous_filename(arg: &str) -> CommandError {
    CommandError::fatal(format!(
        "fatal: ambiguous argument '{arg}': unknown revision or path not in the working tree.\nUse '--' to separate paths from revisions, like this:\n'git <command> [<revision>...] -- [<file>...]'"
    ))
}

fn is_glob(arg: &str) -> bool {
    if arg.starts_with(":(") {
        return true;
    }
    let mut escaped = false;
    for c in arg.chars() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if "*?[".contains(c) {
            return true;
        }
    }
    false
}

/// Join a CLI path with the invoking cwd (like C's prefix handling).
fn work_tree_path(
    ctx: &RepoContext,
    _repo: &git_core::Repository,
    wt: &PathBuf,
    arg: &str,
) -> PathBuf {
    let p = Path::new(arg);
    if p.is_absolute() {
        // Outside the work tree it cannot be a filename.
        p.to_path_buf()
    } else {
        // Resolve against the invoking directory when it is inside the
        // work tree; otherwise against the work tree root.
        ctx.cwd
            .canonicalize()
            .ok()
            .and_then(|c| {
                c.strip_prefix(wt).ok().map(|_| c.join(arg))
            })
            .unwrap_or_else(|| wt.join(arg))
    }
}

fn resolve_path(
    ctx: &RepoContext,
    repo: &git_core::Repository,
    op: &str,
) -> String {
    checkout_core::resolve_path_arg(ctx, repo, op)
}

fn resolve_paths(
    ctx: &RepoContext,
    repo: &git_core::Repository,
    ops: &[String],
) -> Vec<String> {
    ops.iter().map(|o| resolve_path(ctx, repo, o)).collect()
}

/// Resolve `rev` as a committish for the no-paths form.
fn resolve_committish(
    repo: &git_core::Repository,
    rev: &str,
) -> Result<git_hash::Oid, CommandError> {
    crate::resolve_arg(repo, rev).map_err(|_| {
        CommandError::fatal(format!("fatal: Failed to resolve '{rev}' as a valid revision."))
    })
}

/// `reset --soft <commit>`: move HEAD only (ORIG_HEAD updated, reflog
/// written). Dies in the middle of a merge with unmerged state.
fn reset_soft(
    repo: &git_core::Repository,
    odb: &Odb,
    rev: &str,
    target: &git_hash::Oid,
    unborn: bool,
) -> Result<(), CommandError> {
    let _ = odb;
    let index = read_index_or_empty(repo)?;
    if checkout_core::has_unmerged(&index) || checkout_core::merge_in_progress(&repo.git_dir) {
        return Err(CommandError::fatal(
            "fatal: Cannot do a soft reset in the middle of a merge.",
        ));
    }
    if !unborn {
        move_head(repo, rev, target)?;
    }
    remove_branch_state(&repo.git_dir);
    Ok(())
}

/// `reset [--mixed] <commit>`: reset the index to the target tree (plus
/// worktree stat refresh), move HEAD, print the unstaged summary.
fn reset_mixed(
    repo: &git_core::Repository,
    odb: &Odb,
    rev: &str,
    target: &git_hash::Oid,
    unborn: bool,
    quiet: bool,
    no_refresh: bool,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    if repo.bare {
        return Err(CommandError::fatal(
            "fatal: mixed reset is not allowed in a bare repository",
        ));
    }
    let algo = repo.hash_algo;
    let tree_oid = commit_tree_of(odb, algo, target)?;
    let new_map = checkout_core::tree_to_map(odb, algo, &tree_oid)?;
    let mut entries = bare_entries(&new_map);
    let work_tree = repo
        .work_tree
        .clone()
        .ok_or_else(|| CommandError::fatal("fatal: this operation must be run in a work tree"))?;
    if !no_refresh {
        let filemode = repo.config.get_bool("core", "filemode").unwrap_or(true);
        refresh_stat(&work_tree, &mut entries, algo, filemode);
    }
    let triples: Vec<(u32, git_hash::Oid, String)> =
        entries.iter().map(|e| (e.mode, e.oid, e.name.clone())).collect();
    let cache_tree = crate::treeobj::cache_tree_from_entries(&triples, algo)
        .map_err(CommandError::from)?;
    let index = Index { version: 2, entries, cache_tree: Some(cache_tree) };
    write_index(repo, &index)?;

    if !unborn {
        move_head(repo, rev, target)?;
    }
    remove_branch_state(&repo.git_dir);

    if !quiet {
        print_unstaged_summary(repo, &work_tree, &index, out)?;
    }
    Ok(())
}

/// `reset --hard <commit>`: move HEAD, reset index and worktree.
fn reset_hard(
    repo: &git_core::Repository,
    odb: &Odb,
    rev: &str,
    target: &git_hash::Oid,
    unborn: bool,
    quiet: bool,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    let algo = repo.hash_algo;
    let work_tree = repo
        .work_tree
        .clone()
        .ok_or_else(|| CommandError::fatal("fatal: this operation must be run in a work tree"))?;
    let tree_oid = commit_tree_of(odb, algo, target)?;
    let new_map = checkout_core::tree_to_map(odb, algo, &tree_oid)?;

    // Reset the index (entries carry fresh stat, like C's checkout write-out).
    let mut index = rebuild_index(repo, &new_map, false)?;
    // Worktree write-out (forced: no safety checks for --hard).
    let old_index = read_index_or_empty(repo)?;
    let old_view = index_tree_view(&old_index);
    let mut old_map = checkout_core::TreeMap::new();
    for (path, (mode, oid)) in &old_view {
        // Blob payloads are only needed for comparisons, which --hard
        // skips; insert placeholders.
        old_map.insert(
            path.clone(),
            checkout_core::BlobInfo { mode: *mode, oid: *oid, data: Vec::new() },
        );
    }
    checkout_core::apply_tree_to_worktree_forced(repo, &new_map, &old_map)?;
    // Fill stat from the freshly written files.
    for e in index.entries.iter_mut() {
        if let Ok(md) = std::fs::symlink_metadata(work_tree.join(&e.name)) {
            checkout_core::fill_stat(e, &md);
        }
    }
    write_index(repo, &index)?;

    if !unborn {
        move_head(repo, rev, target)?;
    }
    remove_branch_state(&repo.git_dir);

    if !quiet && !unborn {
        let subject = checkout_core::commit_subject(odb, algo, target);
        let short = checkout_core::short_oid(repo, target);
        writeln!(out, "HEAD is now at {short} {subject}")
            .map_err(|e| CommandError::fatal(e.to_string()))?;
    }
    Ok(())
}

/// Move HEAD to `target`, updating ORIG_HEAD and reflogs like C's
/// `reset_refs()`.
fn move_head(
    repo: &git_core::Repository,
    rev: &str,
    target: &git_hash::Oid,
) -> Result<(), CommandError> {
    let head = read_head(repo);
    update_orig_head(repo, head.oid);
    let store = git_refs::RefStore::from_repo(repo);
    match &head.symref {
        Some(sym) => {
            store
                .update(sym, Some(target))
                .map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
        }
        None => {
            checkout_core::write_head_detached(repo, target)?;
        }
    }
    if checkout_core::log_all_ref_updates(repo) {
        let ident = checkout_core::committer_ident(repo)?;
        let old = head.oid.unwrap_or(*repo.hash_algo.null_oid());
        let msg = checkout_core::reflog_action(format!("reset: moving to {rev}"));
        checkout_core::reflog_append(repo, "HEAD", &old, target, &ident, &msg);
        if let Some(sym) = head.symref {
            if old != *target {
                checkout_core::reflog_append(repo, &sym, &old, target, &ident, &msg);
            }
        }
    }
    Ok(())
}

/// The tree of a commit-ish target (already verified to be a commit).
/// The empty tree (unborn reset) has no object; it maps to itself.
fn commit_tree_of(
    odb: &Odb,
    algo: git_hash::HashAlgorithm,
    target: &git_hash::Oid,
) -> Result<git_hash::Oid, CommandError> {
    if *target == checkout_core::empty_tree_oid(algo) {
        return Ok(*target);
    }
    let obj = odb
        .read(target)
        .map_err(|_| CommandError::fatal(format!("fatal: unable to read tree ({target})")))?;
    if obj.kind != ObjectKind::Commit {
        return Err(CommandError::fatal(format!("fatal: unable to read tree ({target})")));
    }
    parse_commit(&obj.data, algo)
        .map(|c| c.tree)
        .map_err(|e| CommandError::fatal(format!("fatal: unable to read tree ({target}): {e}")))
}

/// Print "Unstaged changes after reset:" + M/D lines for tracked paths whose
/// worktree content differs from the new index (untracked files excluded).
fn print_unstaged_summary(
    repo: &git_core::Repository,
    work_tree: &PathBuf,
    index: &Index,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    use crate::worktree::worktree_blob;
    let algo = repo.hash_algo;
    let filemode = repo.config.get_bool("core", "filemode").unwrap_or(true);
    let mut lines: Vec<(char, &str)> = Vec::new();
    for e in &index.entries {
        if e.stage != 0 {
            continue;
        }
        match worktree_blob(work_tree, &e.name, algo, filemode) {
            None => lines.push(('D', e.name.as_str())),
            Some((oid, mode)) => {
                if oid != e.oid || mode != e.mode {
                    lines.push(('M', e.name.as_str()));
                }
            }
        }
    }
    if lines.is_empty() {
        return Ok(());
    }
    writeln!(out, "Unstaged changes after reset:")
        .map_err(|e| CommandError::fatal(e.to_string()))?;
    for (c, p) in lines {
        writeln!(out, "{c}\t{p}").map_err(|e| CommandError::fatal(e.to_string()))?;
    }
    Ok(())
}

/// Paths form: copy `rev`'s tree entries for the matched paths into the
/// index (removing entries absent from the tree). Silent except for the
/// "Unstaged changes after reset:" summary (suppressed by `-q`); no HEAD move.
fn reset_paths(
    _ctx: &RepoContext,
    repo: &git_core::Repository,
    odb: &Odb,
    rev: &str,
    paths: &[String],
    quiet: bool,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    let algo = repo.hash_algo;
    let rev_name = if rev == "@" { "HEAD" } else { rev };
    let tree_oid = match crate::resolve_arg(repo, rev_name) {
        Ok(oid) => commit_or_tree_to_tree(odb, algo, &oid).map_err(|_| {
            CommandError::fatal(format!("fatal: Failed to resolve '{rev_name}' as a valid tree."))
        })?,
        Err(_) => {
            return Err(CommandError::fatal(format!(
                "fatal: Failed to resolve '{rev_name}' as a valid tree."
            )));
        }
    };
    let new_map = checkout_core::tree_to_map(odb, algo, &tree_oid)?;
    let mut index = read_index_or_empty(repo)?;
    let matched = |path: &str| paths.iter().any(|s| checkout_core::spec_matches(s, path));
    index.entries.retain(|e| !matched(&e.name));
    for (path, blob) in &new_map {
        if matched(path) {
            index.entries.push(IndexEntry::bare(blob.oid, blob.mode, path.clone()));
        }
    }
    index.entries.sort_by(|a, b| {
        a.name.cmp(&b.name).then_with(|| a.stage.cmp(&b.stage))
    });
    if let Some(ct) = index.cache_tree.as_mut() {
        for p in paths {
            ct.invalidate_path(p);
        }
    }
    write_index(repo, &index)?;
    if !quiet {
        if let Some(wt) = &repo.work_tree {
            print_unstaged_summary(repo, wt, &index, out)?;
        }
    }
    Ok(())
}
