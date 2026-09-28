//! Centralized reflog storage: line format, parsing, write gating, append.
//!
//! A port of the reflog side of `refs/files-backend.c` (`log_ref_setup`,
//! `log_ref_write_fd`) plus `refs_parse_log_all_ref_updates_config` and
//! `should_autocreate_reflog` from `refs.c`. Every mutating command logs
//! through this module (the single writer); read paths (`reflog show`)
//! parse through it too, so format and gating live in exactly one place.
//!
//! Line contract (C `log_ref_write_fd`):
//! `<old-hex> <new-hex> <committer-ident>[\t<message>]\n` — no tab when the
//! message is empty. The committer ident keeps the exact shape the command
//! layer builds (`Name <email> <timestamp> <tz>`, split at the `>`
//! boundary by the existing `checkout_core` helper); this module treats it
//! as opaque bytes, and the message after the tab as opaque bytes (T-01-03:
//! never interpreted).

use std::io::Write as _;
use std::path::Path;

use git_core::Repository;
use git_hash::{HashAlgorithm, Oid};

use super::RefError;

/// Four-state `core.logallrefupdates` gating (C `enum log_refs_config`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogAllRefUpdates {
    /// Key absent: non-bare repos log the standard prefix set, bare repos
    /// log nothing (resolved at use).
    Unset,
    /// `false`: log nothing.
    None,
    /// `true`: log `HEAD` plus `refs/heads/`, `refs/remotes/`, `refs/notes/`.
    Normal,
    /// The `always` string: log every ref.
    Always,
}

impl LogAllRefUpdates {
    /// Parse a `core.logallrefupdates` value
    /// (C `refs_parse_log_all_ref_updates_config`).
    pub fn parse(value: Option<&str>) -> LogAllRefUpdates {
        match value {
            None => LogAllRefUpdates::Unset,
            Some(v) if v.eq_ignore_ascii_case("always") => LogAllRefUpdates::Always,
            Some(v) => match parse_config_bool(v) {
                Some(true) => LogAllRefUpdates::Normal,
                _ => LogAllRefUpdates::None,
            },
        }
    }
}

/// C `git_parse_maybe_bool` truth table, local so `git-refs` needs no
/// `git-config` edge: true/yes/on/1 (and `y`/`t` shorthands C accepts) are
/// true; false/no/off/0 and the empty string are false; anything else is
/// unparseable (C would die; the caller falls back to the unset default,
/// matching the pre-existing `log_all_ref_updates` behavior).
fn parse_config_bool(v: &str) -> Option<bool> {
    let lower = v.to_ascii_lowercase();
    match lower.as_str() {
        "true" | "yes" | "on" | "1" | "y" | "t" => Some(true),
        "false" | "no" | "off" | "0" | "" => Some(false),
        _ => None,
    }
}

/// Whether a ref update must be logged (C `should_autocreate_reflog` with
/// the `LOG_REFS_UNSET` → bare-default resolution from `log_ref_setup`).
pub fn should_log(cfg: LogAllRefUpdates, refname: &str, bare: bool) -> bool {
    let cfg = match cfg {
        LogAllRefUpdates::Unset => {
            if bare {
                LogAllRefUpdates::None
            } else {
                LogAllRefUpdates::Normal
            }
        }
        c => c,
    };
    match cfg {
        LogAllRefUpdates::Always => true,
        LogAllRefUpdates::Normal => {
            refname == "HEAD"
                || refname.starts_with("refs/heads/")
                || refname.starts_with("refs/remotes/")
                || refname.starts_with("refs/notes/")
        }
        LogAllRefUpdates::None | LogAllRefUpdates::Unset => false,
    }
}

/// Gating decision for a repository (reads `core.logallrefupdates` and the
/// bare flag, like `log_ref_setup`).
pub fn should_log_repo(repo: &Repository, refname: &str) -> bool {
    let cfg = LogAllRefUpdates::parse(repo.config.get("core", "logallrefupdates"));
    should_log(cfg, refname, repo.bare)
}

/// Default reflog expiry windows in days, probed against the tree C binary
/// on 2026-09-28 (research open question O1; `t/` wins ties, but `t/t1410`
/// only ever passes explicit `--expire`, so the binary + `reflog.h` decide).
///
/// Probe: entries dated 100d/40d/20d, reachable vs orphan-tip, run through
/// `git reflog expire --dry-run --verbose` with defaults and with an
/// explicit `--expire-unreachable`:
/// * 100d and 40d entries (reachable or not) prune under defaults, while
///   20d entries are kept — the TOTAL window is ~30d, matching
///   `REFLOG_EXPIRE_OPTIONS_INIT` (`now - 30d`), not the documented 90d;
/// * an explicit `--expire-unreachable` prunes only unreachable entries,
///   confirming the reachability branch is real.
///
/// Note C checks the total window FIRST (`should_expire_reflog_ent`):
/// anything older than total expires whatever its reachability, so the
/// 90d unreachable default is masked in practice (an entry old enough for
/// it already tripped the 30d ceiling). Port the header verbatim — the
/// future `expire` implementation (plan 01-03) inherits C's exact
/// semantics, masked branch included.
///
/// `gc.reflogExpire` overrides total, `gc.reflogExpireUnreachable`
/// overrides unreachable (C `reflog_expire_config`).
pub const DEFAULT_EXPIRE_TOTAL_DAYS: u64 = 30;
/// See [`DEFAULT_EXPIRE_TOTAL_DAYS`]: masked by the total ceiling, kept for
/// verbatim parity with `REFLOG_EXPIRE_OPTIONS_INIT`.
pub const DEFAULT_EXPIRE_UNREACHABLE_DAYS: u64 = 90;

