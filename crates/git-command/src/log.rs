//! `git log`: walk commits and show them via the pretty engine.

use std::collections::HashSet;
use std::io::Write;

use crate::{Command, CommandError, RepoContext};
use git_hash::Oid;
use git_object::{parse_commit, parse_tree, ObjectKind};
use git_odb::Odb;
use git_pretty::{CommitInfo, Format, Options};
use git_revision::rev_info::{walk_commits, RevOptions};

pub struct Log;

impl Command for Log {
    fn name(&self) -> &'static str {
        "log"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut format = Format::Medium;
        let mut date_mode = git_pretty::date::DateMode::Default;
        let mut tips: Vec<Oid> = Vec::new();
        let mut hidden: Vec<Oid> = Vec::new();
        let mut paths: Vec<String> = Vec::new();
        let mut negate = false;
        let mut after_dashdash = false;
        let mut opts = RevOptions::default();
        let repo = ctx.repository()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let algo = repo.hash_algo;

        let add_refs = |prefix: &str, tips: &mut Vec<Oid>, repo: &git_core::Repository| {
            let store = git_refs::RefStore::from_repo(repo);
            for (name, oid) in store.list() {
                if name.starts_with(prefix) {
                    tips.push(oid);
                }
            }
        };

        for a in args {
            let a = a.clone();
            if after_dashdash {
                paths.push(a);
                continue;
            }
            match a.as_str() {
                "--" => after_dashdash = true,
                "--oneline" => format = Format::UserTerminated("%h %s".to_string()),
                "--reverse" => opts.reverse = true,
                "--first-parent" => opts.first_parent = true,
                "--merges" => opts.min_parents = 2,
                "--no-merges" => opts.max_parents = Some(1),
                "--topo-order" => opts.order = git_revision::rev_info::Order::Topo,
                "--date-order" => opts.order = git_revision::rev_info::Order::Date,
                s if s.starts_with("--pretty=") => {
                    let spec = &s["--pretty=".len()..];
                    format = Format::parse(spec).ok_or_else(|| {
                        CommandError::fatal(format!("fatal: invalid --pretty format: {spec}"))
                    })?;
                }
                "--pretty" | "--format" => format = Format::Medium,
                s if s.starts_with("--format=") => {
                    format = Format::UserTerminated(s["--format=".len()..].to_string());
                }
                s if s.starts_with("--date=") => {
                    let spec = &s["--date=".len()..];
                    date_mode = git_pretty::date::DateMode::parse(spec).ok_or_else(|| {
                        CommandError::error(format!("fatal: invalid date format: {spec}"))
                    })?;
                }
                s if s.starts_with("-n") && s.len() > 2 => {
                    opts.max_count = Some(s[2..].parse().unwrap_or(0));
                }
                s if s.starts_with("--max-count=") => {
                    opts.max_count = Some(s["--max-count=".len()..].parse().unwrap_or(0));
                }
                s if s.starts_with("--skip=") => {
                    opts.skip = s["--skip=".len()..].parse().unwrap_or(0);
                }
                "--all" => add_refs("refs/", &mut tips, &repo),
                "--branches" => add_refs("refs/heads/", &mut tips, &repo),
                "--tags" => add_refs("refs/tags/", &mut tips, &repo),
                "--remotes" => add_refs("refs/remotes/", &mut tips, &repo),
                s if s.starts_with("--glob=") => {
                    let mut pat = s["--glob=".len()..].to_string();
                    if !pat.starts_with("refs/") {
                        pat = format!("refs/{pat}");
                    }
                    add_refs(&pat, &mut tips, &repo);
                }
                s if s.starts_with("--min-parents=") => {
                    opts.min_parents = s["--min-parents=".len()..].parse().unwrap_or(0);
                }
                s if s.starts_with("--max-parents=") => {
                    opts.max_parents = Some(s["--max-parents=".len()..].parse().unwrap_or(usize::MAX));
                }
                s if s.starts_with("--author=") => opts.authors.push(s["--author=".len()..].to_string()),
                s if s.starts_with("--committer=") => {
                    opts.committers.push(s["--committer=".len()..].to_string());
                }
                s if s.starts_with("--grep=") => opts.greps.push(s["--grep=".len()..].to_string()),
                "--invert-grep" => opts.invert_grep = true,
                "-i" | "--regexp-ignore-case" => opts.ignore_case = true,
                "--not" => negate = true,
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("log: option '{s}' not supported")));
                }
                s => {
                    if let Some(rest) = s.strip_prefix('^') {
                        let oid = crate::resolve_arg(&repo, rest)?;
                        hidden.push(oid);
                        continue;
                    }
                    if let Some((a, b)) = s.split_once("..") {
                        let symmetric = s.contains("...");
                        let (a, b) = if symmetric {
                            let mid = s.find("...").unwrap();
                            (&s[..mid], &s[mid + 3..])
                        } else {
                            (a, b)
                        };
                        let a_oid = if a.is_empty() {
                            crate::resolve_arg(&repo, "HEAD")?
                        } else {
                            crate::resolve_arg(&repo, a)?
                        };
                        let b_oid = if b.is_empty() {
                            crate::resolve_arg(&repo, "HEAD")?
                        } else {
                            crate::resolve_arg(&repo, b)?
                        };
                        if symmetric {
                            if let Some(base) = merge_base(&repo, &a_oid, &b_oid) {
                                hidden.push(base);
                            }
                            tips.push(a_oid);
                        } else {
                            hidden.push(a_oid);
                        }
                        tips.push(b_oid);
                        continue;
                    }
                    let oid = crate::resolve_arg(&repo, s)?;
                    if negate {
                        hidden.push(oid);
                        negate = false;
                    } else {
                        tips.push(oid);
                    }
                }
            }
        }

        if tips.is_empty() {
            tips.push(crate::resolve_arg(&repo, "HEAD")?);
        }

        let mut loader = |oid: &Oid| -> Option<git_object::Commit> {
            let obj = odb.read(oid).ok()?;
            if obj.kind != ObjectKind::Commit {
                return None;
            }
            parse_commit(&obj.data, algo).ok()
        };

        let mut ids = walk_commits(&mut loader, &tips, &hidden, &opts);

        // Path limiting with default history simplification: show a commit
        // when its tree differs from every parent restricted to the
        // pathspec (roots always qualify).
        if !paths.is_empty() {
            ids.retain(|oid| commit_touches_paths(&odb, oid, &paths, algo));
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut opts = Options {
            date: date_mode,
            abbrev: 7,
            color: false,
            now,
        };

        let resolver = git_revision::Resolver::new(&repo).ok();
        let mut first = true;
        for oid in &ids {
            let obj = odb.read(oid).map_err(|_| CommandError::error("bad commit"))?;
            let info = CommitInfo::parse(*oid, &obj.data, algo)
                .ok_or_else(|| CommandError::error("bad commit"))?;
            if let Some(resolver) = &resolver {
                opts.abbrev = resolver.unique_abbrev_len(oid, 7);
            }
            if !first && !format.is_oneline() {
                writeln!(out).map_err(|e| CommandError::fatal(e.to_string()))?;
            }
            first = false;
            git_pretty::format_commit(&format, &info, &opts, out)
                .map_err(|e| CommandError::error(e.to_string()))?;
        }
        Ok(())
    }
}

