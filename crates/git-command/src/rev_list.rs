//! `git rev-list`: list commit (and with `--objects`, tree/blob) object ids.

use std::collections::HashSet;
use std::io::Write;

use crate::{Command, CommandError, RepoContext};
use git_hash::Oid;
use git_object::{parse_commit, parse_tree, ObjectKind};
use git_odb::Odb;
use git_revision::rev_info::{walk_commits, RevOptions};

pub struct RevList;

impl Command for RevList {
    fn name(&self) -> &'static str {
        "rev-list"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut with_parents = false;
        let mut count = false;
        let mut objects = false;
        let mut no_walk = false;
        let mut opts = RevOptions::default();
        let mut tips: Vec<Oid> = Vec::new();
        let mut hidden: Vec<Oid> = Vec::new();
        let mut paths: Vec<String> = Vec::new();
        let mut path_limit = false;
        let mut negate = false;
        let mut i = 0;
        let repo = ctx.repository()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let algo = repo.hash_algo;

        // Ref-selection flags resolve to additional tips.
        let add_refs = |prefix: &str,
                            tips: &mut Vec<Oid>,
                            repo: &git_core::Repository,
                            odb: &Odb|
         -> Result<(), CommandError> {
            let store = git_refs::RefStore::from_repo(repo);
            for (name, oid) in store.list() {
                if name.starts_with(prefix) {
                    let _ = odb;
                    tips.push(oid);
                }
            }
            Ok(())
        };

