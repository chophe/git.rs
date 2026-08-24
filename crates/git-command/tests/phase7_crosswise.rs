//! Crosswise tests for Phase 7 (refs) against the system C git.
//! Skips when no system `git` is available.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use git_command::Command as GitCommand;
use git_command::{rev_parse, show_ref, update_ref};
use git_command::CommandError;

static COUNTER: AtomicU32 = AtomicU32::new(0);
static CWD_LOCK: Mutex<()> = Mutex::new(());

fn git() -> Option<&'static str> {
    for cand in ["/usr/bin/git", "/usr/local/bin/git", "/opt/homebrew/bin/git"] {
        if Path::new(cand).exists() {
            return Some(cand);
        }
    }
    None
}

fn tempdir() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("git-p7-xwise-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn run(git: &str, dir: &Path, args: &[&str]) -> String {
    String::from_utf8(Command::new(git).args(args).current_dir(dir).output().expect("git").stdout).unwrap()
}

fn with_cwd<R>(dir: &Path, f: impl FnOnce() -> R) -> R {
    let _guard = CWD_LOCK.lock().unwrap();
    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir).unwrap();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    std::env::set_current_dir(prev).unwrap();
    match r {
        Ok(r) => r,
        Err(p) => std::panic::resume_unwind(p),
    }
}

fn ours(cmd: &dyn GitCommand, args: &[&str]) -> Result<String, CommandError> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    cmd.run(&args, &mut out)?;
    Ok(String::from_utf8(out).unwrap())
}

fn build_repo() -> PathBuf {
    let git = git().unwrap();
    let dir = tempdir();
    run(git, &dir, &["init", "-q"]);
    run(git, &dir, &["config", "user.name", "T"]);
    run(git, &dir, &["config", "user.email", "t@e.com"]);
    std::fs::write(dir.join("f"), "a\n").unwrap();
    run(git, &dir, &["add", "-A"]);
    run(git, &dir, &["commit", "-qm", "c1"]);
    run(git, &dir, &["branch", "other"]);
    run(git, &dir, &["tag", "v1"]);
    dir
}

#[test]
fn listing_commands_match_real_git() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    for (label, args) in [
        ("rev-parse HEAD", vec!["--verify", "HEAD"]),
        ("rev-parse branch", vec!["--verify", "refs/heads/other"]),
        ("show-ref", vec![]),
        ("for-each-ref", vec![]),
        ("for-each-ref heads", vec!["refs/heads/"]),
    ] {
        let expected = run(git, &dir, &{
            let mut a: Vec<&str> = vec![];
            match label {
                l if l.starts_with("rev-parse") => a.push("rev-parse"),
                "show-ref" => a.push("show-ref"),
                _ => a.push("for-each-ref"),
            }
            a.extend(args.iter().copied());
            a
        });
        let got = with_cwd(&dir, || {
            let revparse = rev_parse::RevParse;
            let showref = show_ref::ShowRef;
            let foreach = show_ref::ForEachRef;
            let cmd: &dyn GitCommand = if label.starts_with("rev-parse") {
                &revparse
            } else if label == "show-ref" {
                &showref
            } else {
                &foreach
            };
            ours(cmd, &args)
        })
        .expect("runs");
        assert_eq!(got, expected, "{label} mismatch");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn branch_and_tag_listing_match() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    let expected_branch = run(git, &dir, &["branch"]);
    let got_branch = with_cwd(&dir, || ours(&show_ref::Branch, &[])).expect("branch");
    assert_eq!(got_branch, expected_branch, "branch mismatch");
    let expected_tag = run(git, &dir, &["tag", "-l"]);
    let got_tag = with_cwd(&dir, || ours(&show_ref::Tag, &["-l"])).expect("tag");
    assert_eq!(got_tag, expected_tag, "tag -l mismatch");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn our_refs_are_read_by_real_git() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    let head = run(git, &dir, &["rev-parse", "HEAD"]);
    let head = head.trim().to_string();
    with_cwd(&dir, || ours(&update_ref::UpdateRef, &["refs/heads/fromrust", &head])).expect("update-ref");

    let seen = run(git, &dir, &["rev-parse", "--verify", "refs/heads/fromrust"]);
    assert_eq!(seen.trim(), head, "real git should see our ref");

    // Deletion is also visible to real git.
    with_cwd(&dir, || ours(&update_ref::UpdateRef, &["-d", "refs/heads/fromrust"])).expect("delete");
    let check = Command::new(git)
        .args(["rev-parse", "--verify", "refs/heads/fromrust"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!check.status.success(), "real git should no longer see the deleted ref");
    let _ = std::fs::remove_dir_all(&dir);
}