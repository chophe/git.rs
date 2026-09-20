//! `git checkout`: switch branches, detach HEAD, or restore paths.
//!
//! Port of `builtin/checkout.c` (`cmd_checkout`) for the two-way branch
//! switch (including `-b`/`-B`, `--detach`, `--orphan`, `-f`, `-q`,
//! `-t`/`--track` for local start points, and the detached-HEAD advice) and
//! the paths form (`checkout [<tree-ish>] [--] <paths>`, `--ours`/`--theirs`
//! conflict resolution). Deferred with explicit errors: `-m`/`--merge`,
//! `--conflict`, `-p`, remote-tracking DWIM (`checkout <remote-branch>`).

use std::collections::{HashMap, HashSet};
use std::io::Write;

use crate::checkout_core::{
    self, checkout_safety_error, lookup_local_branch, read_head, read_index_or_empty,
    rebuild_index, verify_uptodate, write_index,
};
use crate::{Command, CommandError, RepoContext};
use git_object::ObjectKind;
use git_odb::Odb;

pub struct Checkout;

pub(crate) struct CheckoutArgs {
    pub quiet: bool,
    pub force: bool,
    pub merge: bool,
    pub patch: bool,
    pub detach: bool,
    pub orphan: Option<String>,
    pub new_branch: Option<String>,
    pub new_branch_force: bool,
    pub track: TrackOpt,
    pub ours: bool,
    pub theirs: bool,
    pub conflict: Option<String>,
    pub operands: Vec<String>,
    pub dashdash: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TrackOpt {
    #[default]
    Unspecified,
    Track,
    NoTrack,
}

impl Command for Checkout {
    fn name(&self) -> &'static str {
        "checkout"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let parsed = parse_checkout_args(args, "checkout")?;
        run_checkout(ctx, &parsed, out, Mode::Checkout)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Checkout,
    Switch,
}

pub(crate) fn parse_checkout_args(args: &[String], cmd: &str) -> Result<CheckoutArgs, CommandError> {
    let mut a = CheckoutArgs {
        quiet: false,
        force: false,
        merge: false,
        patch: false,
        detach: false,
        orphan: None,
        new_branch: None,
        new_branch_force: false,
        track: TrackOpt::Unspecified,
        ours: false,
        theirs: false,
        conflict: None,
        operands: Vec::new(),
        dashdash: false,
    };
    let usage = if cmd == "switch" {
        "usage: git switch [<options>] [<branch>]"
    } else {
        "usage: git checkout [<options>] [<branch>]"
    };
    let mut i = 0usize;
    while i < args.len() {
        let s = &args[i];
        match s.as_str() {
            "-q" | "--quiet" => a.quiet = true,
            "-f" | "--force" => a.force = true,
            "--discard-changes" => {
                if cmd != "switch" {
                    return Err(CommandError::usage(format!(
                        "error: unknown option `discard-changes'\n{usage}"
                    )));
                }
                a.force = true;
            }
            "-m" | "--merge" => a.merge = true,
            "-p" | "--patch" => a.patch = true,
            "-d" | "--detach" => a.detach = true,
            "--orphan" => {
                i += 1;
                a.orphan = Some(
                    args.get(i)
                        .ok_or_else(|| {
                            CommandError::usage(format!("error: option `orphan' requires a value\n{usage}"))
                        })?
                        .clone(),
                );
            }
            "-b" => {
                i += 1;
                a.new_branch = Some(
                    args.get(i)
                        .ok_or_else(|| {
                            CommandError::usage(format!("error: option `b' requires a value\n{usage}"))
                        })?
                        .clone(),
                );
            }
            "-B" => {
                i += 1;
                a.new_branch = Some(
                    args.get(i)
                        .ok_or_else(|| {
                            CommandError::usage(format!(
                                "error: option `B' requires a value\n{usage}"
                            ))
                        })?
                        .clone(),
                );
                a.new_branch_force = true;
            }
            "-t" | "--track" => a.track = TrackOpt::Track,
            "--no-track" => a.track = TrackOpt::NoTrack,
            "--guess" | "--no-guess" => {}
            "-c" | "-C" => {
                if cmd != "switch" {
                    return Err(unknown_switch(s, usage));
                }
                i += 1;
                a.new_branch = Some(
                    args.get(i)
                        .ok_or_else(|| {
                            CommandError::usage(format!(
                                "error: option `{}' requires a value\n{usage}",
                                &s[1..]
                            ))
                        })?
                        .clone(),
                );
                a.new_branch_force = *s == "-C";
            }
            "-l" => {}
            "--no-progress" | "--progress" => {}
            "--recurse-submodules" => {}
            s if s.starts_with("--recurse-submodules=") => {}
            "--overwrite-ignore" | "--no-overwrite-ignore" => {}
            "--ignore-skip-worktree-bits" | "--no-ignore-skip-worktree-bits" => {}
            "--ignore-other-worktrees" => {}
            "--ours" => a.ours = true,
            "--theirs" => a.theirs = true,
            s if s.starts_with("--conflict=") => {
                a.conflict = Some(s["--conflict=".len()..].to_string())
            }
            "--conflict" => {
                i += 1;
                a.conflict = Some(
                    args.get(i)
                        .ok_or_else(|| {
                            CommandError::usage(format!(
                                "error: option `conflict' requires a value\n{usage}"
                            ))
                        })?
                        .clone(),
                );
            }
            "--" => {
                // Keep the marker (like PARSE_OPT_KEEP_DASHDASH) so the
                // "Updated N paths" rule can see it.
                a.dashdash = true;
                a.operands.push("--".to_string());
                a.operands.extend(args[i + 1..].iter().cloned());
                break;
            }
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(unknown_switch(s, usage));
            }
            s => a.operands.push(s.to_string()),
        }
        i += 1;
    }
    Ok(a)
}

