//! `git status`: report index/worktree state.
//!
//! Port of `builtin/commit.c`'s `wt_status` output for the common cases:
//! the default long format (staged / not-staged / untracked / ignored blocks
//! with C's hint lines and labels), `--short`/`-s`, `--porcelain[=v1]`,
//! `-b`/`--branch`, `-z`, `--ignored`, and `--untracked-files`.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;

use crate::worktree;
use crate::{Command, CommandError, RepoContext};
use git_hash::Oid;
use git_index::Index;
use git_object::{parse_commit, parse_tree, ObjectKind};
use git_odb::Odb;
use git_refs::RefStore;

pub struct Status;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Long,
    Short,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UntrackedMode {
    No,
    Normal,
    All,
}

struct Args {
    format: Format,
    branch: bool,
    nul: bool,
    ignored: bool,
    untracked: UntrackedMode,
}

impl Args {
    fn parse(args: &[String]) -> Result<Args, CommandError> {
        let mut a = Args {
            format: Format::Long,
            branch: false,
            nul: false,
            ignored: false,
            untracked: UntrackedMode::Normal,
        };
        let mut i = 0usize;
        while i < args.len() {
            let arg = &args[i];
            match arg.as_str() {
                "--porcelain" | "--porcelain=v1" | "--short" | "-s" => a.format = Format::Short,
                "--long" => a.format = Format::Long,
                "-b" | "--branch" => a.branch = true,
                "--no-branch" => a.branch = false,
                "-z" | "--null" => {
                    a.nul = true;
                    a.format = Format::Short;
                }
                "-v" | "--verbose" => {}
                "-u" | "--untracked-files" => a.untracked = UntrackedMode::Normal,
                "-uno" | "--untracked-files=no" => a.untracked = UntrackedMode::No,
                "-unormal" | "--untracked-files=normal" => a.untracked = UntrackedMode::Normal,
                "-uall" | "--untracked-files=all" => a.untracked = UntrackedMode::All,
                "--ignored" | "--ignored=traditional" => a.ignored = true,
                "--ignored=matching" => a.ignored = true,
                "--ignored=no" | "--no-ignored" => a.ignored = false,
                "--no-renames" | "--renames" | "--ahead-behind" | "--no-ahead-behind" | "--show-stash" => {}
                s if s.starts_with('-') && !s.starts_with("--") && s.len() > 1 => {
                    let chars: Vec<char> = s[1..].chars().collect();
                    let mut j = 0usize;
                    while j < chars.len() {
                        match chars[j] {
                            's' => a.format = Format::Short,
                            'b' => a.branch = true,
                            'z' => {
                                a.nul = true;
                                a.format = Format::Short;
                            }
                            'v' => {}
                            'u' => {
                                let rest: String = chars[j + 1..].iter().collect();
                                a.untracked = match rest.as_str() {
                                    "" | "normal" => UntrackedMode::Normal,
                                    "no" => UntrackedMode::No,
                                    "all" => UntrackedMode::All,
                                    _ => {
                                        return Err(CommandError::usage(format!(
                                            "error: unknown option `{s}'\nusage: git status [<options>] [--] [<pathspec>...]"
                                        )));
                                    }
                                };
                                break;
                            }
                            c => {
                                return Err(CommandError::usage(format!(
                                    "error: unknown switch `{c}'\nusage: git status [<options>] [--] [<pathspec>...]"
                                )));
                            }
                        }
                        j += 1;
                    }
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!(
                        "error: unknown option `{s}'\nusage: git status [<options>] [--] [<pathspec>...]"
                    )));
                }
                _ => {}
            }
            i += 1;
        }
        Ok(a)
    }
}

impl Command for Status {
    fn name(&self) -> &'static str {
        "status"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let a = Args::parse(args)?;
        let repo = ctx.repository()?;
        let algo = repo.hash_algo;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let work_tree = repo
            .work_tree
            .clone()
            .ok_or_else(|| CommandError::fatal("fatal: this operation must be run in a work tree"))?;
        let filemode = repo.config.get_bool("core", "filemode").unwrap_or(true);
        let index = Index::read(&repo.index_file(), algo).unwrap_or_default();

