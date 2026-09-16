//! `git commit`: record changes to the repository.
//!
//! Port of the non-hook, non-signing core of `builtin/commit.c`:
//! `-m`/`-F` messages, `-a`, `--amend`, `--allow-empty[-message]`, `--author`,
//! `--date`, `--signoff`, `-q`, `--cleanup=<mode>`, the default "whitespace"
//! message cleanup, tree construction from the index, ref + reflog updates, and
//! git's `[<branch> <short>] <subject>` / diffstat summary.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use crate::ident;
use crate::treeobj::{build, insert_path, TreeNode};
use crate::{Command, CommandError, RepoContext};
use git_diff::tree::compare_trees;
use git_hash::{HashAlgorithm, Oid};
use git_index::Index;
use git_object::{Object, ObjectKind};
use git_odb::{LooseStore, Odb};
use git_refs::RefStore;

pub struct Commit;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cleanup {
    Whitespace,
    Strip,
    Verbatim,
    Scissors,
}

impl Cleanup {
    fn parse(s: &str) -> Option<Cleanup> {
        match s {
            "whitespace" => Some(Cleanup::Whitespace),
            "strip" => Some(Cleanup::Strip),
            "verbatim" => Some(Cleanup::Verbatim),
            "scissors" => Some(Cleanup::Scissors),
            "default" => None,
            _ => None,
        }
    }
}

struct Args {
    all: bool,
    messages: Vec<String>,
    file: Option<String>,
    amend: bool,
    allow_empty: bool,
    allow_empty_message: bool,
    quiet: bool,
    author: Option<String>,
    date: Option<String>,
    signoff: bool,
    no_edit: bool,
    cleanup: Option<Cleanup>,
    paths: Vec<String>,
}

impl Args {
    fn parse(args: &[String]) -> Result<Args, CommandError> {
        let mut a = Args {
            all: false,
            messages: Vec::new(),
            file: None,
            amend: false,
            allow_empty: false,
            allow_empty_message: false,
            quiet: false,
            author: None,
            date: None,
            signoff: false,
            no_edit: false,
            cleanup: None,
            paths: Vec::new(),
        };
        let mut after_dd = false;
        let mut i = 0usize;
        while i < args.len() {
            let arg = &args[i];
            if after_dd {
                a.paths.push(arg.clone());
                i += 1;
                continue;
            }
            let (name, inline) = match arg.split_once('=') {
                Some((n, v)) if arg.starts_with("--") => (n.to_string(), Some(v.to_string())),
                _ => (arg.clone(), None),
            };
            let need = |flag: &str, i: &mut usize| -> Result<String, CommandError> {
                if let Some(v) = &inline {
                    return Ok(v.clone());
                }
                *i += 1;
                args.get(*i)
                    .cloned()
                    .ok_or_else(|| CommandError::usage(format!("option `{flag}' requires a value")))
            };
            match name.as_str() {
                "-a" | "--all" => a.all = true,
                "-m" | "--message" => a.messages.push(need("m", &mut i)?),
                "-F" | "--file" => a.file = Some(need("F", &mut i)?),
                "--amend" => a.amend = true,
                "--allow-empty" => a.allow_empty = true,
                "--allow-empty-message" => a.allow_empty_message = true,
                "-q" | "--quiet" => a.quiet = true,
                "--author" => a.author = Some(need("author", &mut i)?),
                "--date" => a.date = Some(need("date", &mut i)?),
                "--signoff" | "-s" => a.signoff = true,
                "--no-edit" => a.no_edit = true,
                "-e" | "--edit" => a.no_edit = false,
                "--cleanup" => {
                    let v = need("cleanup", &mut i)?;
                    a.cleanup = Some(Cleanup::parse(&v).ok_or_else(|| {
                        CommandError::fatal(format!(
                            "fatal: Invalid cleanup mode {v}"
                        ))
                    })?);
                }
                "--no-verify" | "-n" | "--no-status" | "--no-post-rewrite" | "--no-gpg-sign" => {}
                "--" => after_dd = true,
                s if s.starts_with('-') && s.len() > 1 && s != "-" => {
                    // Bundled short options (e.g. `-am "msg"`).
                    let chars: Vec<char> = s[1..].chars().collect();
                    let mut j = 0usize;
                    while j < chars.len() {
                        match chars[j] {
                            'a' => a.all = true,
                            'q' => a.quiet = true,
                            's' => a.signoff = true,
                            'n' | 'e' => {}
                            'v' => {}
                            c @ ('m' | 'F') => {
                                let rest: String = chars[j + 1..].iter().collect();
                                let val = if !rest.is_empty() {
                                    rest
                                } else {
                                    i += 1;
                                    args.get(i).cloned().ok_or_else(|| {
                                        CommandError::usage(format!("option `{c}' requires a value"))
                                    })?
                                };
                                if c == 'm' {
                                    a.messages.push(val);
                                } else {
                                    a.file = Some(val);
                                }
                                break;
                            }
                            c => {
                                return Err(CommandError::usage(format!(
                                    "error: unknown switch `{c}'\nusage: git commit [<options>] [--] <pathspec>..."
                                )));
                            }
                        }
                        j += 1;
                    }
                }
                s => a.paths.push(s.to_string()),
            }
            i += 1;
        }
        Ok(a)
    }
}

