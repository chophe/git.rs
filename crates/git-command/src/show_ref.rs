//! `git show-ref` and `git for-each-ref`: list references.

use std::io::Write;

use crate::{Command, CommandError, RepoContext};
use git_hash::Oid;
use git_odb::Odb;
use git_refs::RefStore;

pub struct ShowRef;

impl Command for ShowRef {
    fn name(&self) -> &'static str {
        "show-ref"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut quiet = false;
        let mut short = false;
        let mut verify = false;
        let mut heads_only = false;
        let mut tags_only = false;
        let mut patterns: Vec<String> = Vec::new();
        for a in args {
            match a.as_str() {
                "-q" | "--quiet" => quiet = true,
                "-s" => short = true,
                "--verify" => verify = true,
                "--heads" => heads_only = true,
                "--tags" => tags_only = true,
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("show-ref: unexpected argument '{s}'")));
                }
                s => patterns.push(s.to_string()),
            }
        }
        let repo = ctx.repository()?;
        let store = RefStore::from_repo(&repo);
        let refs = store.list();
        if verify && patterns.is_empty() {
            return Err(CommandError::fatal("fatal: --verify requires a reference"));
        }
        let mut shown = 0usize;
        // With `--verify` every pattern must match exactly; otherwise
        // patterns filter by exact-or-prefix match (C `show-ref`). The
        // verify path below prints; the main loop only counts there.
        for (name, oid) in &refs {
            if heads_only && !name.starts_with("refs/heads/") {
                continue;
            }
            if tags_only && !name.starts_with("refs/tags/") {
                continue;
            }
            let matched = if patterns.is_empty() {
                true
            } else if verify {
                patterns.iter().any(|p| p == name)
            } else {
                patterns.iter().any(|p| *name == *p || name.starts_with(p.as_str()))
            };
            if !matched {
                continue;
            }
            shown += 1;
            if !verify && !quiet {
                if short {
                    writeln!(out, "{oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
                } else {
                    writeln!(out, "{oid} {name}").map_err(|e| CommandError::fatal(e.to_string()))?;
                }
            }
        }
        if verify {
            for p in &patterns {
                // Exact list hit, or anything resolvable (onelevel
                // pseudorefs like PSEUDOREF never appear in the
                // refs/ listing but resolve fine, like C).
                let hit = refs.iter().find(|(n, _)| n == p).map(|(_, o)| *o);
                let oid = match hit {
                    Some(o) => Some(o),
                    None => crate::resolve_arg(&repo, p).ok(),
                };
                let Some(oid) = oid else {
                    if quiet {
                        return Err(CommandError::silent(1));
                    }
                    return Err(CommandError::fatal(format!("fatal: '{p}' - not a valid ref")));
                };
                if !quiet {
                    if short {
                        writeln!(out, "{oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
                    } else {
                        writeln!(out, "{oid} {p}").map_err(|e| CommandError::fatal(e.to_string()))?;
                    }
                }
            }
            return Ok(());
        }
        if shown == 0 {
            return Err(CommandError::silent(1));
        }
        Ok(())
    }
}

pub struct ForEachRef;

impl Command for ForEachRef {
    fn name(&self) -> &'static str {
        "for-each-ref"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut pattern: Option<String> = None;
        let mut format = "%(objectname) %(objecttype)\t%(refname)".to_string();
        for a in args {
            if let Some(f) = a.strip_prefix("--format=") {
                format = f.to_string();
            } else if a.starts_with('-') && a.len() > 1 {
                return Err(CommandError::usage(format!("for-each-ref: option '{a}' not supported")));
            } else {
                pattern = Some(a.clone());
            }
        }

        let repo = ctx.repository()?;
        let store = RefStore::from_repo(&repo);
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let refs = store.list();

        for (name, oid) in refs {
            if let Some(p) = &pattern {
                if !name.starts_with(p.as_str()) {
                    continue;
                }
            }
            let kind = odb
                .read(&oid)
                .map(|o| o.kind.as_str().to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            let line = format
                .replace("%(objectname)", &oid.to_string())
                .replace("%(objecttype)", &kind)
                .replace("%(refname)", &name);
            writeln!(out, "{line}").map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}

/// List refs under a prefix, printing the short name (used by `branch` and
/// `tag`). `mark_head` prefixes `* ` to the current branch.
pub fn list_short(
    ctx: &RepoContext,
    out: &mut dyn Write,
    prefix: &str,
    mark_head: bool,
) -> Result<(), CommandError> {
    let repo = ctx.repository()?;
    let store = RefStore::from_repo(&repo);
    let head_target = if mark_head {
        store.head_symbolic_target()
    } else {
        None
    };
    for (name, _oid) in store.list() {
        if !name.starts_with(prefix) {
            continue;
        }
        let short = name[prefix.len()..].to_string();
        if mark_head {
            if Some(&name) == head_target.as_ref() {
                writeln!(out, "* {short}").map_err(|e| CommandError::fatal(e.to_string()))?;
            } else {
                writeln!(out, "  {short}").map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        } else {
            writeln!(out, "{short}").map_err(|e| CommandError::fatal(e.to_string()))?;
        }
    }
    Ok(())
}

pub struct Branch;

impl Command for Branch {
    fn name(&self) -> &'static str {
        "branch"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let repo = ctx.repository()?;
        let store = git_refs::RefStore::from_repo(&repo);

        let mut delete = false;
        let mut rest: Vec<String> = Vec::new();
        for a in args {
            match a.as_str() {
                "-l" | "--list" | "-a" | "-r" => {}
                "-d" | "-D" => delete = true,
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("branch: option '{s}' not supported")));
                }
                s => rest.push(s.to_string()),
            }
        }

