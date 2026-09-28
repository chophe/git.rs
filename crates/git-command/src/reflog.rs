//! `git reflog` — the full seven-subcommand surface.
//!
//! A port of `builtin/reflog.c` (`show|list|exists|write|delete|drop|
//! expire`) over the [`git_refs::reflog`] storage module (plan 01-01).
//! Every user-facing string below was captured from the tree C binary
//! (2.55.0.552); `t/t1410-reflog.sh` is the oracle (`t/` wins ties).
//!
//! Expiry policy (C `reflog.c`): `gc.reflogExpire` overrides the total
//! window, `gc.reflogExpireUnreachable` the unreachable window, with
//! per-ref `gc.<pattern>.reflogExpire*` overrides matched by wildmatch;
//! `refs/stash` never expires when unconfigured; the built-binary defaults
//! are total=30d / unreachable=90d
//! ([`git_refs::reflog::DEFAULT_EXPIRE_TOTAL_DAYS`]).

use std::collections::HashSet;
use std::io::Write;

use crate::{Command, CommandError, RepoContext};
use git_hash::Oid;
use git_odb::Odb;

pub struct Reflog;

const SHOW_USAGE: &str = "usage: git reflog [show] [<log-options>] [<ref>]";
const LIST_USAGE: &str = "usage: git reflog list";
const EXISTS_USAGE: &str = "usage: git reflog exists <ref>";
const WRITE_USAGE: &str = "usage: git reflog write <ref> <old-oid> <new-oid> <message>";
const DELETE_USAGE: &str = "usage: git reflog delete [--rewrite] [--updateref]\n\
                         [--dry-run | -n] [--verbose] <ref>@{<specifier>}...";
const DROP_USAGE: &str = "usage: git reflog drop [--all [--single-worktree] | <refs>...]";
const EXPIRE_USAGE: &str = "usage: git reflog expire [--expire=<time>] [--expire-unreachable=<time>]\n\
                         [--rewrite] [--updateref] [--stale-fix]\n\
                         [--dry-run | -n] [--verbose] [--all [--single-worktree] | <refs>...]";

/// Full usage strings for `-h` per subcommand (C `parse-options` `-h`
/// output, captured from the tree binary).
const EXPIRE_HELP: &str = "usage: git reflog expire [--expire=<time>] [--expire-unreachable=<time>]\n\
                         [--rewrite] [--updateref] [--stale-fix]\n\
                         [--dry-run | -n] [--verbose] [--all [--single-worktree] | <refs>...]\n\
\n\
    -n, --[no-]dry-run    do not actually prune any entries\n\
    --[no-]rewrite        rewrite the old SHA1 with the new SHA1 of the entry that now precedes it\n\
    --[no-]updateref      update the reference to the value of the top reflog entry\n\
    --[no-]verbose        print extra information on screen\n\
    --expire <timestamp>  prune entries older than the specified time\n\
    --expire-unreachable <timestamp>\n\
                          prune entries older than <time> that are not reachable from the current tip of the branch\n\
    --[no-]stale-fix      prune any reflog entries that point to broken commits\n\
    --[no-]all            process the reflogs of all references\n\
    --[no-]single-worktree\n\
                          limits processing to reflogs from the current worktree only\n";
const DELETE_HELP: &str = "usage: git reflog delete [--rewrite] [--updateref]\n\
                         [--dry-run | -n] [--verbose] <ref>@{<specifier>}...\n\
\n\
    -n, --[no-]dry-run    do not actually prune any entries\n\
    --[no-]rewrite        rewrite the old SHA1 with the new SHA1 of the entry that now precedes it\n\
    --[no-]updateref      update the reference to the value of the top reflog entry\n\
    --[no-]verbose        print extra information on screen\n";
const DROP_HELP: &str = "usage: git reflog drop [--all [--single-worktree] | <refs>...]\n\
\n\
    --[no-]all            drop the reflogs of all references\n\
    --[no-]single-worktree\n\
                          drop reflogs from the current worktree only\n";

impl Command for Reflog {
    fn name(&self) -> &'static str {
        "reflog"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let (sub, rest) = match args.first().map(String::as_str) {
            Some("show") => ("show", &args[1..]),
            Some("list") => ("list", &args[1..]),
            Some("exists") => ("exists", &args[1..]),
            Some("write") => ("write", &args[1..]),
            Some("delete") => ("delete", &args[1..]),
            Some("drop") => ("drop", &args[1..]),
            Some("expire") => ("expire", &args[1..]),
            _ => ("show", args),
        };
        match sub {
            "show" => cmd_show(ctx, rest, out),
            "list" => cmd_list(ctx, rest, out),
            "exists" => cmd_exists(ctx, rest, out),
            "write" => cmd_write(ctx, rest, out),
            "delete" => cmd_delete(ctx, rest, out),
            "drop" => cmd_drop(ctx, rest, out),
            "expire" => cmd_expire(ctx, rest, out),
            _ => unreachable!(),
        }
    }
}