impl Command for Commit {
    fn name(&self) -> &'static str {
        "commit"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let a = Args::parse(args)?;
        if !a.paths.is_empty() {
            return Err(CommandError::fatal("fatal: partial commit (pathspecs) is not supported yet"));
        }

        let repo = ctx.repository()?;
        let algo = repo.hash_algo;
        if repo.work_tree.is_none() {
            return Err(CommandError::fatal("fatal: this operation must be run in a work tree"));
        }
        let store = LooseStore::from_repo(&repo);
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;

        let mut index = Index::read(&repo.index_file(), algo).unwrap_or_default();
        if index.entries.iter().any(|e| e.stage != 0) {
            return Err(CommandError::fatal("fatal: cannot commit with unmerged paths"));
        }

        // `-a`: stage tracked modifications/deletions into the index exactly
        // like `git add -u` (which also writes the blobs and refreshes stat).
        if a.all {
            let mut sink: Vec<u8> = Vec::new();
            crate::add::Add.run(ctx, &["-u".to_string()], &mut sink)?;
            index = Index::read(&repo.index_file(), algo).unwrap_or_default();
        }

        // Build the tree from the (stage 0) index.
        let mut root: BTreeMap<String, TreeNode> = BTreeMap::new();
        for e in &index.entries {
            let comps: Vec<&str> = e.name.split('/').collect();
            insert_path(&mut root, &comps, e.mode, e.oid);
        }
        let (tree_oid, _) = build(&root, Vec::new(), algo, Some(&store)).map_err(CommandError::from)?;

        // Resolve HEAD.
        let refs = RefStore::from_repo(&repo);
        let head_oid = repo.resolve_head();
        let head_symref = refs.head_symbolic_target();

        if a.amend && head_oid.is_none() {
            return Err(CommandError::fatal(
                "fatal: You have nothing to amend.",
            ));
        }

        let head_commit = head_oid.and_then(|oid| {
            odb.read(&oid)
                .ok()
                .and_then(|o| git_pretty::CommitInfo::parse(oid, &o.data, algo))
        });

        // Nothing-to-commit detection.
        if !a.amend && !a.allow_empty {
            let same_tree = head_commit.as_ref().map(|c| c.tree == tree_oid).unwrap_or(false);
            let empty_repo = head_oid.is_none() && index.entries.is_empty()
                && crate::worktree::untracked_and_ignored(&repo, &index, false, true).0.is_empty();
            if same_tree || empty_repo {
                let branch = head_symref
                    .as_deref()
                    .map(short_branch)
                    .unwrap_or_else(|| "HEAD".to_string());
                print_nothing_to_commit(out, &repo, &index, algo, &branch, head_oid.is_none());
                return Err(CommandError::silent(1));
            }
        }