fn unknown_switch(s: &str, usage: &str) -> CommandError {
    if let Some(long) = s.strip_prefix("--") {
        let name = long.split('=').next().unwrap_or(long);
        CommandError::usage(format!("error: unknown option `{name}'\n{usage}"))
    } else {
        let c = s.chars().nth(1).unwrap_or('?');
        CommandError::usage(format!("error: unknown switch `{c}'\n{usage}"))
    }
}

/// Shared implementation for `checkout` and `switch` (which parses its own
/// args and delegates here with `Mode::Switch`).
pub(crate) fn run_checkout(
    ctx: &RepoContext,
    a: &CheckoutArgs,
    out: &mut dyn Write,
    mode: Mode,
) -> Result<(), CommandError> {
    if a.patch {
        return Err(CommandError::fatal("fatal: checkout: --patch is not supported yet"));
    }
    if a.merge {
        return Err(CommandError::fatal("fatal: checkout: --merge is not supported yet"));
    }
    if a.conflict.is_some() {
        return Err(CommandError::fatal("fatal: checkout: --conflict is not supported yet"));
    }
    if a.orphan.is_some() && a.detach {
        return Err(CommandError::fatal("fatal: '--detach' cannot be used with '--orphan'"));
    }
    if a.orphan.is_some() && a.new_branch.is_some() {
        let (l, u) = if mode == Mode::Switch { ("c", "C") } else { ("b", "B") };
        return Err(CommandError::fatal(format!(
            "fatal: options '-{l}', '-{u}', and '--orphan' cannot be used together"
        )));
    }

    // Split operands around "--".
    let dd = a.operands.iter().position(|o| o == "--");
    let (rev_part, paths) = match dd {
        Some(0) => (Vec::new(), a.operands[1..].to_vec()),
        Some(n) => {
            if n > 1 {
                return Err(CommandError::fatal(format!(
                    "fatal: only one reference expected, {n} given."
                )));
            }
            (a.operands[..1].to_vec(), a.operands[2..].to_vec())
        }
        None => {
            if mode == Mode::Switch {
                if a.operands.len() > 1 {
                    return Err(CommandError::fatal("fatal: only one reference expected"));
                }
                (a.operands.clone(), Vec::new())
            } else if a.operands.len() > 1 && !resolves_as_rev(ctx, &a.operands[0]) {
                // `checkout <path>...`: everything is paths (from index).
                (Vec::new(), a.operands.clone())
            } else if a.operands.len() > 1 {
                // `checkout <tree-ish> <paths>` without "--".
                (a.operands[..1].to_vec(), a.operands[1..].to_vec())
            } else {
                (a.operands.clone(), Vec::new())
            }
        }
    };

    // switch never accepts paths.
    if mode == Mode::Switch && !paths.is_empty() {
        return Err(CommandError::fatal(format!("fatal: invalid reference: {}", paths[0])));
    }

    // Paths form (checkout only).
    if !paths.is_empty() || (mode == Mode::Checkout && a.dashdash) {
        return checkout_paths(ctx, a, &rev_part, &paths, out);
    }

    // Branch-switch form.
    if mode == Mode::Switch && rev_part.is_empty() && a.new_branch.is_none() && !a.detach {
        return Err(CommandError::fatal("fatal: missing branch or commit argument"));
    }
    switch_branch(ctx, a, rev_part.first().map(String::as_str), out, mode)
}

/// Does `arg` resolve as a revision (for the tree-vs-paths split)?
fn resolves_as_rev(ctx: &RepoContext, arg: &str) -> bool {
    let Ok(repo) = ctx.repository() else { return false };
    let at = if arg == "@" { "HEAD" } else { arg };
    crate::resolve_arg(&repo, at).is_ok()
}

/// Resolve `-` to the previous branch via logs/HEAD.
fn resolve_dash(repo: &git_core::Repository, arg: &str) -> String {
    if arg == "-" {
        if let Some(prev) = checkout_core::previous_branch(repo) {
            return prev;
        }
    }
    arg.to_string()
}