        // HEAD tree (path -> (mode, oid)).
        let head_oid = repo.resolve_head();
        let mut head_tree: HashMap<String, (String, Oid)> = HashMap::new();
        if let Some(head) = head_oid {
            if let Ok(obj) = odb.read(&head) {
                if obj.kind == ObjectKind::Commit {
                    if let Ok(commit) = parse_commit(&obj.data, algo) {
                        flatten_tree(&odb, commit.tree, "", &mut head_tree);
                    }
                }
            }
        }
        let index_names: std::collections::HashSet<&str> =
            index.entries.iter().map(|e| e.name.as_str()).collect();

        // Staged (index vs HEAD): A/M/D/T.
        let mut staged: BTreeMap<String, char> = BTreeMap::new();
        for e in index.entries.iter().filter(|e| e.stage == 0) {
            let new_mode = format!("{:o}", e.mode);
            match head_tree.get(&e.name) {
                None => {
                    staged.insert(e.name.clone(), 'A');
                }
                Some((mode, oid)) => {
                    if *oid != e.oid {
                        staged.insert(e.name.clone(), if type_bits(mode) != type_bits(&new_mode) { 'T' } else { 'M' });
                    } else if *mode != new_mode {
                        staged.insert(e.name.clone(), 'M');
                    }
                }
            }
        }
        for (path, _) in &head_tree {
            if !index_names.contains(path.as_str()) {
                staged.insert(path.clone(), 'D');
            }
        }

        // Unstaged (worktree vs index).
        let unstaged: BTreeMap<String, char> = worktree::unstaged_changes(&work_tree, &index, algo, filemode)
            .into_iter()
            .map(|(c, p)| (p, c))
            .collect();

        // Untracked / ignored. Always learn whether untracked paths exist so the
        // long `nothing to commit` branch can match even when display is off.
        let (untracked, ignored) = worktree::untracked_and_ignored(
            &repo,
            &index,
            a.ignored,
            a.untracked != UntrackedMode::All,
        );
        let has_untracked = !untracked.is_empty();
        let untracked = if a.untracked == UntrackedMode::No {
            Vec::new()
        } else {
            untracked
        };

        // Unmerged (stage > 0).
        let mut unmerged: BTreeMap<String, (char, char)> = BTreeMap::new();
        {
            let mut stages: BTreeMap<String, [bool; 4]> = BTreeMap::new();
            for e in index.entries.iter().filter(|e| e.stage != 0) {
                stages.entry(e.name.clone()).or_insert([false; 4])[e.stage as usize] = true;
            }
            for (path, s) in stages {
                let xy = match (s[1], s[2], s[3]) {
                    (_, true, true) => ('U', 'U'),
                    (true, true, false) => ('A', 'A'),
                    (true, false, true) => ('D', 'U'),
                    (false, true, false) => ('A', 'U'),
                    (false, false, true) => ('U', 'A'),
                    (true, false, false) => ('D', 'D'),
                    _ => ('U', 'U'),
                };
                unmerged.insert(path, xy);
            }
        }

        // Paths are shown relative to the invocation directory, like C.
        let disp = |p: &str| display_path(&work_tree, &ctx.cwd, p);

        let branch = branch_label(&repo, head_oid);