        // Message.
        let raw_message = resolve_message(&repo, &a, head_commit.as_ref())?;
        let cleanup = a
            .cleanup
            .unwrap_or(Cleanup::Whitespace);
        let message = cleanup_message(&raw_message, cleanup);
        if message.trim().is_empty() && !a.allow_empty_message {
            return Err(CommandError::error("Aborting commit due to empty commit message."));
        }

        // Author / committer.
        let author = resolve_author(&repo, &a, head_commit.as_ref())?;
        let committer = ident::user_ident(&repo, false)?;

        // Parents.
        let parents: Vec<Oid> = if a.amend {
            head_commit.as_ref().map(|c| c.parents.clone()).unwrap_or_default()
        } else {
            head_oid.into_iter().collect()
        };

        let mut content = format!("tree {tree_oid}\n");
        for p in &parents {
            content.push_str(&format!("parent {p}\n"));
        }
        content.push_str(&format!("author {author}\ncommitter {committer}\n\n{message}"));
        let obj = Object::from_data(ObjectKind::Commit, content.into_bytes());
        let new_oid = store.write(&obj).map_err(CommandError::from)?;

        // Update the ref HEAD points at (or HEAD itself when detached).
        let ref_name = head_symref.clone().unwrap_or_else(|| "HEAD".to_string());
        refs.update(&ref_name, Some(&new_oid))
            .map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;

        // Reflog (HEAD + branch), when logallrefupdates allows.
        let subject = first_line(&message);
        let action = {
        let action = if a.amend {
            format!("commit (amend): {subject}")
        } else if head_oid.is_none() {
            format!("commit (initial): {subject}")
        } else {
            format!("commit: {subject}")
        };
        action.trim_end().to_string()
        };
        if should_log_refs(&repo) {
            let old = head_oid.unwrap_or(*algo.null_oid());
            // HEAD is always logged; the branch ref's reflog is only written
            // when its value actually changes (C skips a no-op ref update).
            append_reflog(&repo.git_dir.join("logs/HEAD"), &old, &new_oid, &committer, &action);
            if let Some(sym) = &head_symref {
                if old != new_oid {
                    append_reflog(
                        &repo.git_dir.join("logs").join(sym),
                        &old,
                        &new_oid,
                        &committer,
                        &action,
                    );
                }
            }
        }