        while i < args.len() {
            let a = args[i].clone();
            match a.as_str() {
                "--parents" => with_parents = true,
                "--count" => count = true,
                "--objects" | "--objects-edge" => objects = true,
                "--no-walk" | "--no-walk=sorted" | "--no-walk=unsorted" => no_walk = true,
                "--do-walk" => no_walk = false,
                "--all" => {
                    add_refs("refs/", &mut tips, &repo, &odb)?;
                }
                "--branches" => add_refs("refs/heads/", &mut tips, &repo, &odb)?,
                "--tags" => add_refs("refs/tags/", &mut tips, &repo, &odb)?,
                "--remotes" => add_refs("refs/remotes/", &mut tips, &repo, &odb)?,
                s if s.starts_with("--glob=") => {
                    let mut pat = s["--glob=".len()..].to_string();
                    if !pat.starts_with("refs/") {
                        pat = format!("refs/{pat}");
                    }
                    add_refs(&pat, &mut tips, &repo, &odb)?;
                }
                "--topo-order" => opts.order = git_revision::rev_info::Order::Topo,
                "--date-order" => opts.order = git_revision::rev_info::Order::Date,
                "--reverse" => opts.reverse = true,
                "--first-parent" => opts.first_parent = true,
                "--merges" => {
                    opts.min_parents = 2;
                }
                "--no-merges" => opts.max_parents = Some(1),
                s if s.starts_with("--min-parents=") => {
                    opts.min_parents = s["--min-parents=".len()..].parse().unwrap_or(0);
                }
                s if s.starts_with("--max-parents=") => {
                    let n = s["--max-parents=".len()..].parse().unwrap_or(usize::MAX);
                    opts.max_parents = Some(n);
                }
                "--no-merges-implicit" => {}
                s if s.starts_with("--max-count=") => {
                    opts.max_count = Some(s["--max-count=".len()..].parse().unwrap_or(0));
                }
                s if s.starts_with("-n") && s.len() > 2 => {
                    opts.max_count = Some(s[2..].parse().unwrap_or(0));
                }
                "-n" => {
                    i += 1;
                    let v = args.get(i).ok_or_else(|| CommandError::usage("rev-list: -n requires a value"))?;
                    opts.max_count = Some(v.parse().unwrap_or(0));
                }
                s if s.starts_with("--skip=") => {
                    opts.skip = s["--skip=".len()..].parse().unwrap_or(0);
                }
                "--not" => negate = true,
                s if s.starts_with("--author=") => opts.authors.push(s["--author=".len()..].to_string()),
                s if s.starts_with("--committer=") => {
                    opts.committers.push(s["--committer=".len()..].to_string());
                }
                s if s.starts_with("--grep=") => opts.greps.push(s["--grep=".len()..].to_string()),
                "--invert-grep" => opts.invert_grep = true,
                "-i" | "--regexp-ignore-case" => opts.ignore_case = true,
                "--" => {
                    path_limit = true;
                    for p in &args[i + 1..] {
                        paths.push(p.clone());
                    }
                    break;
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("rev-list: option '{s}' not supported")));
                }
                s => {
                    // Range forms: A..B / A...B, and ^rev exclusions.
                    if let Some(oid) = parse_range_or_rev(&repo, s, &mut tips, &mut hidden)? {
                        if negate {
                            hidden.push(oid);
                        } else {
                            tips.push(oid);
                        }
                    }
                }
            }
            i += 1;
        }

        if tips.is_empty() && hidden.is_empty() {
            return Err(CommandError::usage("rev-list: missing commit"));
        }

        let mut loader = |oid: &Oid| -> Option<git_object::Commit> {
            let obj = odb.read(oid).ok()?;
            if obj.kind != ObjectKind::Commit {
                return None;
            }
            parse_commit(&obj.data, algo).ok()
        };

        let mut ids: Vec<Oid> = if no_walk {
            let mut seen = HashSet::new();
            tips.into_iter().filter(|o| seen.insert(*o)).collect()
        } else {
            walk_commits(&mut loader, &tips, &hidden, &opts)
        };
        if path_limit && !paths.is_empty() {
            ids.retain(|oid| crate::log::commit_touches_paths(&odb, oid, &paths, algo));
        }

        if count {
            writeln!(out, "{}", ids.len()).map_err(|e| CommandError::fatal(e.to_string()))?;
            return Ok(());
        }

        for oid in &ids {
            if with_parents {
                let parents = loader(oid).map(|c| c.parents).unwrap_or_default();
                let line = std::iter::once(oid.to_string())
                    .chain(parents.iter().map(|p| p.to_string()))
                    .collect::<Vec<_>>()
                    .join(" ");
                writeln!(out, "{line}").map_err(|e| CommandError::fatal(e.to_string()))?;
            } else {
                writeln!(out, "{oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }

        // C git lists all commits first, then the tree/blob objects in
        // traversal order (deduplicated across commits).
        if objects {
            let mut seen = HashSet::new();
            for oid in &ids {
                if let Ok(obj) = odb.read(oid) {
                    if let Ok(commit) = parse_commit(&obj.data, algo) {
                        emit_tree(out, &odb, &commit.tree, "", algo, &mut seen)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Parse a revision argument that may be `A..B`, `A...B`, `^rev`, or a
/// plain rev; pushes/returns the appropriate oid.
fn parse_range_or_rev(
    repo: &git_core::Repository,
    s: &str,
    tips: &mut Vec<Oid>,
    hidden: &mut Vec<Oid>,
) -> Result<Option<Oid>, CommandError> {
    if let Some(rest) = s.strip_prefix('^') {
        let oid = crate::resolve_arg(repo, rest)?;
        hidden.push(oid);
        return Ok(None);
    }
    if let Some((a, b)) = s.split_once("...") {
        let a_oid = if a.is_empty() {
            crate::resolve_arg(repo, "HEAD")?
        } else {
            crate::resolve_arg(repo, a)?
        };
        let b_oid = if b.is_empty() {
            crate::resolve_arg(repo, "HEAD")?
        } else {
            crate::resolve_arg(repo, b)?
        };
        tips.push(a_oid);
        tips.push(b_oid);
        // Symmetric difference: exclude the merge base.
        if let Some(base) = merge_base(repo, &a_oid, &b_oid) {
            hidden.push(base);
        }
        return Ok(None);
    }
    if let Some((a, b)) = s.split_once("..") {
        let a_oid = if a.is_empty() {
            crate::resolve_arg(repo, "HEAD")?
        } else {
            crate::resolve_arg(repo, a)?
        };
        let b_oid = if b.is_empty() {
            crate::resolve_arg(repo, "HEAD")?
        } else {
            crate::resolve_arg(repo, b)?
        };
        hidden.push(a_oid);
        return Ok(Some(b_oid));
    }
    Ok(Some(crate::resolve_arg(repo, s)?))
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
                parse_commit(&o.data, algo)
                    .ok()
                    .and_then(|c| {
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
    common.sort_by(|a, b| b.0.cmp(&a.0));
    common.into_iter().next().map(|(_, oid)| oid)
}

fn emit_tree(
    out: &mut dyn Write,
    odb: &Odb,
    tree: &Oid,
    path: &str,
    algo: git_hash::HashAlgorithm,
    seen: &mut HashSet<Oid>,
) -> Result<(), CommandError> {
    if !seen.insert(tree.clone()) {
        return Ok(());
    }
    writeln!(out, "{tree} {path}").map_err(|e| CommandError::fatal(e.to_string()))?;
    let obj = odb.read(tree).map_err(|e| CommandError::error(format!("{tree}: {e}")))?;
    let entries = parse_tree(&obj.data, algo)
        .map_err(|e| CommandError::error(format!("{tree}: {e}")))?;
    for e in &entries {
        let name = String::from_utf8_lossy(&e.name);
        let child_path = if path.is_empty() {
            name.into_owned()
        } else {
            format!("{path}/{name}")
        };
        if e.is_dir() {
            emit_tree(out, odb, &e.oid, &child_path, algo, seen)?;
        } else if seen.insert(e.oid.clone()) {
            writeln!(out, "{} {}", e.oid, child_path)
                .map_err(|err| CommandError::fatal(err.to_string()))?;
        }
    }
    Ok(())
}