/// `reflog show [<ref>] [-- <paths>]`: newest-first
/// `<abbrev> <ref>@{<n>}: <message>` lines (probed byte-identical).
/// A `--` separator switches to log-style pathspec filtering; an
/// unresolvable ref dies with C's ambiguous-argument fatal even when a log
/// file exists.
fn cmd_show(ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
    for a in args {
        if a == "-h" || a == "--help" {
            writeln!(out, "{SHOW_USAGE}").map_err(|e| CommandError::fatal(e.to_string()))?;
            return Ok(());
        }
    }
    let (refs, paths) = match args.iter().position(|a| a == "--") {
        Some(i) => (&args[..i], &args[i + 1..]),
        None => (args, &[][..]),
    };
    if refs.len() > 1 {
        return Err(CommandError::usage("usage: git reflog show <ref>"));
    }
    // Unknown dash-options fall through to the log layer in C; without a
    // full `log -g` option set here, report usage (uncovered by `t/`).
    for a in refs {
        if a.starts_with('-') && a.len() > 1 {
            return Err(CommandError::usage(format!("reflog: option '{a}' not supported")));
        }
    }
    let refname = refs.first().map_or("HEAD", String::as_str);
    let repo = ctx.repository()?;
    crate::resolve_arg(&repo, refname).map(|_| ())?;
    let entries = git_refs::reflog::read_all(&repo.git_dir, refname, repo.hash_algo);
    let odb = if paths.is_empty() {
        None
    } else {
        Odb::from_repo(&repo).map_err(CommandError::from).ok()
    };
    for (n, e) in entries.iter().rev().enumerate() {
        if let (Some(odb), false) = (&odb, paths.is_empty()) {
            if !crate::log::commit_touches_paths(odb, &e.new, paths, repo.hash_algo) {
                continue;
            }
        }
        let abbrev = crate::checkout_core::short_oid(&repo, &e.new);
        writeln!(out, "{abbrev} {refname}@{{{n}}}: {}", e.message)
            .map_err(|e| CommandError::fatal(e.to_string()))?;
    }
    Ok(())
}

/// `reflog list`: every reflog in the current worktree's git dir plus the
/// shared (non-per-worktree) logs of the common dir, sorted.
fn cmd_list(ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        writeln!(out, "{LIST_USAGE}").map_err(|e| CommandError::fatal(e.to_string()))?;
        return Ok(());
    }
    if let Some(bogus) = args.first() {
        return Err(CommandError::error(format!("error: list does not accept arguments: '{bogus}'")));
    }
    let repo = ctx.repository()?;
    for name in worktree_logs(&repo, false) {
        writeln!(out, "{name}").map_err(|e| CommandError::fatal(e.to_string()))?;
    }
    Ok(())
}

/// Logs visible from this worktree: its own git dir plus the common dir's
/// shared logs (per-worktree refs of other worktrees stay out). With
/// `all_worktrees`, every linked worktree's git dir is included too (C
/// `collect_reflog` over `get_worktrees`).
fn worktree_logs(repo: &git_core::Repository, all_worktrees: bool) -> Vec<String> {
    if all_worktrees {
        let mut dirs: Vec<std::path::PathBuf> = vec![repo.git_dir.clone()];
        if repo.common_dir != repo.git_dir {
            dirs.push(repo.common_dir.clone());
        }
        // Linked worktrees' git dirs live at $COMMONDIR/worktrees/<name>.
        if let Ok(rd) = std::fs::read_dir(repo.common_dir.join("worktrees")) {
            let mut names: Vec<_> = rd.flatten().collect();
            names.sort_by_key(|e| e.file_name());
            for e in names {
                if e.path().is_dir() {
                    dirs.push(e.path());
                }
            }
        }
        let refs: Vec<&std::path::Path> = dirs.iter().map(|p| p.as_path()).collect();
        return git_refs::reflog::collect_logs(&refs);
    }
    if repo.common_dir == repo.git_dir {
        return git_refs::reflog::collect_logs(&[repo.git_dir.as_path()]);
    }
    let mut out = git_refs::reflog::collect_logs(&[repo.git_dir.as_path()]);
    for name in git_refs::reflog::collect_logs(&[repo.common_dir.as_path()]) {
        if !is_per_worktree_ref(&name) {
            out.push(name);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// C `is_per_worktree_ref`: `refs/worktree/`, `refs/bisect/`,
/// `refs/rewritten/` live in the worktree's own git dir.
fn is_per_worktree_ref(name: &str) -> bool {
    name.starts_with("refs/worktree/")
        || name.starts_with("refs/bisect/")
        || name.starts_with("refs/rewritten/")
}

/// `reflog exists <ref>`: exit 0 when `logs/<ref>` exists, silent exit 1
/// otherwise; bad names die `invalid ref format` (C `cmd_reflog_exists`).
fn cmd_exists(ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        writeln!(out, "{EXISTS_USAGE}").map_err(|e| CommandError::fatal(e.to_string()))?;
        return Ok(());
    }
    let Some(refname) = args.first() else {
        return Err(CommandError::usage(EXISTS_USAGE));
    };
    if git_refs::validate_refname_allow_onelevel(refname).is_err() {
        return Err(CommandError::fatal(format!("fatal: invalid ref format: {refname}")));
    }
    let repo = ctx.repository()?;
    if repo.git_dir.join("logs").join(refname).is_file() {
        return Ok(());
    }
    Err(CommandError::silent(1))
}

/// C `is_root_ref` (refs.c): all-caps/underscore/dash names plus the
/// irregular root refs (`HEAD`, `AUTO_MERGE`, ...).
fn is_root_ref(name: &str) -> bool {
    const IRREGULAR: &[&str] = &[
        "HEAD",
        "AUTO_MERGE",
        "BISECT_EXPECTED_REV",
        "NOTES_MERGE_PARTIAL",
        "NOTES_MERGE_REF",
        "MERGE_AUTOSTASH",
    ];
    if IRREGULAR.contains(&name) {
        return true;
    }
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_uppercase() || b == b'-' || b == b'_')
}

/// `reflog write <ref> <old> <new> <message>`: append one entry (C
/// `cmd_reflog_write`); non-null oids must exist as objects.
fn cmd_write(ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        writeln!(out, "{WRITE_USAGE}").map_err(|e| CommandError::fatal(e.to_string()))?;
        return Ok(());
    }
    if args.len() != 4 {
        return Err(CommandError::usage(WRITE_USAGE));
    }
    let (name, old_s, new_s, message) = (&args[0], &args[1], &args[2], &args[3]);
    if !is_root_ref(name) && git_refs::validate_refname(name).is_err() {
        return Err(CommandError::fatal(format!("fatal: invalid reference name: {name}")));
    }
    let repo = ctx.repository()?;
    let algo = repo.hash_algo;
    let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
    let old = Oid::from_hex(old_s, algo)
        .map_err(|_| CommandError::fatal(format!("fatal: invalid old object ID: '{old_s}'")))?;
    if old != *algo.null_oid() && odb.read(&old).is_err() {
        return Err(CommandError::fatal(format!("fatal: old object '{old_s}' does not exist")));
    }
    let new = Oid::from_hex(new_s, algo)
        .map_err(|_| CommandError::fatal(format!("fatal: invalid new object ID: '{new_s}'")))?;
    if new != *algo.null_oid() && odb.read(&new).is_err() {
        return Err(CommandError::fatal(format!("fatal: new object '{new_s}' does not exist")));
    }
    let ident = crate::checkout_core::committer_ident(&repo)?;
    git_refs::reflog::append(&repo.git_dir, name, &old, &new, &ident, message)
        .map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
    Ok(())
}