/// Merge base: the newest common ancestor (full-parent ancestry).
fn merge_base(repo: &git_core::Repository, a: &Oid, b: &Oid) -> Option<Oid> {
    let odb = Odb::from_repo(repo).ok()?;
    let algo = repo.hash_algo;
    let ancestors = |start: &Oid| -> Option<HashSet<Oid>> {
        let mut seen = HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(start.clone());
        while let Some(oid) = queue.pop_front() {
            if !seen.insert(oid.clone()) {
                continue;
            }
            let obj = odb.read(&oid).ok()?;
            if obj.kind != ObjectKind::Commit {
                continue;
            }
            if let Ok(c) = parse_commit(&obj.data, algo) {
                for p in c.parents {
                    queue.push_back(p);
                }
            }
        }
        Some(seen)
    };
    let a_set = ancestors(a)?;
    let b_set = ancestors(b)?;
    let mut common: Vec<(i64, Oid)> = a_set
        .intersection(&b_set)
        .filter_map(|oid| {
            odb.read(oid).ok().and_then(|o| {
                parse_commit(&o.data, algo).ok().and_then(|c| {
                    c.committer.and_then(|raw| {
                        raw.rsplit(' ')
                            .find(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
                            .and_then(|t| t.parse::<i64>().ok())
                            .map(|d| (d, oid.clone()))
                    })
                })
            })
        })
        .collect();
    common.sort_by(|x, y| y.0.cmp(&x.0));
    common.into_iter().next().map(|(_, oid)| oid)
}

/// Whether a commit changes any of `paths` relative to each parent
/// (default simplification: hidden only when treesame to all parents).
fn commit_touches_paths(odb: &Odb, oid: &Oid, paths: &[String], algo: git_hash::HashAlgorithm) -> bool {
    let obj = match odb.read(oid) {
        Ok(o) => o,
        Err(_) => return true,
    };
    let commit = match parse_commit(&obj.data, algo) {
        Ok(c) => c,
        Err(_) => return true,
    };
    let tree_changed = |parent: Option<&Oid>| -> bool {
        let old = parent
            .and_then(|p| odb.read(p).ok())
            .and_then(|o| parse_commit(&o.data, algo).ok())
            .map(|c| c.tree);
        tree_paths_differ(odb, old.as_ref(), &commit.tree, paths, algo)
    };
    if commit.parents.is_empty() {
        return tree_changed(None);
    }
    commit.parents.iter().all(|p| tree_changed(Some(p)))
}

/// Whether the set of paths (under the pathspec) differs between two trees.
fn tree_paths_differ(
    odb: &Odb,
    old: Option<&Oid>,
    new: &Oid,
    paths: &[String],
    algo: git_hash::HashAlgorithm,
) -> bool {
    if old == Some(new) {
        return false;
    }
    let mut changed = false;
    compare_trees(odb, old, Some(new), "", paths, algo, &mut changed);
    changed
}

fn compare_trees(
    odb: &Odb,
    old: Option<&Oid>,
    new: Option<&Oid>,
    prefix: &str,
    paths: &[String],
    algo: git_hash::HashAlgorithm,
    changed: &mut bool,
) {
    if *changed {
        return;
    }
    let in_scope = |p: &str| {
        paths.iter().any(|path| {
            let path = path.trim_end_matches('/');
            p == path || path.starts_with(&format!("{p}/")) || p.starts_with(&format!("{path}/"))
        })
    };
    let entries_of = |t: Option<&Oid>| -> Vec<(String, Oid, bool)> {
        let Some(t) = t else { return Vec::new() };
        let Ok(obj) = odb.read(t) else { return Vec::new() };
        let Ok(entries) = parse_tree(&obj.data, algo) else { return Vec::new() };
        entries
            .iter()
            .map(|e| {
                (
                    String::from_utf8_lossy(&e.name).into_owned(),
                    e.oid.clone(),
                    e.is_dir(),
                )
            })
            .collect()
    };
    let old_entries = entries_of(old);
    let new_entries = entries_of(new);

    let mut names: HashSet<String> = old_entries.iter().map(|(n, _, _)| n.clone()).collect();
    names.extend(new_entries.iter().map(|(n, _, _)| n.clone()));
    for name in names {
        let child = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if !in_scope(&child) {
            continue;
        }
        let o = old_entries.iter().find(|(n, _, _)| *n == name);
        let n = new_entries.iter().find(|(n, _, _)| *n == name);
        match (o, n) {
            (None, None) => {}
            (Some((_, oo, ot)), Some((_, no, nt))) => {
                if oo != no || ot != nt {
                    *changed = true;
                    return;
                }
                if *ot {
                    compare_trees(odb, Some(oo), Some(no), &child, paths, algo, changed);
                }
            }
            _ => {
                *changed = true;
                return;
            }
        }
    }
}