        match a.format {
            Format::Short => {
                if a.branch {
                    write!(out, "## {branch}{}", if a.nul { "\0" } else { "\n" })
                        .map_err(|e| CommandError::fatal(e.to_string()))?;
                }
                let mut lines: Vec<(String, char, char)> = Vec::new();
                let mut paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
                paths.extend(staged.keys().cloned());
                paths.extend(unstaged.keys().cloned());
                paths.extend(unmerged.keys().cloned());
                for p in &paths {
                    let x = unmerged.get(p).map(|(x, _)| *x).or_else(|| staged.get(p).copied()).unwrap_or(' ');
                    let y = unmerged.get(p).map(|(_, y)| *y).or_else(|| unstaged.get(p).copied()).unwrap_or(' ');
                    lines.push((p.clone(), x, y));
                }
                for p in &untracked {
                    lines.push((p.clone(), '?', '?'));
                }
                if a.ignored {
                    for p in &ignored {
                        lines.push((p.clone(), '!', '!'));
                    }
                }
                for (p, x, y) in &lines {
                    if a.nul {
                        let dp = disp(&p);
                        out.write_all(format!("{x}{y} {dp}\0").as_bytes())
                            .map_err(|e| CommandError::fatal(e.to_string()))?;
                    } else {
                        writeln!(out, "{x}{y} {}", disp(&p)).map_err(|e| CommandError::fatal(e.to_string()))?;
                    }
                }
            }
            Format::Long => {
                writeln!(out, "On branch {branch}").map_err(|e| CommandError::fatal(e.to_string()))?;
                if head_oid.is_none() {
                    writeln!(out).ok();
                    writeln!(out, "No commits yet").ok();
                    writeln!(out).ok();
                }
                if !staged.is_empty() {
                    writeln!(out, "Changes to be committed:").ok();
                    if head_oid.is_none() {
                        writeln!(out, "  (use \"git rm --cached <file>...\" to unstage)").ok();
                    } else {
                        writeln!(out, "  (use \"git restore --staged <file>...\" to unstage)").ok();
                    }
                    for (p, c) in &staged {
                        writeln!(out, "\t{:<12}{}", staged_label(*c), disp(p)).ok();
                    }
                }
                if !unstaged.is_empty() {
                    if !staged.is_empty() || !unmerged.is_empty() {
                        writeln!(out).ok();
                    }
                    writeln!(out, "Changes not staged for commit:").ok();
                    if unstaged.values().any(|c| *c == 'D') {
                        writeln!(out, "  (use \"git add/rm <file>...\" to update what will be committed)").ok();
                    } else {
                        writeln!(out, "  (use \"git add <file>...\" to update what will be committed)").ok();
                    }
                    writeln!(out, "  (use \"git restore <file>...\" to discard changes in working directory)").ok();
                    for (p, c) in &unstaged {
                        writeln!(out, "\t{:<12}{}", staged_label(*c), disp(p)).ok();
                    }
                }
                if !unmerged.is_empty() {
                    if !staged.is_empty() {
                        writeln!(out).ok();
                    }
                    writeln!(out, "Unmerged paths:").ok();
                    writeln!(out, "  (use \"git add <file>...\" to mark resolution)").ok();
                    for (p, (x, y)) in &unmerged {
                        writeln!(out, "\t{:<12}{}", unmerged_label(*x, *y), disp(p)).ok();
                    }
                }
                if a.untracked != UntrackedMode::No && !untracked.is_empty() {
                    if !staged.is_empty() || !unstaged.is_empty() || !unmerged.is_empty() {
                        writeln!(out).ok();
                    }
                    writeln!(out, "Untracked files:").ok();
                    writeln!(out, "  (use \"git add <file>...\" to include in what will be committed)").ok();
                    for p in &untracked {
                        writeln!(out, "\t{}", disp(p)).ok();
                    }
                }
                if !ignored.is_empty() {
                    if !staged.is_empty() || !unstaged.is_empty() || !unmerged.is_empty() || !untracked.is_empty() {
                        writeln!(out).ok();
                    }
                    writeln!(out, "Ignored files:").ok();
                    writeln!(out, "  (use \"git add -f <file>...\" to include in what will be committed)").ok();
                    for p in &ignored {
                        writeln!(out, "\t{}", disp(p)).ok();
                    }
                }
                let show_notice = a.format == Format::Long
                    && a.untracked == UntrackedMode::No
                    && (!staged.is_empty() || !unmerged.is_empty());
                if show_notice
                    && (!staged.is_empty()
                        || !unstaged.is_empty()
                        || !unmerged.is_empty()
                        || !ignored.is_empty())
                {
                    writeln!(out).ok();
                }
                if show_notice {
                    writeln!(out, "Untracked files not listed (use -u option to show untracked files)").ok();
                }
                if staged.is_empty() && unmerged.is_empty() {
                    if !unstaged.is_empty() || !untracked.is_empty() || !ignored.is_empty() {
                        writeln!(out).ok();
                    }
                    if unstaged.is_empty() && untracked.is_empty() && !has_untracked {
                        if head_oid.is_none() {
                            writeln!(out, "nothing to commit (create/copy files and use \"git add\" to track)").ok();
                        } else {
                            writeln!(out, "nothing to commit, working tree clean").ok();
                        }
                    } else if !unstaged.is_empty() {
                        writeln!(out, "no changes added to commit (use \"git add\" and/or \"git commit -a\")").ok();
                    } else if !untracked.is_empty() {
                        writeln!(out, "nothing added to commit but untracked files present (use \"git add\" to track)").ok();
                    } else if head_oid.is_none() {
                        writeln!(out, "nothing to commit (create/copy files and use \"git add\" to track)").ok();
                    } else if a.untracked == UntrackedMode::No {
                        writeln!(out, "nothing to commit (use -u to show untracked files)").ok();
                    } else {
                        writeln!(out, "nothing added to commit but untracked files present (use \"git add\" to track)").ok();
                    }
                } else if !show_notice
                    && (!staged.is_empty()
                        || !unstaged.is_empty()
                        || !unmerged.is_empty()
                        || !untracked.is_empty()
                        || !ignored.is_empty())
                {
                    writeln!(out).ok();
                }
            }
        }
        Ok(())
    }
}