/// Prune dials shared by `delete` and `expire` (C `EXPIRE_REFLOGS_*`).
#[derive(Debug, Clone, Copy)]
struct PruneFlags {
    rewrite: bool,
    updateref: bool,
    dry_run: bool,
    verbose: bool,
}

/// Parse `-n/--dry-run/--rewrite/--updateref/--verbose` in `args`,
/// returning the flags plus the remaining operands. `-h/--help` prints
/// `help` to `out` and returns `None` (the caller then exits 129).
fn parse_prune_flags(
    args: &[String],
    usage: &str,
    help: &str,
    out: &mut dyn Write,
) -> Result<Option<(PruneFlags, Vec<String>)>, CommandError> {
    let mut flags = PruneFlags { rewrite: false, updateref: false, dry_run: false, verbose: false };
    let mut rest = Vec::new();
    for a in args {
        match a.as_str() {
            "-h" | "--help" => {
                writeln!(out, "{help}").map_err(|e| CommandError::fatal(e.to_string()))?;
                return Ok(None);
            }
            "-n" | "--dry-run" => flags.dry_run = true,
            "--rewrite" => flags.rewrite = true,
            "--updateref" => flags.updateref = true,
            "--verbose" => flags.verbose = true,
            s if s.starts_with('-') && s.len() > 1 => {
                if s.starts_with("--") {
                    return Err(CommandError::usage(format!("error: unknown option `{s}'\n{usage}")));
                }
                // Bundled shorts (`-n...`).
                let mut ok = true;
                for c in s[1..].chars() {
                    if c == 'n' {
                        flags.dry_run = true;
                    } else {
                        ok = false;
                        break;
                    }
                }
                if !ok {
                    return Err(CommandError::usage(format!("error: unknown option `{s}'\n{usage}")));
                }
            }
            s => rest.push(s.to_string()),
        }
    }
    Ok(Some((flags, rest)))
}