/// The branch-switch form shared by checkout and switch.
fn switch_branch(
    ctx: &RepoContext,
    a: &CheckoutArgs,
    arg: Option<&str>,
    out: &mut dyn Write,
    mode: Mode,
) -> Result<(), CommandError> {
    let repo = ctx.repository()?;
    let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
    let head = read_head(&repo);
    let store = git_refs::RefStore::from_repo(&repo);

    // --orphan takes a branch name, never a start point.
    if let Some(orphan) = &a.orphan {
        if arg.is_some() {
            return Err(CommandError::fatal("fatal: '--orphan' cannot take <start-point>"));
        }
        if a.track == TrackOpt::Track {
            return Err(CommandError::fatal("fatal: '--orphan' cannot be used with '-t'"));
        }
        return switch_orphan(ctx, &repo, &odb, a, orphan, mode);
    }

    // No branch argument at all.
    let Some(raw_arg) = arg else {
        if a.new_branch.is_some() {
            // `checkout -b new` / `switch -c new`: create at HEAD.
            return create_and_switch(ctx, &repo, &odb, a, NewBranchStart::Head, out, mode);
        }
        if a.detach {
            // Detach at HEAD.
            let oid = head.oid.ok_or_else(|| {
                CommandError::fatal("fatal: You are on a branch yet to be born")
            })?;
            return detach_head(ctx, &repo, &odb, a, "HEAD", &oid, out, mode);
        }
        // Bare `checkout` with no args is a silent no-op.
        return Ok(());
    };
    let arg = resolve_dash(&repo, raw_arg);
    let arg = arg.as_str();

    // `checkout HEAD` (exactly) is a silent no-op.
    if mode == Mode::Checkout && arg == "HEAD" && a.new_branch.is_none() && !a.detach {
        return Ok(());
    }

    // A local branch?
    if let Some((full_ref, tip)) = lookup_local_branch(&store, arg) {
        if a.detach {
            return detach_head(ctx, &repo, &odb, a, arg, &tip, out, mode);
        }
        if a.new_branch.is_some() {
            // `-b`/`-B` with an explicit start point that is a branch.
            let up = arg.strip_prefix("refs/heads/").unwrap_or(arg).to_string();
            return create_and_switch(
                ctx,
                &repo,
                &odb,
                a,
                NewBranchStart::Commit { upstream: Some(up), oid: tip },
                out,
                mode,
            );
        }
        return switch_to_branch(ctx, &repo, &odb, a, &full_ref, &tip, out, mode, NewBranch::No);
    }

    // Otherwise it must resolve to a commit (detach), else DWIM/error paths.
    let at_arg = if arg == "@" { "HEAD" } else { arg };
    let oid = match crate::resolve_arg(&repo, at_arg) {
        Ok(oid) => match odb.read(&oid) {
            Ok(_) => oid,
            // Resolves syntactically but the object is missing: behave as
            // if it did not resolve (pathspec error below).
            Err(_) => return not_a_revision(ctx, &repo, a, arg, out, mode),
        },
        Err(_) => return not_a_revision(ctx, &repo, a, arg, out, mode),
    };
    // Classify the object.
    let obj = odb.read(&oid).map_err(|_| {
        CommandError::fatal(format!("fatal: unable to read tree ({oid})"))
    })?;
    let commit_oid = match obj.kind {
        ObjectKind::Commit => oid,
        ObjectKind::Tag => match checkout_core::peel_to_commit(&odb, &oid) {
            Ok(c) => c,
            Err((bad, kind)) => {
                if kind != "bad" {
                    eprintln!("error: object {bad} is a {kind}, not a commit");
                }
                return Err(CommandError::fatal(format!(
                    "fatal: Could not parse object '{arg}'."
                )));
            }
        },
        ObjectKind::Tree => {
            return Err(CommandError::fatal(format!(
                "fatal: Cannot switch branch to a non-commit '{arg}'"
            )));
        }
        ObjectKind::Blob => {
            return Err(CommandError::fatal(format!("fatal: unable to read tree ({oid})")));
        }
    };
    if mode == Mode::Switch && !a.detach {
        return die_expecting_a_branch(&repo, arg, &oid);
    }
    if a.new_branch.is_some() {
        return create_and_switch(
            ctx,
            &repo,
            &odb,
            a,
            NewBranchStart::Commit { upstream: None, oid: commit_oid },
            out,
            mode,
        );
    }
    detach_head(ctx, &repo, &odb, a, arg, &commit_oid, out, mode)
}

/// The argument is not a local branch and did not resolve: for checkout it
/// may still be a path (or remote DWIM, deferred); for switch it is invalid.
fn not_a_revision(
    ctx: &RepoContext,
    repo: &git_core::Repository,
    a: &CheckoutArgs,
    arg: &str,
    out: &mut dyn Write,
    mode: Mode,
) -> Result<(), CommandError> {
    if mode == Mode::Switch {
        return Err(CommandError::fatal(format!("fatal: invalid reference: {arg}")));
    }
    // Remote-tracking DWIM is deferred: report the pathspec error C would
    // print when no tracking branch matches either.
    if is_known_path(ctx, repo, arg) {
        return checkout_paths(ctx, a, &[], &[arg.to_string()], out);
    }
    Err(CommandError::error(format!(
        "error: pathspec '{arg}' did not match any file(s) known to git"
    )))
}

/// Is `arg` a path in the index or worktree (for checkout DWIM)?
fn is_known_path(ctx: &RepoContext, repo: &git_core::Repository, arg: &str) -> bool {
    let rel = checkout_core::resolve_path_arg(ctx, repo, arg);
    if let Ok(index) = read_index_or_empty(repo) {
        if index.entries.iter().any(|e| e.name == rel) {
            return true;
        }
    }
    if let Some(wt) = &repo.work_tree {
        if std::fs::symlink_metadata(wt.join(&rel)).is_ok() {
            return true;
        }
    }
    false
}

/// `switch <non-branch>`: "a branch is expected, got ..." + detach hint.
fn die_expecting_a_branch(
    repo: &git_core::Repository,
    arg: &str,
    oid: &git_hash::Oid,
) -> Result<(), CommandError> {
    let store = git_refs::RefStore::from_repo(repo);
    let odb = Odb::from_repo(repo).map_err(CommandError::from)?;
    let msg = match odb.read(oid) {
        Ok(o) if o.kind == ObjectKind::Tag => {
            let name = store
                .list()
                .iter()
                .filter(|(n, o)| n.starts_with("refs/tags/") && *o == *oid)
                .map(|(n, _)| n.trim_start_matches("refs/tags/").to_string())
                .next()
                .unwrap_or_else(|| arg.to_string());
            format!("fatal: a branch is expected, got tag '{name}'")
        }
        _ => {
            if store.resolve(&format!("refs/remotes/{arg}")).is_some() {
                format!("fatal: a branch is expected, got remote branch '{arg}'")
            } else if arg == "HEAD" {
                if let Some(sym) = store.head_symbolic_target() {
                    format!("fatal: a branch is expected, got '{sym}'")
                } else {
                    format!("fatal: a branch is expected, got commit '{arg}'")
                }
            } else {
                format!("fatal: a branch is expected, got commit '{arg}'")
            }
        }
    };
    eprintln!("{msg}");
    if repo.config.get_bool("advice", "suggestdetachinghead").unwrap_or(true) {
        eprintln!(
            "hint: If you want to detach HEAD at the commit, try again with the --detach option."
        );
    }
    Err(CommandError::silent(128))
}