        // Summary output.
        if !a.quiet {
            let branch = head_symref
                .as_deref()
                .map(short_branch)
                .unwrap_or_else(|| "detached HEAD".to_string());
            let root = if head_oid.is_none() { " (root-commit)" } else { "" };
            writeln!(out, "[{branch}{root} {}] {subject}", short_oid(&new_oid))
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            if let (Some(author_id), Some(committer_id)) =
                (git_pretty::Ident::parse(&author), git_pretty::Ident::parse(&committer))
            {
                if author_id.name != committer_id.name || author_id.email != committer_id.email {
                    writeln!(out, " Author: {} <{}>", author_id.name, author_id.email).ok();
                }
                if a.amend || a.date.is_some() {
                    writeln!(out, " Date: {}", author_id.ts.format_git_default()).ok();
                }
            }
            // For `--amend` the summary compares against the amended commit's
            // first parent (C's `commit` diffstat), otherwise against HEAD.
            let base_tree = if a.amend {
                head_commit
                    .as_ref()
                    .and_then(|c| c.parents.first())
                    .and_then(|p| odb.read(p).ok())
                    .and_then(|o| git_pretty::CommitInfo::parse(*head_commit.as_ref().unwrap().parents.first().unwrap(), &o.data, algo))
                    .map(|c| c.tree)
            } else {
                head_commit.as_ref().map(|c| c.tree)
            };
            let stat = diffstat(&odb, base_tree, tree_oid, algo);
            out.write_all(stat.as_bytes()).map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}

/// Print git's "nothing to commit" report (status-like blocks + final line).
fn print_nothing_to_commit(
    out: &mut dyn Write,
    repo: &git_core::Repository,
    index: &Index,
    algo: HashAlgorithm,
    branch: &str,
    initial: bool,
) {
    let work_tree = repo.work_tree.clone().unwrap_or_default();
    let filemode = repo.config.get_bool("core", "filemode").unwrap_or(true);
    let unstaged = crate::worktree::unstaged_changes(&work_tree, index, algo, filemode);
    let untracked = crate::worktree::untracked_and_ignored(repo, index, false, true).0;

    writeln!(out, "On branch {branch}").ok();
    if initial {
        writeln!(out).ok();
        writeln!(out, "Initial commit").ok();
    }
    if !unstaged.is_empty() {
        writeln!(out, "Changes not staged for commit:").ok();
        if unstaged.iter().any(|(c, _)| *c == 'D') {
            writeln!(out, "  (use \"git add/rm <file>...\" to update what will be committed)").ok();
        } else {
            writeln!(out, "  (use \"git add <file>...\" to update what will be committed)").ok();
        }
        writeln!(out, "  (use \"git restore <file>...\" to discard changes in working directory)").ok();
        for (c, p) in &unstaged {
            let label = if *c == 'D' { "deleted:" } else { "modified:" };
            writeln!(out, "\t{label:<12}{p}").ok();
        }
    }
    if !untracked.is_empty() {
        if !unstaged.is_empty() || initial {
            writeln!(out).ok();
        }
        writeln!(out, "Untracked files:").ok();
        writeln!(out, "  (use \"git add <file>...\" to include in what will be committed)").ok();
        for p in &untracked {
            writeln!(out, "\t{p}").ok();
        }
    }
    if !unstaged.is_empty() || !untracked.is_empty() || initial {
        writeln!(out).ok();
    }
    if unstaged.is_empty() && untracked.is_empty() {
        if initial {
            writeln!(out, "nothing to commit (create/copy files and use \"git add\" to track)").ok();
        } else {
            writeln!(out, "nothing to commit, working tree clean").ok();
        }
    } else if unstaged.is_empty() {
        writeln!(out, "nothing added to commit but untracked files present (use \"git add\" to track)").ok();
    } else {
        writeln!(out, "no changes added to commit (use \"git add\" and/or \"git commit -a\")").ok();
    }
}

/// The message from `-m`/`-F`/editor (or HEAD for `--amend --no-edit`).
fn resolve_message(
    repo: &git_core::Repository,
    a: &Args,
    head: Option<&git_pretty::CommitInfo>,
) -> Result<String, CommandError> {
    if !a.messages.is_empty() {
        return Ok(a.messages.join("\n\n"));
    }
    if let Some(f) = &a.file {
        if f == "-" {
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            return Ok(buf);
        }
        return std::fs::read_to_string(f)
            .map_err(|e| CommandError::fatal(format!("fatal: could not read log file '{f}': {e}")));
    }
    if a.amend && a.no_edit {
        if let Some(h) = head {
            return Ok(String::from_utf8_lossy(&h.message).into_owned());
        }
    }
    // Editor path: only supported when an editor is configured (tests use -m).
    let editor = repo
        .get("core", "editor")
        .map(str::to_string)
        .or_else(|| std::env::var("GIT_EDITOR").ok())
        .or_else(|| std::env::var("EDITOR").ok());
    let Some(editor) = editor else {
        return Err(CommandError::fatal(
            "fatal: no commit message given (use -m/-F; interactive editor not supported yet)",
        ));
    };
    let msg_path = repo.git_dir.join("COMMIT_EDITMSG");
    let initial = head
        .filter(|_| a.amend)
        .map(|h| String::from_utf8_lossy(&h.message).into_owned())
        .unwrap_or_default();
    std::fs::write(&msg_path, &initial).map_err(|e| CommandError::fatal(e.to_string()))?;
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$0\""))
        .arg(&msg_path)
        .status()
        .map_err(|e| CommandError::fatal(format!("fatal: unable to run editor: {e}")))?;
    if !status.success() {
        return Err(CommandError::fatal("fatal: editor exited with a failure"));
    }
    std::fs::read_to_string(&msg_path).map_err(|e| CommandError::fatal(e.to_string()))
}