/// `reflog delete [--rewrite] [--updateref] [--dry-run] [--verbose]
/// <ref>@{<spec>}...`: remove the selected entries (C `reflog_delete`:
///
/// numeric `@{N}` removes the Nth-newest entry via the recno counter;
/// anything else is an approxidate that removes every entry older than
/// it).
fn cmd_delete(ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
    let Some((flags, rest)) = parse_prune_flags(args, DELETE_USAGE, DELETE_HELP, out)? else {
        return Ok(());
    };
    if rest.is_empty() {
        return Err(CommandError::error("error: no reflog specified to delete"));
    }
    let repo = ctx.repository()?;
    let mut status = 0;
    for rev in &rest {
        let Some(at) = rev.find("@{") else {
            eprintln!("error: not a reflog: {rev}");
            status = -1;
            continue;
        };
        let Some(dwim) = dwim_log(&repo, &rev[..at]) else {
            eprintln!("error: no reflog for '{rev}'");
            status = -1;
            continue;
        };
        let spec = &rev[at + 2..];
        let selector = match spec.strip_suffix('}').filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) {
            Some(n) => DeleteSelector::Index(n.parse::<usize>().unwrap_or(usize::MAX)),
            None => {
                // Date form: one trailing `}` stripped like C's
                // approxidate input.
                let date_s = spec.strip_suffix('}').unwrap_or(spec);
                DeleteSelector::OlderThan(parse_expiry_arg(date_s))
            }
        };
        if expire_one(&repo, &dwim, &selector, flags) != 0 {
            status = -1;
        }
    }
    if status != 0 {
        return Err(CommandError::silent(255));
    }
    Ok(())
}

/// Which entries a `delete` selector removes.
enum DeleteSelector {
    /// `@{N}`: the Nth newest entry (no-op when out of range, like C's
    /// recno counter that never fires).
    Index(usize),
    /// `@{date}`: entries older than the timestamp.
    OlderThan(i64),
}

/// Parse an expiry timestamp: `never`/`false` mean "never" (0, like C
/// `git_config_expiry_date`), `all` means "everything" (i64::MAX, matching
/// the tree binary's `--expire=all` behavior), otherwise the shared date
/// parser. Unparseable values become 0 (C's approxidate fallback).
fn parse_expiry_arg(s: &str) -> i64 {
    let t = s.trim();
    if t.eq_ignore_ascii_case("never") || t.eq_ignore_ascii_case("false") {
        return 0;
    }
    if t.eq_ignore_ascii_case("all") {
        return i64::MAX;
    }
    let now = crate::ident::now_utc();
    git_date::parse(t, now).map(|ts| ts.secs).unwrap_or(0)
}

/// Expire one ref by a `delete` selector. Returns 0 on success.
fn expire_one(
    repo: &git_core::Repository,
    refname: &str,
    selector: &DeleteSelector,
    flags: PruneFlags,
) -> i32 {
    let lock = match lock_ref_for_expire(repo, refname) {
        Some(l) => l,
        None => return -1,
    };
    let git_dir = log_git_dir(repo, refname);
    let entries = git_refs::reflog::read_all(&git_dir, refname, repo.hash_algo);
    let prune: Vec<bool> = match selector {
        DeleteSelector::Index(n) => {
            // Nth newest: index len-1-n in oldest-first order.
            entries.iter().enumerate().map(|(i, _)| i + 1 + n == entries.len()).collect()
        }
        DeleteSelector::OlderThan(ts) => entries.iter().map(|e| *ts != 0 && e.timestamp < *ts).collect(),
    };
    let rc = apply_prune(repo, refname, &git_dir, &entries, &prune, flags);
    drop(lock);
    rc
}

/// `reflog drop [--all [--single-worktree] | <refs>...]`: delete whole
/// logs (C `cmd_reflog_drop`).
fn cmd_drop(ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
    let mut do_all = false;
    let mut single_worktree = false;
    let mut rest = Vec::new();
    for a in args {
        match a.as_str() {
            "-h" | "--help" => {
                writeln!(out, "{DROP_HELP}").map_err(|e| CommandError::fatal(e.to_string()))?;
                return Ok(());
            }
            "--all" => do_all = true,
            "--single-worktree" => single_worktree = true,
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(CommandError::usage(format!("error: unknown option `{s}'\n{DROP_USAGE}")));
            }
            s => rest.push(s.to_string()),
        }
    }
    if do_all && !rest.is_empty() {
        return Err(CommandError::usage(format!("usage: references specified along with --all\n{DROP_USAGE}")));
    }
    let repo = ctx.repository()?;
    if do_all {
        for name in worktree_logs(&repo, !single_worktree) {
            git_refs::reflog::remove_log(&log_git_dir(&repo, &name), &name);
        }
        return Ok(());
    }
    let mut status = 0;
    for r in &rest {
        match dwim_log(&repo, r) {
            Some(name) => git_refs::reflog::remove_log(&repo.git_dir, &name),
            None => {
                eprintln!("error: reflog could not be found: '{r}'");
                status = -1;
            }
        }
    }
    if status != 0 {
        return Err(CommandError::silent(255));
    }
    Ok(())
}

/// The git dir holding `logs/<name>` (per-worktree logs live beside their
/// worktree; shared logs under the common dir).
fn log_git_dir(repo: &git_core::Repository, name: &str) -> std::path::PathBuf {
    if is_per_worktree_ref(name) || repo.git_dir.join("logs").join(name).is_file() {
        repo.git_dir.clone()
    } else {
        repo.common_dir.clone()
    }
}