/// How a `-b`/`-B`/`-c`/`-C` branch gets its start point.
enum NewBranchStart {
    /// At HEAD (no explicit start point).
    Head,
    /// At an explicit commit; `upstream` is the start branch short name
    /// when the start point was a local branch (for `--track`).
    Commit { upstream: Option<String>, oid: git_hash::Oid },
}

/// Whether the switch target is a newly-created branch (message selection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NewBranch {
    No,
    /// Created now; `existed` selects "Switched to and reset" vs "new branch".
    Yes { existed: bool },
}

/// Create a new branch at HEAD or at an explicit commit, then switch to it.
fn create_and_switch(
    ctx: &RepoContext,
    repo: &git_core::Repository,
    odb: &Odb,
    a: &CheckoutArgs,
    start: NewBranchStart,
    out: &mut dyn Write,
    mode: Mode,
) -> Result<(), CommandError> {
    let name = a.new_branch.clone().unwrap();
    let head = read_head(repo);
    let (start_oid, upstream): (git_hash::Oid, Option<String>) = match start {
        NewBranchStart::Head => match head.oid {
            // Unborn HEAD: just move the symref
            // (switch_unborn_to_new_branch).
            None => {
                let full = checkout_core::new_branch_ref(&name)?;
                checkout_core::write_head_symref(repo, &full)?;
                if !a.quiet {
                    eprintln!("Switched to a new branch '{name}'");
                }
                return Ok(());
            }
            Some(oid) => (oid, None),
        },
        NewBranchStart::Commit { upstream, oid } => (oid, upstream),
    };
    let full = checkout_core::new_branch_ref(&name)?;
    let store = git_refs::RefStore::from_repo(repo);
    let existed = store.resolve(&full).is_some();
    if existed && !a.new_branch_force {
        return Err(CommandError::fatal(format!(
            "fatal: a branch named '{name}' already exists"
        )));
    }
    store
        .update(&full, Some(&start_oid))
        .map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
    // --track from a local start point records upstream config + message.
    if a.track == TrackOpt::Track {
        if let Some(up) = upstream.as_deref() {
            append_branch_config(repo, &name, ".", &format!("refs/heads/{up}"))?;
            writeln!(out, "branch '{name}' set up to track '{up}'.")
                .map_err(|e| CommandError::fatal(e.to_string()))?;
        }
    }
    switch_to_branch(ctx, repo, odb, a, &full, &start_oid, out, mode, NewBranch::Yes { existed })
}

/// Append a `[branch "name"]` stanza to the repo config.
fn append_branch_config(
    repo: &git_core::Repository,
    name: &str,
    remote: &str,
    merge: &str,
) -> Result<(), CommandError> {
    let path = repo.git_dir.join("config");
    let mut content = std::fs::read_to_string(&path).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&format!("[branch \"{name}\"]\n\tremote = {remote}\n\tmerge = {merge}\n"));
    std::fs::write(&path, content).map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
    Ok(())
}

