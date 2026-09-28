//! `git reflog` — tracer slice: the `show` subcommand only.
//!
//! The full subcommand matrix (`list`, `exists`, `write`, `delete`, `drop`,
//! `expire`) lands in plan 01-03; those names are rejected with a usage
//! error here so the gap is visible instead of silently wrong. Any other
//! first word falls through to `show` as a revision, exactly like C's
//! `cmd_reflog` falling through to `cmd_log_reflog` (so `git reflog bogus`
//! dies with C's ambiguous-argument text, byte for byte).
//!
//! Output contract (probed against the tree C binary, 2026-09-28):
//! entries render newest-first as `<abbrev-new> <ref>@{<n}>: <message>`,
//! one per line, with the space after the colon even when the message is
//! empty. A ref that does not resolve as a revision dies with C's
//! ambiguous-argument fatal (exit 128) even when a log file exists; a
//! resolving ref with no log prints nothing, exit 0.

use std::io::Write;

use crate::{Command, CommandError, RepoContext};

/// Subcommands owned by plan 01-03 (full reflog surface).
const FUTURE_SUBCOMMANDS: &[&str] = &["list", "exists", "write", "delete", "drop", "expire"];

pub struct Reflog;

impl Command for Reflog {
    fn name(&self) -> &'static str {
        "reflog"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut args = args;
        if let Some(first) = args.first() {
            match first.as_str() {
                "show" => args = &args[1..],
                s if FUTURE_SUBCOMMANDS.contains(&s) => {
                    return Err(CommandError::usage(format!("reflog: subcommand '{s}' not supported")));
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("reflog: option '{s}' not supported")));
                }
                _ => {} // C fall-through: first word is a revision for show.
            }
        }
        if args.len() > 1 {
            return Err(CommandError::usage("usage: git reflog show <ref>"));
        }
        let refname = args.first().map_or("HEAD", String::as_str);
        show(ctx, refname, out)
    }
}

fn show(ctx: &RepoContext, refname: &str, out: &mut dyn Write) -> Result<(), CommandError> {
    let repo = ctx.repository()?;
    // C resolves the revision before reading the log: an unborn HEAD dies
    // with the ambiguous-argument fatal even when `logs/HEAD` exists.
    // `resolve_arg` renders that exact text already.
    crate::resolve_arg(&repo, refname).map(|_| ())?;
    let entries = git_refs::reflog::read_all(&repo.git_dir, refname, repo.hash_algo);
    for (n, e) in entries.iter().rev().enumerate() {
        let abbrev = crate::checkout_core::short_oid(&repo, &e.new);
        writeln!(out, "{abbrev} {refname}@{{{n}}}: {}", e.message)
            .map_err(|e| CommandError::fatal(e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_hash::HashAlgorithm;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn repo_with_log() -> (tempfile_dir, String) {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("git-reflog-cmd-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let git = dir.join(".git");
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let algo = HashAlgorithm::Sha1;
        let old = *algo.null_oid();
        let new = *algo.empty_blob();
        // The loose ref must exist so the revision-resolvability check passes.
        std::fs::write(git.join("refs/heads/main"), format!("{new}\n")).unwrap();
        let ident = "T Est <t@example.com> 1752327337 +0000";
        git_refs::reflog::append(&git, "HEAD", &old, &new, ident, "commit (initial): probe").unwrap();
        git_refs::reflog::append(&git, "HEAD", &new, &new, ident, "commit: second").unwrap();
        git_refs::reflog::append(&git, "refs/heads/main", &old, &new, ident, "commit (initial): probe").unwrap();
        git_refs::reflog::append(&git, "refs/heads/main", &new, &new, ident, "commit: second").unwrap();
        (tempfile_dir(dir), new.to_string())
    }

    struct tempfile_dir(PathBuf);
    impl Drop for tempfile_dir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    #[test]
    fn show_head_renders_newest_first() {
        let (tmp, new_hex) = repo_with_log();
        let ctx = RepoContext::at(&tmp.0);
        let mut buf = Vec::new();
        Reflog.run(&ctx, &["show".to_string(), "HEAD".to_string()], &mut buf).unwrap();
        let text = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("HEAD@{0}: commit: second"), "{}", lines[0]);
        assert!(lines[0].starts_with(&new_hex[..7]), "{}", lines[0]);
        assert!(lines[1].ends_with("HEAD@{1}: commit (initial): probe"), "{}", lines[1]);
    }

    #[test]
    fn show_defaults_to_head_and_accepts_bare_ref() {
        let (tmp, _) = repo_with_log();
        let ctx = RepoContext::at(&tmp.0);
        let mut a = Vec::new();
        Reflog.run(&ctx, &[], &mut a).unwrap();
        let mut b = Vec::new();
        Reflog.run(&ctx, &["refs/heads/main".to_string()], &mut b).unwrap();
        assert!(!a.is_empty() && !b.is_empty());
        assert!(String::from_utf8(b).unwrap().contains("refs/heads/main@{0}"));
    }

    #[test]
    fn show_unresolvable_ref_dies_like_c() {
        let (tmp, _) = repo_with_log();
        let ctx = RepoContext::at(&tmp.0);
        let mut buf = Vec::new();
        let err = Reflog
            .run(&ctx, &["show".to_string(), "refs/heads/nope".to_string()], &mut buf)
            .unwrap_err();
        assert_eq!(err.code, 128);
        assert!(err.message.contains("ambiguous argument 'refs/heads/nope'"), "{}", err.message);
    }

    #[test]
    fn future_subcommands_are_usage_errors() {
        let (tmp, _) = repo_with_log();
        let ctx = RepoContext::at(&tmp.0);
        let mut buf = Vec::new();
        let err = Reflog.run(&ctx, &["expire".to_string()], &mut buf).unwrap_err();
        assert_eq!(err.code, 129);
    }
}