/// C `repo_dwim_log`: expand `arg` through the rev-parse ref rules and
/// return the refname whose reflog exists (the resolved target's log when
/// the spell-out has none but resolves to something that does, e.g. `HEAD`
/// falling back to `refs/heads/main`'s log).
pub(crate) fn dwim_log(repo: &git_core::Repository, arg: &str) -> Option<String> {
    let arg = if arg == "@" { "HEAD" } else { arg };
    let store = git_refs::RefStore::from_repo(repo);
    let cands = [
        arg.to_string(),
        format!("refs/{arg}"),
        format!("refs/tags/{arg}"),
        format!("refs/heads/{arg}"),
        format!("refs/remotes/{arg}"),
        format!("refs/remotes/{arg}/HEAD"),
    ];
    for c in &cands {
        if repo.git_dir.join("logs").join(c).is_file() {
            return Some(c.clone());
        }
        let resolved = store.resolve(c);
        if resolved.is_none() {
            continue;
        }
        // The spell-out has no log but resolves elsewhere: use the
        // target's log when the names differ.
        if c == &"HEAD" {
            if let Some(sym) = store.head_symbolic_target() {
                if sym != *c && repo.git_dir.join("logs").join(&sym).is_file() {
                    return Some(sym);
                }
            }
        }
    }
    None
}

/// `reflog expire` policy for one ref: explicit CLI windows plus the
/// config-derived defaults after per-ref overrides (C
/// `reflog_expire_config` + `reflog_expire_options_set_refname`).
struct ExpirePolicy {
    total: i64,
    unreachable: i64,
    stalefix: bool,
}

/// Read the expiry policy for `refname` (C `reflog_expire_config` key
/// mapping; unparseable values fall back to the probed defaults per
/// threat T-01-13, never to delete-all).
fn expire_policy(
    repo: &git_core::Repository,
    refname: &str,
    cli_total: Option<i64>,
    cli_unreach: Option<i64>,
) -> ExpirePolicy {
    let now = crate::ident::now_utc().secs;
    let mut dflt_total = now - git_refs::reflog::DEFAULT_EXPIRE_TOTAL_DAYS as i64 * 86400;
    let mut dflt_unreach = now - git_refs::reflog::DEFAULT_EXPIRE_UNREACHABLE_DAYS as i64 * 86400;
    // `gc.reflogExpire*` defaults.
    if let Some(v) = repo.config.get("gc", "reflogexpire") {
        dflt_total = expiry_config_value(v, dflt_total);
    }
    if let Some(v) = repo.config.get("gc", "reflogexpireunreachable") {
        dflt_unreach = expiry_config_value(v, dflt_unreach);
    }
    // Per-ref `gc.<pattern>.reflogExpire*` overrides (first wildmatch wins).
    let mut pat_total: Option<i64> = None;
    let mut pat_unreach: Option<i64> = None;
    for e in repo.config.entries() {
        if !e.section.eq_ignore_ascii_case("gc") {
            continue;
        }
        let Some(pattern) = &e.subsection else { continue };
        if git_attributes::wildmatch(pattern, refname, 0) != git_attributes::WM_MATCH {
            continue;
        }
        if e.key.eq_ignore_ascii_case("reflogexpire") && pat_total.is_none() {
            pat_total = Some(expiry_config_value(&e.value, dflt_total));
        } else if e.key.eq_ignore_ascii_case("reflogexpireunreachable") && pat_unreach.is_none() {
            pat_unreach = Some(expiry_config_value(&e.value, dflt_unreach));
        }
    }
    let mut p_total = pat_total.unwrap_or(dflt_total);
    let mut p_unreach = pat_unreach.unwrap_or(dflt_unreach);
    // Unconfigured `refs/stash` never expires (C exemption).
    if refname == "refs/stash" && pat_total.is_none() && repo.config.get("gc", "reflogexpire").is_none() {
        p_total = 0;
    }
    if refname == "refs/stash"
        && pat_unreach.is_none()
        && repo.config.get("gc", "reflogexpireunreachable").is_none()
    {
        p_unreach = 0;
    }
    if let Some(t) = cli_total {
        p_total = t;
    }
    if let Some(u) = cli_unreach {
        p_unreach = u;
    }
    ExpirePolicy { total: p_total, unreachable: p_unreach, stalefix: false }
}

/// A `gc.*Expire*` config value: `never`/`false` disable (0 = never, like
/// C); otherwise an expiry date before which entries prune. Unparseable
/// values keep the default (T-01-13).
fn expiry_config_value(v: &str, dflt: i64) -> i64 {
    let t = v.trim();
    if t.eq_ignore_ascii_case("never") || t.eq_ignore_ascii_case("false") {
        return 0;
    }
    if t.eq_ignore_ascii_case("all") {
        return i64::MAX;
    }
    let now = crate::ident::now_utc();
    git_date::parse(t, now).map(|ts| ts.secs).unwrap_or(dflt)
}