fn staged_label(c: char) -> &'static str {
    match c {
        'A' => "new file:",
        'D' => "deleted:",
        'T' => "typechange:",
        _ => "modified:",
    }
}

fn unmerged_label(x: char, y: char) -> &'static str {
    match (x, y) {
        ('U', 'U') => "both modified:",
        ('A', 'A') => "both added:",
        ('D', 'U') => "deleted by us:",
        ('A', 'U') => "added by us:",
        ('U', 'A') => "added by them:",
        ('D', 'D') => "both deleted:",
        _ => "both modified:",
    }
}

fn type_bits(mode: &str) -> u32 {
    u32::from_str_radix(mode, 8).unwrap_or(0) & 0o170000
}

/// Render a repo-relative path the way C does from the current directory:
/// relative to the cwd with `../` segments as needed (lexically normalized).
fn display_path(work_tree: &std::path::Path, cwd: &std::path::Path, path: &str) -> String {
    fn split(p: &str) -> Vec<&str> {
        p.split('/').filter(|s| !s.is_empty() && *s != ".").collect()
    }
    fn norm(comps: Vec<&str>) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for c in comps {
            if c == ".." {
                out.pop();
            } else {
                out.push(c);
            }
        }
        out
    }
    let (path, dir_suffix) = match path.strip_suffix('/') {
        Some(base) => (base, "/"),
        None => (path, ""),
    };
    let rel = match cwd.strip_prefix(work_tree) {
        Ok(rel) => rel,
        Err(_) => return format!("{path}{dir_suffix}"),
    };
    let rel_str = rel.to_string_lossy().into_owned().replace('\\', "/");
    let mut downs = norm(split(&rel_str));
    let mut ups = norm(split(path));
    while !downs.is_empty() && !ups.is_empty() && downs[0] == ups[0] {
        downs.remove(0);
        ups.remove(0);
    }
    let mut out = String::new();
    for _ in &downs {
        out.push_str("../");
    }
    out.push_str(&ups.join("/"));
    out.push_str(dir_suffix);
    out
}

fn branch_label(repo: &git_core::Repository, head_oid: Option<Oid>) -> String {
    let refs = RefStore::from_repo(repo);
    if let Some(target) = refs.head_symbolic_target() {
        if let Some(short) = target.strip_prefix("refs/heads/") {
            return short.to_string();
        }
    }
    if let Some(oid) = head_oid {
        let hex = oid.to_string();
        return format!("HEAD detached at {}", &hex[..7.min(hex.len())]);
    }
    "HEAD (no branch)".to_string()
}

fn flatten_tree(odb: &Odb, tree: Oid, prefix: &str, map: &mut HashMap<String, (String, Oid)>) {
    if let Ok(obj) = odb.read(&tree) {
        if let Ok(entries) = parse_tree(&obj.data, tree.algorithm()) {
            for e in &entries {
                let path = if prefix.is_empty() {
                    String::from_utf8_lossy(&e.name).into_owned()
                } else {
                    format!("{prefix}/{}", String::from_utf8_lossy(&e.name))
                };
                if e.is_dir() {
                    flatten_tree(odb, e.oid, &path, map);
                } else {
                    map.insert(path, (e.mode.clone(), e.oid));
                }
            }
        }
    }
}