/// Apply C's message cleanup.
fn cleanup_message(msg: &str, mode: Cleanup) -> String {
    match mode {
        Cleanup::Verbatim => {
            if msg.is_empty() {
                msg.to_string()
            } else if msg.ends_with('\n') {
                msg.to_string()
            } else {
                format!("{msg}\n")
            }
        }
        Cleanup::Whitespace | Cleanup::Strip | Cleanup::Scissors => {
            let mut lines: Vec<String> = Vec::new();
            for line in msg.lines() {
                if (mode == Cleanup::Strip || mode == Cleanup::Scissors) && line.starts_with('#') {
                    continue;
                }
                lines.push(line.trim_end().to_string());
            }
            while lines.first().map(|l| l.is_empty()).unwrap_or(false) {
                lines.remove(0);
            }
            while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
                lines.pop();
            }
            if lines.is_empty() {
                String::new()
            } else {
                format!("{}\n", lines.join("\n"))
            }
        }
    }
}

/// The author ident, honoring `--author`, `--date`, and `--amend` (keep HEAD's).
fn resolve_author(
    repo: &git_core::Repository,
    a: &Args,
    head: Option<&git_pretty::CommitInfo>,
) -> Result<String, CommandError> {
    // Start from the resolved identity line.
    let mut base = ident::user_ident(repo, true)?;
    if a.amend && a.author.is_none() && a.date.is_none() {
        if let Some(h) = head {
            return Ok(format!("{} <{}> {}", h.author.name, h.author.email, h.author.ts.format_raw()));
        }
    }
    // Split "Name <email> <ts> <tz>".
    let lt = base.find('<').unwrap_or(base.len());
    let name = base[..lt].trim_end().to_string();
    let gt = base[lt..].find('>').map(|i| i + lt).unwrap_or(base.len());
    let email = if lt < base.len() { base[lt + 1..gt].to_string() } else { String::new() };
    let tail = base[gt + 1..].trim().to_string(); // "<ts> <tz>"

    let (name, email) = if let Some(author) = &a.author {
        let lt = author.find('<').ok_or_else(|| {
            CommandError::fatal(format!("fatal: invalid author '{author}'"))
        })?;
        let gt = author[lt..].find('>').map(|i| i + lt).ok_or_else(|| {
            CommandError::fatal(format!("fatal: invalid author '{author}'"))
        })?;
        (author[..lt].trim_end().to_string(), author[lt + 1..gt].to_string())
    } else {
        (name, email)
    };

    let tail = if let Some(date) = &a.date {
        let now = ident::now_utc();
        let ts = git_date::parse(date, now)
            .map_err(|e| CommandError::fatal(format!("fatal: invalid date '{date}': {e}")))?;
        ts.format_raw()
    } else {
        tail
    };
    base = format!("{name} <{email}> {tail}");
    Ok(base)
}