/// `reflog expire [...]`: prune entries per policy (C `cmd_reflog_expire`
/// + `files_reflog_expire`).
fn cmd_expire(ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
    let mut cli_total: Option<i64> = None;
    let mut cli_unreach: Option<i64> = None;
    let mut stalefix = false;
    let mut do_all = false;
    let mut single_worktree = false;
    let mut flags = PruneFlags { rewrite: false, updateref: false, dry_run: false, verbose: false };
    let mut rest = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "-h" | "--help" => {
                writeln!(out, "{EXPIRE_HELP}").map_err(|e| CommandError::fatal(e.to_string()))?;
                return Ok(());
            }
            "-n" | "--dry-run" => flags.dry_run = true,
            "--rewrite" => flags.rewrite = true,
            "--updateref" => flags.updateref = true,
            "--verbose" => flags.verbose = true,
            "--stale-fix" => stalefix = true,
            "--all" => do_all = true,
            "--single-worktree" => single_worktree = true,
            s if s.starts_with("--expire=") => {
                let v = &s["--expire=".len()..];
                cli_total = Some(parse_expiry_cli(v, "--expire")?);
            }
            s if s.starts_with("--expire-unreachable=") => {
                let v = &s["--expire-unreachable=".len()..];
                cli_unreach = Some(parse_expiry_cli(v, "--expire-unreachable")?);
            }
            "--expire" | "--expire-unreachable" => {
                i += 1;
                let opt = &args[i - 1];
                let v = args
                    .get(i)
                    .ok_or_else(|| CommandError::usage(format!("error: option `{opt}' requires a value\n{EXPIRE_USAGE}")))?;
                let t = parse_expiry_cli(v, opt)?;
                if opt == "--expire" {
                    cli_total = Some(t);
                } else {
                    cli_unreach = Some(t);
                }
            }
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(CommandError::usage(format!("error: unknown option `{s}'\n{EXPIRE_USAGE}")));
            }
            s => rest.push(s.to_string()),
        }
        i += 1;
    }
    let repo = ctx.repository()?;
    let targets: Vec<String> = if do_all {
        worktree_logs(&repo, !single_worktree)
    } else {
        let mut v = Vec::new();
        let mut failed = false;
        for r in &rest {
            match dwim_log(&repo, r) {
                Some(name) => v.push(name),
                None => {
                    eprintln!("error: reflog could not be found: '{r}'");
                    failed = true;
                }
            }
        }
        if failed {
            return Err(CommandError::silent(255));
        }
        v
    };
    let mut failed = false;
    for name in &targets {
        let mut policy = expire_policy(&repo, name, cli_total, cli_unreach);
        policy.stalefix = stalefix;
        if expire_with_policy(&repo, name, &policy, flags) != 0 {
            failed = true;
        }
    }
    if failed {
        return Err(CommandError::silent(255));
    }
    Ok(())
}

/// Parse a CLI `--expire*` value (C `expire_total_callback` /
/// `expire_unreachable_callback`): `die` on unparseable input.
fn parse_expiry_cli(v: &str, opt: &str) -> Result<i64, CommandError> {
    let t = v.trim();
    if t.eq_ignore_ascii_case("never") || t.eq_ignore_ascii_case("false") {
        return Ok(0);
    }
    if t.eq_ignore_ascii_case("all") {
        return Ok(i64::MAX);
    }
    let now = crate::ident::now_utc();
    match git_date::parse(t, now) {
        Ok(ts) => Ok(ts.secs),
        Err(_) => Err(CommandError::fatal(format!("fatal: invalid timestamp '{v}' given to '{opt}'"))),
    }
}

/// Hold the ref lock across one ref's rewrite like C
/// (`lock_ref_oid_basic`); contention (or other lock failure) prints C's
/// `cannot lock ref` error and fails the ref. Returns `None` on failure.
fn lock_ref_for_expire(
    repo: &git_core::Repository,
    refname: &str,
) -> Option<git_refs::lock::LockFile> {
    let store = git_refs::RefStore::from_repo(repo);
    match git_refs::lock::LockFile::acquire(&store.common_dir().join(refname)) {
        Ok(l) => Some(l),
        Err(e) => {
            eprintln!("error: cannot lock ref '{refname}': {e}");
            None
        }
    }
}

/// Expire one ref under a fully-resolved policy. Returns 0 on success.
fn expire_with_policy(
    repo: &git_core::Repository,
    refname: &str,
    policy: &ExpirePolicy,
    flags: PruneFlags,
) -> i32 {
    let lock = match lock_ref_for_expire(repo, refname) {
        Some(l) => l,
        None => return -1,
    };
    let git_dir = log_git_dir(repo, refname);
    if !git_dir.join("logs").join(refname).is_file() {
        // Raced away after locking: nothing to do (C returns success).
        return 0;
    }
    let entries = git_refs::reflog::read_all(&git_dir, refname, repo.hash_algo);
    let reach = Reachability::build(repo, refname, policy);
    let odb = Odb::from_repo(repo).ok();
    // Oldest-first evaluation (C `for_each_reflog_ent` order).
    let prune: Vec<bool> =
        entries.iter().map(|e| should_prune_entry(&reach, odb.as_ref(), repo, policy, e)).collect();
    let rc = apply_prune(repo, refname, &git_dir, &entries, &prune, flags);
    drop(lock);
    rc
}