        if delete {
            if rest.len() != 1 {
                return Err(CommandError::usage("branch -d: requires <branchname>"));
            }
            let name = rest[0].trim_start_matches("refs/heads/").to_string();
            let full = format!("refs/heads/{name}");
            // Refuse to delete the checked-out branch (C
            // `delete_branches` worktree check).
            if store.head_symbolic_target().as_deref() == Some(full.as_str()) {
                let wt = repo
                    .work_tree
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| repo.git_dir.display().to_string());
                return Err(CommandError::error(format!(
                    "error: cannot delete branch '{name}' used by worktree at '{wt}'"
                )));
            }
            let old = store.resolve(&full).ok_or_else(|| {
                CommandError::error(format!("error: branch '{name}' not found"))
            })?;
            store
                .update(&full, None)
                .map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
            // Deleting a ref removes its reflog (C `refs_delete_ref`).
            git_refs::reflog::remove_log(&repo.git_dir, &full);
            writeln!(out, "Deleted branch {name} (was {}).", crate::checkout_core::short_oid(&repo, &old))
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            return Ok(());
        }

        if rest.is_empty() {
            return list_short(ctx, out, "refs/heads/", true);
        }
        if rest.len() > 2 {
            return Err(CommandError::usage("branch: too many arguments"));
        }
        // Create: refs/heads/<name> at the start point (default HEAD).
        let (start_rev, start_display) = match rest.get(1) {
            Some(s) => (s.clone(), s.clone()),
            None => (
                "HEAD".to_string(),
                match store.head_symbolic_target() {
                    Some(t) => t.strip_prefix("refs/heads/").unwrap_or(&t).to_string(),
                    None => "HEAD".to_string(),
                },
            ),
        };
        let target = crate::resolve_arg(&repo, &start_rev)?;
        let name = rest[0].trim_start_matches("refs/heads/").to_string();
        let full = format!("refs/heads/{name}");
        if git_refs::validate_refname(&full).is_err() {
            return Err(CommandError::fatal(format!(
                "fatal: '{name}' is not a valid branch name\nhint: See 'git help check-ref-format'\nhint: Disable this message with \"git config set advice.refSyntax false\""
            )));
        }
        if store.resolve(&full).is_some() {
            return Err(CommandError::fatal(format!("fatal: a branch named '{name}' already exists")));
        }
        let old = store.resolve(&full).unwrap_or(*repo.hash_algo.null_oid());
        store
            .update(&full, Some(&target))
            .map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
        let msg = format!("branch: Created from {start_display}");
        if let Ok(ident) = crate::checkout_core::committer_ident(&repo) {
            git_refs::reflog::log_update(&repo, &full, &old, &target, &ident, &msg);
        }
        Ok(())
    }
}

pub struct Tag;

impl Command for Tag {
    fn name(&self) -> &'static str {
        "tag"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let repo = ctx.repository()?;
        let store = git_refs::RefStore::from_repo(&repo);