/// One reflog line (C `log_ref_write_fd`): `<old> <new> <committer>` plus a
/// tab-separated message only when the message is non-empty.
pub fn format_line(old: &Oid, new: &Oid, committer: &str, message: &str) -> String {
    if message.is_empty() {
        format!("{old} {new} {committer}\n")
    } else {
        format!("{old} {new} {committer}\t{message}\n")
    }
}

/// One parsed reflog entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflogEntry {
    pub old: Oid,
    pub new: Oid,
    /// Opaque committer ident (`Name <email> <timestamp> <tz>`).
    pub ident: String,
    /// Opaque message (bytes after the tab; empty when the line has none).
    pub message: String,
    /// Seconds-since-epoch parsed from the ident's timestamp field (0 when
    /// the ident carries no parseable timestamp; C treats such entries as
    /// ancient, i.e. they always lose the expiry comparison).
    pub timestamp: i64,
}

/// Parse one `logs/<ref>` line; `None` for malformed lines (callers skip).
pub fn parse_line(line: &str, algo: HashAlgorithm) -> Option<ReflogEntry> {
    let mut parts = line.splitn(3, ' ');
    let old = Oid::from_hex(parts.next()?, algo).ok()?;
    let new = Oid::from_hex(parts.next()?, algo).ok()?;
    let rest = parts.next()?;
    let (ident, message) = match rest.find('\t') {
        Some(i) => (rest[..i].to_string(), rest[i + 1..].to_string()),
        None => (rest.to_string(), String::new()),
    };
    if ident.is_empty() {
        return None;
    }
    Some(ReflogEntry { old, new, timestamp: ident_timestamp(&ident), ident, message })
}

/// The seconds-since-epoch field of a committer ident
/// (`Name <email> <ts> <tz>`); 0 when absent or unparseable.
fn ident_timestamp(ident: &str) -> i64 {
    let mut it = ident.rsplit(' ');
    let _tz = it.next();
    it.next().and_then(|t| t.parse::<i64>().ok()).unwrap_or(0)
}

/// All entries of `logs/<ref>` in file order (oldest first); missing files
/// read as empty, malformed lines are skipped.
pub fn read_all(git_dir: &Path, refname: &str, algo: HashAlgorithm) -> Vec<ReflogEntry> {
    let content = std::fs::read_to_string(git_dir.join("logs").join(refname)).unwrap_or_default();
    content.lines().filter_map(|l| parse_line(l, algo)).collect()
}

/// Append one entry to `logs/<ref>`, creating parent directories and opening
/// append-only (C `log_ref_setup` create path + `log_ref_write_fd`).
/// Callers check [`should_log_repo`] first; this writes unconditionally.
///
/// A stale *empty* directory at the log path (left behind when a `foo/bar`
/// ref was deleted and `foo` is recreated, see `t/t1410` "stale dirs") is
/// removed so the file can be created; C moves such dirs out of the way,
/// which is observationally identical when they hold no entries.
pub fn append(
    git_dir: &Path,
    refname: &str,
    old: &Oid,
    new: &Oid,
    committer: &str,
    message: &str,
) -> Result<(), RefError> {
    let path = git_dir.join("logs").join(refname);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| RefError::Io(e.to_string()))?;
    }
    remove_stale_empty_dir(&path);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| RefError::Io(format!("unable to append to '{}': {e}", path.display())))?;
    f.write_all(format_line(old, new, committer, message).as_bytes())
        .map_err(|e| RefError::Io(format!("unable to append to '{}': {e}", path.display())))?;
    Ok(())
}

/// Remove `path` when it is an empty directory (best-effort).
fn remove_stale_empty_dir(path: &Path) {
    if path.is_dir() {
        let empty = std::fs::read_dir(path)
            .map(|mut rd| rd.next().is_none())
            .unwrap_or(false);
        if empty {
            let _ = std::fs::remove_dir(path);
        }
    }
}

/// The single writer every ref-mutating command logs through (D-02): check
/// the repository gating ([`should_log_repo`], the only place that reads
/// `core.logallrefupdates`) and append when allowed. Failures to open the
/// log are silently ignored, matching the historical call-site behavior.
pub fn log_update(
    repo: &Repository,
    refname: &str,
    old: &Oid,
    new: &Oid,
    committer: &str,
    message: &str,
) {
    if should_log_repo(repo, refname) {
        let _ = append(&repo.git_dir, refname, old, new, committer, message);
    }
}