/// Switch HEAD to an existing local branch (two-way worktree merge).
#[allow(clippy::too_many_arguments)]
fn switch_to_branch(
    ctx: &RepoContext,
    repo: &git_core::Repository,
    odb: &Odb,
    a: &CheckoutArgs,
    full_ref: &str,
    tip: &git_hash::Oid,
    out: &mut dyn Write,
    mode: Mode,
    is_new: NewBranch,
) -> Result<(), CommandError> {
    let algo = repo.hash_algo;
    let head = read_head(repo);

    if mode == Mode::Switch {
        check_operation_in_progress(repo)?;
    }

    let same_branch = head.symref.as_deref() == Some(full_ref);

    let old_tree = match head.oid {
        Some(oid) => {
            let tree = checkout_core::commit_or_tree_to_tree(odb, algo, &oid)?;
            checkout_core::tree_to_map(odb, algo, &tree)?
        }
        None => checkout_core::TreeMap::new(),
    };
    let new_tree_oid = checkout_core::commit_or_tree_to_tree(odb, algo, tip)?;
    let new_map = checkout_core::tree_to_map(odb, algo, &new_tree_oid)?;

    let work_tree = repo
        .work_tree
        .clone()
        .ok_or_else(|| CommandError::fatal("fatal: this operation must be run in a work tree"))?;
    let filemode = repo.config.get_bool("core", "filemode").unwrap_or(true);
    if !a.force {
        let (local, untracked) = verify_uptodate(&work_tree, &old_tree, &new_map, algo, filemode);
        if !local.is_empty() || !untracked.is_empty() {
            return Err(checkout_safety_error(&local, &untracked));
        }
        unmerged_block_error(repo)?;
    } else if checkout_core::has_unmerged(&read_index_or_empty(repo)?) {
        checkout_core::remove_branch_state(&repo.git_dir);
    }

    // Leaving a detached HEAD: orphaned-commit warning when unreachable.
    if !a.quiet && head.symref.is_none() {
        if let Some(old_oid) = head.oid {
            if old_oid != *tip {
                orphaned_commit_warning(ctx, repo, odb, &old_oid, tip, out)?;
            }
        }
    }

    let old_index = read_index_or_empty(repo)?;
    checkout_core::apply_tree_to_worktree(repo, &new_map, &old_tree)?;
    let mut index = rebuild_index(repo, &new_map, false)?;
    // Preserve stat for paths whose blobs did not change (C keeps them).
    let old_entries: HashMap<&str, &git_index::IndexEntry> =
        old_index.entries.iter().map(|e| (e.name.as_str(), e)).collect();
    for e in index.entries.iter_mut() {
        if let Some(old_e) = old_entries.get(e.name.as_str()) {
            if old_e.oid == e.oid && old_e.mode == e.mode && old_e.stage == 0 {
                *e = (*old_e).clone();
            } else if let Ok(md) = std::fs::symlink_metadata(work_tree.join(&e.name)) {
                checkout_core::fill_stat(e, &md);
            }
        } else if let Ok(md) = std::fs::symlink_metadata(work_tree.join(&e.name)) {
            checkout_core::fill_stat(e, &md);
        }
    }
    write_index(repo, &index)?;

    let short = full_ref.strip_prefix("refs/heads/").unwrap_or(full_ref);
    let old_desc = head
        .symref
        .as_deref()
        .map(|s| s.strip_prefix("refs/heads/").unwrap_or(s).to_string())
        .or_else(|| head.oid.map(|o| o.to_string()));
    let msg = checkout_core::reflog_action(format!(
        "checkout: moving from {} to {short}",
        old_desc.as_deref().unwrap_or("(invalid)")
    ));
    checkout_core::write_head_symref(repo, full_ref)?;
    if checkout_core::log_all_ref_updates(repo) {
        let ident = checkout_core::committer_ident(repo)?;
        let old_oid = head.oid.unwrap_or(*algo.null_oid());
        checkout_core::reflog_append(repo, "HEAD", &old_oid, tip, &ident, &msg);
    }

    if !a.quiet {
        match (same_branch, is_new) {
            (true, NewBranch::Yes { .. }) if a.new_branch_force => {
                eprintln!("Reset branch '{short}'");
            }
            (true, _) => {
                eprintln!("Already on '{short}'");
            }
            (false, NewBranch::Yes { existed: true }) => {
                eprintln!("Switched to and reset branch '{short}'");
            }
            (false, NewBranch::Yes { existed: false }) => {
                eprintln!("Switched to a new branch '{short}'");
            }
            (false, NewBranch::No) => {
                eprintln!("Switched to branch '{short}'");
            }
        }
        // Upstream tracking status (stdout), for pre-existing branches only.
        if is_new == NewBranch::No {
            if let Some(status) = checkout_core::tracking_status(repo, odb, short) {
                writeln!(out, "{status}").map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }
    }
    // C's merge_working_tree reports carried-over local changes
    // (`diff-index <new-head>` M/D lines), except when creating a non-force
    // branch at the same commit (which takes the silent one-way path).
    // `checkout -q`/`-f` never report.
    let created_same = matches!(is_new, NewBranch::Yes { .. })
        && !a.new_branch_force
        && head.oid.map(|o| o == *tip).unwrap_or(false);
    if !a.quiet && !a.force && !created_same {
        print_local_changes(&work_tree, &index, algo, filemode, out)?;
    }
    Ok(())
}

/// Report carried-over local modifications after a branch switch, like C's
/// `show_local_changes` (`diff-index <new-head>` M/D lines on stdout).
fn print_local_changes(
    work_tree: &std::path::Path,
    index: &git_index::Index,
    algo: git_hash::HashAlgorithm,
    filemode: bool,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    let mut lines: Vec<(char, String)> = Vec::new();
    for e in &index.entries {
        if e.stage != 0 {
            continue;
        }
        match crate::worktree::worktree_blob(work_tree, &e.name, algo, filemode) {
            None => {
                // A directory (e.g. submodule) cannot be compared; skip it.
                if !work_tree.join(&e.name).is_dir() {
                    lines.push(('D', e.name.clone()));
                }
            }
            Some((oid, mode)) => {
                if oid != e.oid || mode != e.mode {
                    lines.push(('M', e.name.clone()));
                }
            }
        }
    }
    lines.sort();
    lines.dedup();
    for (c, p) in &lines {
        writeln!(out, "{c}\t{p}").map_err(|e| CommandError::fatal(e.to_string()))?;
    }
    Ok(())
}

/// Detach HEAD at `oid`.
#[allow(clippy::too_many_arguments)]
fn detach_head(
    ctx: &RepoContext,
    repo: &git_core::Repository,
    odb: &Odb,
    a: &CheckoutArgs,
    arg: &str,
    oid: &git_hash::Oid,
    out: &mut dyn Write,
    mode: Mode,
) -> Result<(), CommandError> {
    let algo = repo.hash_algo;
    let head = read_head(repo);

    if mode == Mode::Switch {
        check_operation_in_progress(repo)?;
    }

    let old_tree = match head.oid {
        Some(o) => {
            let t = checkout_core::commit_or_tree_to_tree(odb, algo, &o)?;
            checkout_core::tree_to_map(odb, algo, &t)?
        }
        None => checkout_core::TreeMap::new(),
    };
    let new_tree_oid = checkout_core::commit_or_tree_to_tree(odb, algo, oid)?;
    let new_map = checkout_core::tree_to_map(odb, algo, &new_tree_oid)?;

    let work_tree = repo
        .work_tree
        .clone()
        .ok_or_else(|| CommandError::fatal("fatal: this operation must be run in a work tree"))?;
    let filemode = repo.config.get_bool("core", "filemode").unwrap_or(true);
    if !a.force {
        let (local, untracked) = verify_uptodate(&work_tree, &old_tree, &new_map, algo, filemode);
        if !local.is_empty() || !untracked.is_empty() {
            return Err(checkout_safety_error(&local, &untracked));
        }
        unmerged_block_error(repo)?;
    }

    if !a.quiet {
        if let Some(old_oid) = head.oid {
            if head.symref.is_none() && old_oid != *oid {
                orphaned_commit_warning(ctx, repo, odb, &old_oid, oid, out)?;
            }
        }
    }

    let old_index = read_index_or_empty(repo)?;
    checkout_core::apply_tree_to_worktree(repo, &new_map, &old_tree)?;
    let mut index = rebuild_index(repo, &new_map, false)?;
    let old_entries: HashMap<&str, &git_index::IndexEntry> =
        old_index.entries.iter().map(|e| (e.name.as_str(), e)).collect();
    for e in index.entries.iter_mut() {
        if let Some(old_e) = old_entries.get(e.name.as_str()) {
            if old_e.oid == e.oid && old_e.mode == e.mode && old_e.stage == 0 {
                *e = (*old_e).clone();
            } else if let Ok(md) = std::fs::symlink_metadata(work_tree.join(&e.name)) {
                checkout_core::fill_stat(e, &md);
            }
        } else if let Ok(md) = std::fs::symlink_metadata(work_tree.join(&e.name)) {
            checkout_core::fill_stat(e, &md);
        }
    }
    write_index(repo, &index)?;

    let old_desc = head
        .symref
        .as_deref()
        .map(|s| s.strip_prefix("refs/heads/").unwrap_or(s).to_string())
        .or_else(|| head.oid.map(|o| o.to_string()));
    let msg = checkout_core::reflog_action(format!(
        "checkout: moving from {} to {arg}",
        old_desc.as_deref().unwrap_or("(invalid)")
    ));
    checkout_core::write_head_detached(repo, oid)?;
    if checkout_core::log_all_ref_updates(repo) {
        let ident = checkout_core::committer_ident(repo)?;
        let old_oid = head.oid.unwrap_or(*algo.null_oid());
        checkout_core::reflog_append(repo, "HEAD", &old_oid, oid, &ident, &msg);
    }

    if !a.quiet {
        // Detach advice when leaving a branch (not for --detach itself).
        if head.symref.is_some()
            && !a.detach
            && repo.config.get_bool("advice", "detachedhead").unwrap_or(true)
        {
            eprintln!("{}", DETACH_ADVICE.replace("{arg}", arg));
        }
        let short = checkout_core::short_oid(repo, oid);
        let subject = checkout_core::commit_subject(odb, algo, oid);
        eprintln!("HEAD is now at {short} {subject}");
    }
    if !a.quiet && !a.force {
        print_local_changes(&work_tree, &index, algo, filemode, out)?;
    }
    Ok(())
}

const DETACH_ADVICE: &str = "Note: switching to '{arg}'.

You are in 'detached HEAD' state. You can look around, make experimental
changes and commit them, and you can discard any commits you make in this
state without impacting any branches by switching back to a branch.

If you want to create a new branch to retain commits you create, you may
do so (now or later) by using -c with the switch command. Example:

  git switch -c <new-branch-name>

Or undo this operation with:

  git switch -

Turn off this advice by setting config variable advice.detachedHead to false";

/// The orphaned-commit warning when leaving a detached HEAD ("Previous HEAD
/// position was ..." when still reachable, else the leaving-behind warning).
fn orphaned_commit_warning(
    _ctx: &RepoContext,
    repo: &git_core::Repository,
    odb: &Odb,
    old_oid: &git_hash::Oid,
    new_oid: &git_hash::Oid,
    _out: &mut dyn Write,
) -> Result<(), CommandError> {
    let algo = repo.hash_algo;
    let store = git_refs::RefStore::from_repo(repo);
    // Uninteresting: every ref tip plus the commit we are moving to.
    let mut roots: Vec<git_hash::Oid> = store.list().iter().map(|(_, o)| *o).collect();
    roots.push(*new_oid);
    let excluded = reachable_set(odb, algo, &roots);
    // Commits reachable from old but from nothing else.
    let mut orphans = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![*old_oid];
    while let Some(oid) = stack.pop() {
        if !seen.insert(oid) || excluded.contains(&oid) {
            continue;
        }
        orphans.push(oid);
        if let Ok(o) = odb.read(&oid) {
            if o.kind == ObjectKind::Commit {
                if let Ok(c) = git_object::parse_commit(&o.data, algo) {
                    stack.extend(c.parents);
                }
            }
        }
    }
    if orphans.is_empty() {
        let short = checkout_core::short_oid(repo, old_oid);
        let subject = checkout_core::commit_subject(odb, algo, old_oid);
        eprintln!("Previous HEAD position was {short} {subject}");
        return Ok(());
    }
    // Limit the listing like C's ORPHAN_CUTOFF.
    const CUTOFF: usize = 4;
    let total = orphans.len();
    let mut text = String::new();
    for oid in orphans.iter().take(CUTOFF.min(total)) {
        let short = checkout_core::short_oid(repo, oid);
        let subject = checkout_core::commit_subject(odb, algo, oid);
        text.push_str(&format!("  {short} {subject}\n"));
    }
    if total > CUTOFF {
        let more = total - CUTOFF;
        if more == 1 {
            if let Some(last) = orphans.last() {
                let short = checkout_core::short_oid(repo, last);
                let subject = checkout_core::commit_subject(odb, algo, last);
                text.push_str(&format!("  {short} {subject}\n"));
            }
        } else {
            text.push_str(&format!(" ... and {more} more.\n"));
        }
    }
    if total == 1 {
        eprintln!(
            "Warning: you are leaving 1 commit behind, not connected to\nany of your branches:\n\n{text}"
        );
    } else {
        eprintln!(
            "Warning: you are leaving {total} commits behind, not connected to\nany of your branches:\n\n{text}"
        );
    }
    if repo.config.get_bool("advice", "detachedhead").unwrap_or(true) {
        let short = checkout_core::short_oid(repo, old_oid);
        if total == 1 {
            eprintln!(
                "If you want to keep it by creating a new branch, this may be a good time\nto do so with:\n\n git branch <new-branch-name> {short}\n"
            );
        } else {
            eprintln!(
                "If you want to keep them by creating a new branch, this may be a good time\nto do so with:\n\n git branch <new-branch-name> {short}\n"
            );
        }
    }
    Ok(())
}

fn reachable_set(odb: &Odb, algo: git_hash::HashAlgorithm, roots: &[git_hash::Oid]) -> HashSet<git_hash::Oid> {
    let mut seen = HashSet::new();
    let mut stack: Vec<git_hash::Oid> = roots.to_vec();
    while let Some(oid) = stack.pop() {
        if !seen.insert(oid) {
            continue;
        }
        if let Ok(o) = odb.read(&oid) {
            if o.kind == ObjectKind::Commit {
                if let Ok(c) = git_object::parse_commit(&o.data, algo) {
                    stack.extend(c.parents);
                }
            }
        }
    }
    seen
}

/// `--orphan <name>`: unborn branch. checkout keeps index/worktree;
/// switch starts from the empty tree (index emptied, tracked files removed).
fn switch_orphan(
    ctx: &RepoContext,
    repo: &git_core::Repository,
    odb: &Odb,
    a: &CheckoutArgs,
    name: &str,
    mode: Mode,
) -> Result<(), CommandError> {
    let _ = ctx;
    let full = checkout_core::new_branch_ref(name)?;
    let store = git_refs::RefStore::from_repo(repo);
    if store.resolve(&full).is_some() {
        return Err(CommandError::fatal(format!(
            "fatal: a branch named '{name}' already exists"
        )));
    }
    if mode == Mode::Switch {
        check_operation_in_progress(repo)?;
        let head = read_head(repo);
        let algo = repo.hash_algo;
        let old_tree = match head.oid {
            Some(o) => {
                let t = checkout_core::commit_or_tree_to_tree(odb, algo, &o)?;
                checkout_core::tree_to_map(odb, algo, &t)?
            }
            None => checkout_core::TreeMap::new(),
        };
        let new_map = checkout_core::TreeMap::new();
        let work_tree = repo.work_tree.clone().ok_or_else(|| {
            CommandError::fatal("fatal: this operation must be run in a work tree")
        })?;
        let filemode = repo.config.get_bool("core", "filemode").unwrap_or(true);
        if !a.force {
            let (local, untracked) =
                verify_uptodate(&work_tree, &old_tree, &new_map, algo, filemode);
            if !local.is_empty() || !untracked.is_empty() {
                return Err(checkout_safety_error(&local, &untracked));
            }
            unmerged_block_error(repo)?;
        }
        checkout_core::apply_tree_to_worktree(repo, &new_map, &old_tree)?;
        let index = rebuild_index(repo, &new_map, false)?;
        write_index(repo, &index)?;
    }
    checkout_core::write_head_symref(repo, &full)?;
    if !a.quiet {
        eprintln!("Switched to a new branch '{name}'");
    }
    Ok(())
}

/// Fail like unpack-trees when unmerged entries exist (non-forced switch).
fn unmerged_block_error(repo: &git_core::Repository) -> Result<(), CommandError> {
    let index = read_index_or_empty(repo)?;
    let mut names: Vec<&str> = index
        .entries
        .iter()
        .filter(|e| e.stage != 0)
        .map(|e| e.name.as_str())
        .collect();
    names.sort();
    names.dedup();
    if names.is_empty() {
        return Ok(());
    }
    let mut out = String::new();
    for n in &names {
        out.push_str(&format!("{n}: needs merge\n"));
    }
    eprint!("{out}");
    Err(CommandError::error("error: you need to resolve your current index first"))
}

/// `switch`'s in-progress-operation guard.
fn check_operation_in_progress(repo: &git_core::Repository) -> Result<(), CommandError> {
    use crate::checkout_core::OpInProgress;
    match checkout_core::operation_in_progress(&repo.git_dir) {
        OpInProgress::None => Ok(()),
        OpInProgress::Merge => Err(CommandError::fatal(
            "fatal: cannot switch branch while merging\nConsider \"git merge --quit\" or \"git worktree add\".",
        )),
        OpInProgress::Am => Err(CommandError::fatal(
            "fatal: cannot switch branch in the middle of an am session\nConsider \"git am --quit\" or \"git worktree add\".",
        )),
        OpInProgress::Rebase => Err(CommandError::fatal(
            "fatal: cannot switch branch while rebasing\nConsider \"git rebase --quit\" or \"git worktree add\".",
        )),
        OpInProgress::CherryPick => Err(CommandError::fatal(
            "fatal: cannot switch branch while cherry-picking\nConsider \"git cherry-pick --quit\" or \"git worktree add\".",
        )),
        OpInProgress::Revert => Err(CommandError::fatal(
            "fatal: cannot switch branch while reverting\nConsider \"git revert --quit\" or \"git worktree add\".",
        )),
        OpInProgress::Bisect => {
            eprintln!("warning: you are switching branch while bisecting");
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Paths form.
// ---------------------------------------------------------------------------

/// `checkout [<tree-ish>] [--] <paths>` and `checkout -- <paths>`.
#[allow(clippy::too_many_arguments)]
fn checkout_paths(
    ctx: &RepoContext,
    a: &CheckoutArgs,
    rev_part: &[String],
    paths: &[String],
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    if paths.is_empty() {
        // `checkout <tree-ish> --` with no paths: nothing to do.
        return Ok(());
    }
    if a.new_branch.is_some() {
        let nb = a.new_branch.as_deref().unwrap();
        return Err(CommandError::fatal(format!(
            "fatal: Cannot update paths and switch to branch '{nb}' at the same time."
        )));
    }
    if a.detach {
        return Err(CommandError::fatal("fatal: '--detach' cannot be used with updating paths"));
    }
    if a.track == TrackOpt::Track {
        return Err(CommandError::fatal("fatal: '--track' cannot be used with updating paths"));
    }
    if (a.ours || a.theirs) && !rev_part.is_empty() {
        return Err(CommandError::fatal(
            "fatal: '--merge', '--ours', or '--theirs' cannot be used when checking out of a tree",
        ));
    }

    let repo = ctx.repository()?;
    let algo = repo.hash_algo;
    let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;

    // Source: explicit tree-ish, or the index.
    let (source_map, source_label): (Option<checkout_core::TreeMap>, Option<String>) =
        if rev_part.is_empty() {
            (None, None)
        } else {
            let rev = if rev_part[0] == "@" { "HEAD".to_string() } else { rev_part[0].clone() };
            let oid = crate::resolve_arg(&repo, &rev).map_err(|_| {
                CommandError::fatal(format!("fatal: could not resolve '{rev}'"))
            })?;
            let tree_oid = checkout_core::commit_or_tree_to_tree(&odb, algo, &oid).map_err(|_| {
                CommandError::fatal(format!("fatal: reference is not a tree: {rev}"))
            })?;
            let label = checkout_core::short_oid(&repo, &tree_oid);
            let map = checkout_core::tree_to_map(&odb, algo, &tree_oid)?;
            (Some(map), Some(label))
        };

    let mut index = read_index_or_empty(&repo)?;
    let matched = |path: &str| paths.iter().any(|s| checkout_core::spec_matches(s, path));

    // read_tree_some: with a source tree, overlay its matched entries into
    // the index (entries absent from the source are kept: overlay mode).
    if let Some(src) = &source_map {
        index.entries.retain(|e| {
            if e.stage != 0 {
                return true;
            }
            if !matched(&e.name) {
                return true;
            }
            // Overlay: only replace entries the source provides.
            src.contains_key(&e.name)
        });
        index.entries.retain(|e| {
            !(e.stage == 0 && matched(&e.name) && src.contains_key(&e.name))
        });
        for (path, blob) in src {
            if matched(path) {
                index
                    .entries
                    .push(git_index::IndexEntry::bare(blob.oid, blob.mode, path.clone()));
            }
        }
    }

    // Every pathspec must match something (index or, with a source, tree).
    for p in paths {
        let hit = index.entries.iter().any(|e| checkout_core::spec_matches(p, &e.name))
            || source_map
                .as_ref()
                .is_some_and(|t| t.keys().any(|k| checkout_core::spec_matches(p, k)));
        if !hit {
            return Err(CommandError::error(format!(
                "error: pathspec '{p}' did not match any file(s) known to git"
            )));
        }
    }

    // Unmerged handling for matched entries.
    let mut unmerged_names: Vec<String> = Vec::new();
    for e in index.entries.iter().filter(|e| e.stage != 0 && matched(&e.name)) {
        if !unmerged_names.contains(&e.name) {
            unmerged_names.push(e.name.clone());
        }
    }
    let mut skip: HashSet<String> = HashSet::new();
    if !unmerged_names.is_empty() {
        if a.force {
            if !a.quiet {
                for n in &unmerged_names {
                    eprintln!("warning: path '{n}' is unmerged");
                }
            }
            // Skip unmerged paths entirely (index + worktree untouched).
            skip = unmerged_names.iter().cloned().collect();
        } else if source_map.is_none() && !a.ours && !a.theirs {
            // From the index with no stage selection: hard error.
            for n in &unmerged_names {
                eprintln!("error: path '{n}' is unmerged");
            }
            return Err(CommandError::silent(1));
        }
        // With a source tree the unmerged entries were already replaced.
    }

    // --ours/--theirs without a source tree need explicit paths.
    if (a.ours || a.theirs) && paths.is_empty() {
        return Err(CommandError::fatal("fatal: '--ours/--theirs' needs the paths to check out"));
    }

    let work_tree = repo
        .work_tree
        .clone()
        .ok_or_else(|| CommandError::fatal("fatal: this operation must be run in a work tree"))?;
    let filemode = repo.config.get_bool("core", "filemode").unwrap_or(true);
    let symlinks = repo.config.get_bool("core", "symlinks").unwrap_or(true);
    let matched_here = |path: &str| {
        !skip.contains(path) && paths.iter().any(|s| checkout_core::spec_matches(s, path))
    };

    // Resolve the blob for each path to write.
    let mut stages: HashMap<&str, Vec<(u8, u32, git_hash::Oid)>> = HashMap::new();
    for e in index.entries.iter() {
        if e.stage != 0 && matched_here(&e.name) {
            stages.entry(e.name.as_str()).or_default().push((e.stage, e.mode, e.oid));
        }
    }
    // Stage selection: --theirs wins when both are given (matches C's
    // single writeout_stage int, last option wins — parse order ours first).
    let want_stage = if a.theirs { Some(3u8) } else if a.ours { Some(2u8) } else { None };
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
    for e in index.entries.iter().filter(|e| e.stage == 0 && matched_here(&e.name)) {
        if !staged_names.contains(e.name.as_str()) {
            to_write.push((e.name.clone(), e.mode, e.oid));
        }
    }

    let mut written = 0usize;
    for (name, mode, oid) in &to_write {
        let data = odb.read(oid).map(|o| o.data).unwrap_or_default();
        let blob = checkout_core::BlobInfo { mode: *mode, oid: *oid, data };
        checkout_core::write_one_to_worktree(&work_tree, name, &blob, filemode, symlinks)?;
        written += 1;
    }

    // Refresh stat for written entries (C's refresh_cache after checkout).
    for e in index.entries.iter_mut() {
        if e.stage == 0 && matched_here(&e.name) && to_write.iter().any(|(n, _, _)| n == &e.name) {
            if let Ok(md) = std::fs::symlink_metadata(work_tree.join(&e.name)) {
                checkout_core::fill_stat(e, &md);
            }
        }
    }
    write_index(&repo, &index)?;

    // "Updated N path(s) ..." only when no "--" was given and not quiet.
    if !a.quiet && !a.dashdash {
        let print = match &source_label {
            Some(_) => true,
            None => unmerged_names.is_empty() || written > 0,
        };
        if print {
            let label = source_label.as_deref().unwrap_or("the index");
            if written == 1 {
                eprintln!("Updated 1 path from {label}");
            } else {
                eprintln!("Updated {written} paths from {label}");
            }
        }
    }
    let _ = out;
    Ok(())
}