        let mut delete = false;
        let mut annotated = false;
        let mut messages: Vec<String> = Vec::new();
        let mut list = false;
        let mut rest: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < args.len() {
            let a = &args[i];
            match a.as_str() {
                "-l" | "--list" => list = true,
                "-d" => delete = true,
                "-a" | "--annotate" => annotated = true,
                "-m" | "--message" => {
                    i += 1;
                    let v = args
                        .get(i)
                        .ok_or_else(|| CommandError::usage("tag: option 'm' requires a value"))?;
                    messages.push(v.clone());
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("tag: option '{s}' not supported")));
                }
                s => rest.push(s.to_string()),
            }
            i += 1;
        }

        if delete {
            if rest.len() != 1 {
                return Err(CommandError::usage("tag -d: requires <tagname>"));
            }
            let name = rest[0].trim_start_matches("refs/tags/").to_string();
            let full = format!("refs/tags/{name}");
            let old = store.resolve(&full).ok_or_else(|| {
                CommandError::error(format!("error: tag '{name}' not found."))
            })?;
            store
                .update(&full, None)
                .map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
            git_refs::reflog::remove_log(&repo.git_dir, &full);
            writeln!(out, "Deleted tag '{name}' (was {}).", crate::checkout_core::short_oid(&repo, &old))
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            return Ok(());
        }

        if rest.is_empty() || list {
            return list_pattern(ctx, out, "refs/tags/", &rest);
        }
        if rest.len() > 2 {
            return Err(CommandError::usage("tag: too many arguments"));
        }
        // Create: lightweight tag at HEAD (or the given object), or an
        // annotated tag object with `-a -m`.
        let target = if rest.len() > 1 {
            crate::resolve_arg(&repo, &rest[1])?
        } else {
            repo.resolve_head()
                .ok_or_else(|| CommandError::error("failed to resolve 'HEAD' as a valid ref"))?
        };
        let name = rest[0].trim_start_matches("refs/tags/").to_string();
        let full = format!("refs/tags/{name}");
        if store.resolve(&full).is_some() {
            return Err(CommandError::fatal(format!("fatal: tag '{name}' already exists")));
        }
        let new_oid = if annotated {
            let message = messages.join("\n\n");
            if message.trim().is_empty() {
                return Err(CommandError::fatal(
                    "fatal: no tag message given (use -m; interactive editor not supported yet)",
                ));
            }
            let odb = git_odb::Odb::from_repo(&repo).map_err(CommandError::from)?;
            let obj = odb.read(&target).map_err(|_| {
                CommandError::fatal(format!("fatal: unable to read object {target}"))
            })?;
            let tagger = crate::ident::user_ident(&repo, false)?;
            let content = format!(
                "object {target}\ntype {}\ntag {name}\ntagger {tagger}\n\n{}\n",
                obj.kind.as_str(),
                message.trim_end(),
            );
            let loose = git_odb::LooseStore::from_repo(&repo);
            loose
                .write(&git_object::Object::from_data(git_object::ObjectKind::Tag, content.into_bytes()))
                .map_err(CommandError::from)?
        } else {
            target
        };
        let old = *repo.hash_algo.null_oid();
        store
            .update(&full, Some(&new_oid))
            .map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
        if let Ok(ident) = crate::checkout_core::committer_ident(&repo) {
            let msg = tag_reflog_message(&repo, &new_oid);
            git_refs::reflog::log_update(&repo, &full, &old, &new_oid, &ident, &msg);
        }
        Ok(())
    }
}

/// List tags/branches under `prefix`, filtered by optional glob patterns
/// (C `tag --list <pattern>`).
fn list_pattern(
    ctx: &RepoContext,
    out: &mut dyn Write,
    prefix: &str,
    patterns: &[String],
) -> Result<(), CommandError> {
    let repo = ctx.repository()?;
    let store = git_refs::RefStore::from_repo(&repo);
    for (name, _oid) in store.list() {
        if !name.starts_with(prefix) {
            continue;
        }
        let short = name[prefix.len()..].to_string();
        if !patterns.is_empty()
            && !patterns.iter().any(|p| {
                git_attributes::wildmatch(p, &short, 0) == git_attributes::WM_MATCH
                    || git_attributes::wildmatch(p, &name, 0) == git_attributes::WM_MATCH
            })
        {
            continue;
        }
        writeln!(out, "{short}").map_err(|e| CommandError::fatal(e.to_string()))?;
    }
    Ok(())
}

/// The reflog message for a tag creation (C `create_reflog_msg`):
/// `$GIT_REFLOG_ACTION` verbatim when set, else
/// `tag: tagging <abbrev> (<subject>, <short-date>)` for commits
/// (`tree/blob/other-tag/unknown` variants for other objects).
fn tag_reflog_message(repo: &git_core::Repository, oid: &Oid) -> String {
    if std::env::var("GIT_REFLOG_ACTION").map(|v| !v.is_empty()).unwrap_or(false) {
        return crate::checkout_core::reflog_action(String::new());
    }
    let abbrev = crate::checkout_core::short_oid(repo, oid);
    let Ok(odb) = git_odb::Odb::from_repo(repo) else {
        return format!("tag: tagging {abbrev} (object of unknown type)");
    };
    let Ok(obj) = odb.read(oid) else {
        return format!("tag: tagging {abbrev} (object of unknown type)");
    };
    match obj.kind {
        git_object::ObjectKind::Commit => {
            let (subject, date) = git_object::parse_commit(&obj.data, repo.hash_algo)
                .ok()
                .and_then(|c| {
                    let subject = String::from_utf8_lossy(&c.message)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    let date = c
                        .committer
                        .as_deref()
                        .and_then(git_pretty::Ident::parse)
                        .map(|id| short_date(id.ts.secs, id.ts.offset_min));
                    Some((subject, date))
                })
                .unwrap_or_else(|| ("commit object".to_string(), None));
            match date {
                Some(d) => format!("tag: tagging {abbrev} ({subject}, {d})"),
                None => format!("tag: tagging {abbrev} ({subject})"),
            }
        }
        git_object::ObjectKind::Tree => format!("tag: tagging {abbrev} (tree object)"),
        git_object::ObjectKind::Blob => format!("tag: tagging {abbrev} (blob object)"),
        git_object::ObjectKind::Tag => format!("tag: tagging {abbrev} (other tag object)"),
    }
}

/// C `show_date(..., DATE_MODE(SHORT))`: `YYYY-MM-DD` in the ident's own
/// time zone.
fn short_date(secs: i64, offset_min: i32) -> String {
    let days = (secs + offset_min as i64 * 60).div_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant's days-to-civil algorithm (proleptic Gregorian).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}