/// Delete a reflog (`logs/<ref>`), leaving parent directories in place
/// (C `refs_delete_reflog` unlinks the file only).
pub fn remove_log(git_dir: &Path, refname: &str) {
    let _ = std::fs::remove_file(git_dir.join("logs").join(refname));
}

/// All reflog refnames under `logs/` of each given git dir, sorted and
/// deduplicated. Callers pass the worktree git dirs they cover (own git dir
/// plus the common dir for shared refs); linked worktrees' git dirs stay
/// out unless the caller lists them (C `--single-worktree` vs `--all`).
pub fn collect_logs(dirs: &[&Path]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for dir in dirs {
        collect_one(&dir.join("logs"), "", &mut out);
    }
    out.sort();
    out.dedup();
    out
}

fn collect_one(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        // Lock files are not reflogs.
        if name.ends_with(".lock") {
            continue;
        }
        let full = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        if e.path().is_dir() {
            collect_one(&e.path(), &full, out);
        } else {
            out.push(full);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_hash::HashAlgorithm;

    const IDENT: &str = "A U Thor <author@example.com> 1752327337 +0000";

    fn oids() -> (Oid, Oid) {
        let algo = HashAlgorithm::Sha1;
        (*algo.null_oid(), *algo.empty_blob())
    }

    #[test]
    fn format_emits_tab_only_with_message() {
        let (old, new) = oids();
        assert_eq!(
            format_line(&old, &new, IDENT, "commit: probe"),
            format!("{old} {new} {IDENT}\tcommit: probe\n")
        );
        // No tab on empty message (C `log_ref_write_fd`).
        let line = format_line(&old, &new, IDENT, "");
        assert_eq!(line, format!("{old} {new} {IDENT}\n"));
        assert!(!line.contains('\t'));
    }

    #[test]
    fn parse_round_trips_format() {
        let algo = HashAlgorithm::Sha1;
        let (old, new) = oids();
        for msg in ["commit (initial): probe", "", "a\tb"] {
            let line = format_line(&old, &new, IDENT, msg);
            let e = parse_line(line.trim_end_matches('\n'), algo).unwrap();
            assert_eq!(e.old, old);
            assert_eq!(e.new, new);
            assert_eq!(e.ident, IDENT);
            assert_eq!(e.message, msg);
        }
    }

    #[test]
    fn parse_rejects_malformed_lines() {
        let algo = HashAlgorithm::Sha1;
        assert!(parse_line("", algo).is_none());
        assert!(parse_line("nothex nothex ident", algo).is_none());
        let (old, new) = oids();
        // Missing ident.
        assert!(parse_line(&format!("{old} {new}"), algo).is_none());
        assert!(parse_line(&format!("{old} {new} "), algo).is_none());
        // Truncated oid.
        assert!(parse_line("abc123 ident here", algo).is_none());
    }

    #[test]
    fn gating_matches_c_prefix_rule() {
        // Unset: non-bare logs the prefix set, bare logs nothing.
        for r in ["HEAD", "refs/heads/m", "refs/remotes/o/m", "refs/notes/c"] {
            assert!(should_log(LogAllRefUpdates::Unset, r, false), "{r}");
            assert!(!should_log(LogAllRefUpdates::Unset, r, true), "{r} bare");
        }
        assert!(!should_log(LogAllRefUpdates::Unset, "refs/tags/v1", false));
        assert!(!should_log(LogAllRefUpdates::Unset, "ORIG_HEAD", false));
        // Explicit false logs nothing, even HEAD.
        assert!(!should_log(LogAllRefUpdates::None, "HEAD", false));
        // Always logs everything.
        assert!(should_log(LogAllRefUpdates::Always, "refs/tags/v1", true));
        assert!(should_log(LogAllRefUpdates::Always, "refs/meta/x", false));
    }

    #[test]
    fn parse_gating_honors_always_and_bools() {
        assert_eq!(LogAllRefUpdates::parse(None), LogAllRefUpdates::Unset);
        assert_eq!(LogAllRefUpdates::parse(Some("always")), LogAllRefUpdates::Always);
        assert_eq!(LogAllRefUpdates::parse(Some("ALWAYS")), LogAllRefUpdates::Always);
        assert_eq!(LogAllRefUpdates::parse(Some("true")), LogAllRefUpdates::Normal);
        assert_eq!(LogAllRefUpdates::parse(Some("1")), LogAllRefUpdates::Normal);
        assert_eq!(LogAllRefUpdates::parse(Some("false")), LogAllRefUpdates::None);
        assert_eq!(LogAllRefUpdates::parse(Some("0")), LogAllRefUpdates::None);
    }

    #[test]
    fn append_creates_dirs_and_appends() {
        let dir = std::env::temp_dir().join(format!("git-reflog-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let git = dir.join(".git");
        let (old, new) = oids();
        append(&git, "HEAD", &old, &new, IDENT, "commit: one").unwrap();
        append(&git, "HEAD", &new, &new, IDENT, "").unwrap();
        let entries = read_all(&git, "HEAD", HashAlgorithm::Sha1);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message, "commit: one");
        assert_eq!(entries[1].message, "");
        // Missing ref reads as empty.
        assert!(read_all(&git, "refs/heads/nope", HashAlgorithm::Sha1).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