/// Render git's commit summary diffstat between `old` (or empty) and `new`.
fn diffstat(odb: &Odb, old_tree: Option<Oid>, new_tree: Oid, algo: HashAlgorithm) -> String {
    let empty_tree = *algo.empty_tree();
    let old = old_tree.unwrap_or(empty_tree);
    let load = |oid: &Oid| -> Option<Object> {
        if *oid == empty_tree || *oid == *algo.empty_tree() {
            Some(Object::from_data(ObjectKind::Tree, Vec::new()))
        } else {
            odb.read(oid).ok()
        }
    };
    let mut loader = |oid: &Oid| load(oid);
    let old_obj = load(&old).and_then(|o| git_object::parse_tree(&o.data, algo).ok()).unwrap_or_default();
    let new_obj = load(&new_tree).and_then(|o| git_object::parse_tree(&o.data, algo).ok()).unwrap_or_default();
    let changes = compare_trees(&old_obj, &new_obj, "", true, &mut loader);

    let mut files = 0usize;
    let mut ins = 0usize;
    let mut del = 0usize;
    let mut specials: Vec<String> = Vec::new();
    for c in &changes {
        files += 1;
        match c.status {
            'A' => {
                if let Some(oid) = c.new_oid {
                    ins += blob_line_count(odb, &oid);
                }
                specials.push(format!(" create mode {} {}", c.new_mode.clone().unwrap_or_default(), c.path));
            }
            'D' => {
                if let Some(oid) = c.old_oid {
                    del += blob_line_count(odb, &oid);
                }
                specials.push(format!(" delete mode {} {}", c.old_mode.clone().unwrap_or_default(), c.path));
            }
            'T' => {
                if let (Some(o), Some(n)) = (c.old_oid, c.new_oid) {
                    let (i, d) = blob_change_counts(odb, &o, &n);
                    ins += i;
                    del += d;
                }
                specials.push(format!(
                    " mode change {} {} {}",
                    c.old_mode.clone().unwrap_or_default(),
                    format_args!("{} =>", c.new_mode.clone().unwrap_or_default()),
                    c.path
                ));
            }
            _ => {
                if c.old_mode != c.new_mode && c.old_oid == c.new_oid {
                    specials.push(format!(
                        " mode change {} => {} {}",
                        c.old_mode.clone().unwrap_or_default(),
                        c.new_mode.clone().unwrap_or_default(),
                        c.path
                    ));
                } else if let (Some(o), Some(n)) = (c.old_oid, c.new_oid) {
                    let (i, d) = blob_change_counts(odb, &o, &n);
                    ins += i;
                    del += d;
                }
            }
        }
    }

    let mut s = String::new();
    if files == 0 {
        return s;
    }
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    s.push_str(&format!(" {files} file{} changed", plural(files)));
    if ins > 0 {
        s.push_str(&format!(", {ins} insertion{}(+)", plural(ins)));
    }
    if del > 0 {
        s.push_str(&format!(", {del} deletion{}(-)", plural(del)));
    }
    if ins == 0 && del == 0 {
        // C prints the counts even when zero for mode-only changes.
        s.push_str(", 0 insertions(+), 0 deletions(-)");
    }
    s.push('\n');
    specials.sort();
    for line in specials {
        s.push_str(&line);
        s.push('\n');
    }
    s
}

fn blob_line_count(odb: &Odb, oid: &Oid) -> usize {
    odb.read(oid)
        .map(|o| git_diff::myers::split_lines(&o.data).len())
        .unwrap_or(0)
}

fn blob_change_counts(odb: &Odb, old: &Oid, new: &Oid) -> (usize, usize) {
    let a = odb.read(old).map(|o| o.data).unwrap_or_default();
    let b = odb.read(new).map(|o| o.data).unwrap_or_default();
    let la = git_diff::myers::split_lines(&a);
    let lb = git_diff::myers::split_lines(&b);
    let ops = git_diff::myers::diff(&la, &lb);
    let ins = ops.iter().filter(|o| **o == git_diff::myers::Op::Insert).count();
    let del = ops.iter().filter(|o| **o == git_diff::myers::Op::Delete).count();
    (ins, del)
}

fn short_branch(refname: &str) -> String {
    refname.strip_prefix("refs/heads/").unwrap_or(refname).to_string()
}

fn short_oid(oid: &Oid) -> String {
    let hex = oid.to_string();
    hex.chars().take(7).collect()
}

fn first_line(msg: &str) -> String {
    msg.lines().next().unwrap_or("").to_string()
}

fn should_log_refs(repo: &git_core::Repository) -> bool {
    repo.config.get_bool("core", "logallrefupdates").unwrap_or(!repo.bare)
}

/// Append one line to a reflog file (creating parent directories).
fn append_reflog(path: &PathBuf, old: &Oid, new: &Oid, ident_line: &str, message: &str) {
    // Split "Name <email> ts tz" into "Name <email>" and " ts tz".
    let (who, when) = match ident_line.find('>') {
        Some(gt) => (&ident_line[..=gt], ident_line[gt + 1..].trim()),
        None => (ident_line, ""),
    };
    let line = format!("{old} {new} {who} {when}\t{message}\n");
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}
