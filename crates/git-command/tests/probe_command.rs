//! Probe-command drill for User Story 1 (T011).
//!
//! A new command needing only object reads and ref resolution must compile
//! against the existing store/ref interfaces with zero changes to those
//! components. This file IS the probe: it defines a read-only command using
//! only the public `Odb`/`RefStore` APIs and runs it against a seeded repo.
//! If this test compiles and passes, the drill holds. Skips without system git.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use git_command::{Command as GitCommand, CommandError, RepoContext};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn system_git() -> Option<&'static str> {
    for cand in ["/usr/bin/git", "/usr/local/bin/git", "/opt/homebrew/bin/git"] {
        if Path::new(cand).exists() {
            return Some(cand);
        }
    }
    None
}

fn tempdir() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("git-probe-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn sys(git: &str, dir: &Path, args: &[&str]) {
    let ok = Command::new(git)
        .args(args)
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "system git {args:?} must succeed");
}

/// The probe: a read-only command built solely from the existing store/ref
/// read interfaces (`RefStore::from_repo/list/resolve`,
/// `Odb::from_repo/read_header`). No changes to `git-odb`/`git-refs` were
/// needed to write this — that is the assertion.
struct ProbeRefs;

impl GitCommand for ProbeRefs {
    fn name(&self) -> &'static str {
        "probe-refs"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        if !args.is_empty() {
            return Err(CommandError::usage("probe-refs: takes no arguments"));
        }
        let repo = ctx.repository()?;
        let store = git_refs::RefStore::from_repo(&repo);
        let odb = git_odb::Odb::from_repo(&repo).map_err(|e| CommandError::fatal(e.to_string()))?;
        let mut refs = store.list();
        refs.sort();
        for (name, oid) in refs {
            let obj = odb.read(&oid).map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "{:?} {oid} {name}", obj.kind).map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}

#[test]
fn probe_command_compiles_against_store_ref_interfaces() {
    let Some(git) = system_git() else {
        return;
    };
    let dir = tempdir();
    sys(git, &dir, &["init", "-q", "-b", "main"]);
    sys(git, &dir, &["config", "user.name", "Probe"]);
    sys(git, &dir, &["config", "user.email", "p@example.com"]);
    std::fs::write(dir.join("f.txt"), "probe\n").unwrap();
    sys(git, &dir, &["add", "-A"]);
    sys(git, &dir, &["commit", "-qm", "seed"]);

    let ctx = RepoContext::at(&dir);
    let mut out = Vec::new();
    ProbeRefs.run(&ctx, &[], &mut out).expect("probe command runs");
    let text = String::from_utf8(out).expect("utf8 output");
    assert!(text.contains("refs/heads/main"), "probe lists the seeded branch:\n{text}");
    assert!(text.contains("Commit"), "probe resolves object headers:\n{text}");

    // Usage errors keep the 129 class through the probe path.
    let err = ProbeRefs
        .run(&ctx, &["--bogus".to_string()], &mut Vec::new())
        .expect_err("bogus arg is a usage error");
    assert_eq!(err.code, 129);

    std::fs::remove_dir_all(&dir).ok();
}