/// Shared rewrite backend for `delete` and `expire`: report each entry
/// (`keep`/`prune`/`would prune`, C `should_expire_reflog_ent_verbose`),
/// rewrite the log unless dry-run (with `--rewrite` chaining), and update
/// the ref to the newest kept entry under `--updateref`.
fn apply_prune(
    repo: &git_core::Repository,
    refname: &str,
    git_dir: &std::path::Path,
    entries: &[git_refs::reflog::ReflogEntry],
    prune: &[bool],
    flags: PruneFlags,
) -> i32 {
    if flags.verbose {
        for (e, p) in entries.iter().zip(prune.iter()) {
            if *p {
                println!("{} {}", prune_word(flags.dry_run), e.message);
            } else {
                println!("keep {}", e.message);
            }
        }
    }
    if flags.dry_run {
        return 0;
    }
    // Rewrite with chaining when `--rewrite` (C `expire_reflog_ent`:
    // each kept entry's old becomes the previous kept new, starting
    // from null — probed on the tree binary).
    let mut out = String::new();
    let mut last_kept = *repo.hash_algo.null_oid();
    let mut newest_kept: Option<Oid> = None;
    for (e, p) in entries.iter().zip(prune.iter()) {
        if *p {
            continue;
        }
        let old = if flags.rewrite { last_kept } else { e.old };
        out.push_str(&git_refs::reflog::format_line(&old, &e.new, &e.ident, &e.message));
        last_kept = e.new;
        newest_kept = Some(e.new);
    }
    let log_path = git_dir.join("logs").join(refname);
    if let Some(dir) = log_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match git_refs::lock::LockFile::acquire(&log_path) {
        Ok(mut lock) => {
            if lock.write_and_fsync(out.as_bytes()).is_err() || lock.commit().is_err() {
                eprintln!("error: couldn't write {}", log_path.display());
                return -1;
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            return -1;
        }
    }
    if flags.updateref {
        if let Some(new) = newest_kept {
            // Never through a symref (C's NO_RECURSE resolve check).
            if ref_is_direct(repo, refname) {
                let store = git_refs::RefStore::from_repo(repo);
                if store.update(refname, Some(&new)).is_err() {
                    eprintln!("error: couldn't write {refname}");
                    return -1;
                }
            }
        }
    }
    0
}

/// Verbose wording for a prune decision (C
/// `should_expire_reflog_ent_verbose`): `would prune` under `--dry-run`.
fn prune_word(dry_run: bool) -> &'static str {
    if dry_run {
        "would prune"
    } else {
        "prune"
    }
}

/// The reachability context for one ref's expiry (C
/// `reflog_expiry_prepare`: UE_HEAD marks from every ref tip, UE_NORMAL
/// from the ref's own tip, UE_ALWAYS marks nothing).
struct Reachability {
    kind: UnreachKind,
    reachable: HashSet<Oid>,
}

#[derive(PartialEq, Eq)]
enum UnreachKind {
    Always,
    Normal,
    Head,
}

impl Reachability {
    fn build(repo: &git_core::Repository, refname: &str, policy: &ExpirePolicy) -> Reachability {
        let store = git_refs::RefStore::from_repo(repo);
        let kind = if policy.unreachable == 0 || is_head_ref(refname) {
            UnreachKind::Head
        } else {
            match store.resolve(refname) {
                Some(tip) if is_commit(repo, &tip) => UnreachKind::Normal,
                _ => UnreachKind::Always,
            }
        };
        // `expire_unreachable <= expire_total` collapses to UE_ALWAYS
        // (C `reflog_expiry_prepare`).
        let kind = if kind != UnreachKind::Head && policy.unreachable <= policy.total {
            UnreachKind::Always
        } else {
            kind
        };
        let mut reachable = HashSet::new();
        if kind != UnreachKind::Always {
            let tips: Vec<Oid> = match kind {
                UnreachKind::Head => store.list().into_iter().map(|(_, o)| o).collect(),
                UnreachKind::Normal => store.resolve(refname).into_iter().collect(),
                UnreachKind::Always => Vec::new(),
            };
            mark_reachable(repo, &tips, &mut reachable);
        }
        Reachability { kind, reachable }
    }

    /// C `is_unreachable`: null oids are reachable; non-commits are kept;
    /// otherwise consult the mark set (UE_ALWAYS prunes unconditionally).
    fn is_unreachable(&self, repo: &git_core::Repository, oid: &Oid) -> bool {
        if *oid == *repo.hash_algo.null_oid() {
            return false;
        }
        if self.kind == UnreachKind::Always {
            return true;
        }
        let Some(commit_oid) = peel_to_commit(repo, oid) else {
            return false; // Not a commit: keep it.
        };
        !self.reachable.contains(&commit_oid)
    }
}

fn is_head_ref(refname: &str) -> bool {
    refname == "HEAD"
}

/// Peel tags to the underlying commit (C
/// `lookup_commit_reference_gently`); `None` for non-commit objects.
fn peel_to_commit(repo: &git_core::Repository, oid: &Oid) -> Option<Oid> {
    let odb = Odb::from_repo(repo).ok()?;
    let mut cur = *oid;
    loop {
        let obj = odb.read(&cur).ok()?;
        match obj.kind {
            git_object::ObjectKind::Commit => return Some(cur),
            git_object::ObjectKind::Tag => {
                cur = git_object::parse_tag(&obj.data, repo.hash_algo).ok()?.object;
            }
            _ => return None,
        }
    }
}

fn is_commit(repo: &git_core::Repository, oid: &Oid) -> bool {
    peel_to_commit(repo, oid).is_some()
}

/// Mark every commit reachable from `tips` (parents walk; C
/// `mark_reachable` without the time cutoff, which decides identically:
/// entries older than the total window prune regardless of reachability).
fn mark_reachable(repo: &git_core::Repository, tips: &[Oid], out: &mut HashSet<Oid>) {
    let Ok(odb) = Odb::from_repo(repo) else { return };
    let mut stack: Vec<Oid> = tips.to_vec();
    while let Some(oid) = stack.pop() {
        if !out.insert(oid) {
            continue;
        }
        let Ok(obj) = odb.read(&oid) else { continue };
        if obj.kind != git_object::ObjectKind::Commit {
            continue;
        }
        if let Ok(c) = git_object::parse_commit(&obj.data, repo.hash_algo) {
            stack.extend(c.parents);
        }
    }
}

/// C `should_expire_reflog_ent` (plus the `stalefix` completeness check).
fn should_prune_entry(
    reach: &Reachability,
    odb: Option<&Odb>,
    repo: &git_core::Repository,
    policy: &ExpirePolicy,
    e: &git_refs::reflog::ReflogEntry,
) -> bool {
    if policy.total != 0 && e.timestamp < policy.total {
        return true;
    }
    if policy.stalefix {
        let complete = odb
            .map(|o| entry_complete(o, repo, &e.old) && entry_complete(o, repo, &e.new))
            .unwrap_or(false);
        if !complete {
            return true;
        }
    }
    if policy.unreachable != 0 && e.timestamp < policy.unreachable {
        match reach.kind {
            UnreachKind::Always => return true,
            UnreachKind::Normal | UnreachKind::Head => {
                if reach.is_unreachable(repo, &e.old) || reach.is_unreachable(repo, &e.new) {
                    return true;
                }
            }
        }
    }
    false
}

/// C `keep_entry` + `commit_is_complete`: null oids pass; otherwise the
/// oid must peel to a commit whose ancestry parses and whose trees/blobs
/// all exist. Missing objects fail open toward pruning.
fn entry_complete(odb: &Odb, repo: &git_core::Repository, oid: &Oid) -> bool {
    if *oid == *repo.hash_algo.null_oid() {
        return true;
    }
    let Some(tip) = peel_to_commit(repo, oid) else { return false };
    let mut seen_c: HashSet<Oid> = HashSet::new();
    let mut seen_o: HashSet<Oid> = HashSet::new();
    let mut stack = vec![tip];
    while let Some(c) = stack.pop() {
        if !seen_c.insert(c) {
            continue;
        }
        let Ok(obj) = odb.read(&c) else { return false };
        if obj.kind != git_object::ObjectKind::Commit {
            return false;
        }
        let Ok(commit) = git_object::parse_commit(&obj.data, repo.hash_algo) else { return false };
        if !tree_complete(odb, repo, &commit.tree, &mut seen_o) {
            return false;
        }
        stack.extend(commit.parents);
    }
    true
}

/// Every tree/blob under `tree` exists (C `tree_is_complete`).
fn tree_complete(odb: &Odb, repo: &git_core::Repository, tree: &Oid, seen: &mut HashSet<Oid>) -> bool {
    if !seen.insert(*tree) {
        return true;
    }
    let Ok(obj) = odb.read(tree) else { return false };
    if obj.kind != git_object::ObjectKind::Tree {
        return false;
    }
    let Ok(entries) = git_object::parse_tree(&obj.data, repo.hash_algo) else { return false };
    for e in entries {
        if e.is_dir() {
            if !tree_complete(odb, repo, &e.oid, seen) {
                return false;
            }
        } else if odb.read(&e.oid).is_err() {
            return false;
        }
    }
    true
}

/// Whether `refname` holds a direct value (not a symref): C skips
/// `--updateref` for symbolic refs (NO_RECURSE resolve check).
fn ref_is_direct(repo: &git_core::Repository, refname: &str) -> bool {
    if refname == "HEAD" {
        return git_refs::RefStore::from_repo(repo).head_symbolic_target().is_none();
    }
    match std::fs::read_to_string(repo.common_dir.join(refname)) {
        Ok(content) => !content.trim_start().starts_with("ref:"),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_flag_parsing() {
        let mut buf = Vec::new();
        let args = ["--rewrite".to_string(), "-n".to_string(), "HEAD@{0}".to_string()];
        let (flags, rest) = parse_prune_flags(&args, DELETE_USAGE, DELETE_HELP, &mut buf).unwrap().unwrap();
        assert!(flags.rewrite && flags.dry_run && !flags.updateref);
        assert_eq!(rest, vec!["HEAD@{0}".to_string()]);
    }

    #[test]
    fn expiry_arg_spellings() {
        assert_eq!(parse_expiry_arg("never"), 0);
        assert_eq!(parse_expiry_arg("false"), 0);
        assert_eq!(parse_expiry_arg("all"), i64::MAX);
        assert_eq!(parse_expiry_arg("garbage-timestamp-xyz"), 0);
    }
}
