//! Crosswise tests for Phase 5 (diff) against the system C git.
//! Skips when no system `git` is available.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use git_command::Command as GitCommand;
use git_command::{diff, diff_tree};
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
    let dir = std::env::temp_dir().join(format!("git-p5-xwise-{}-{n}", std::process::id()));
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

/// Build a repo with a modification, an addition, and a deletion, across a
/// nested directory. Returns (dir, old_tree, new_tree).
fn build_repo() -> (PathBuf, String, String) {
    let git = git().unwrap();
    let dir = tempdir();
    run(git, &dir, &["init", "-q"]);
    run(git, &dir, &["config", "user.name", "T"]);
    run(git, &dir, &["config", "user.email", "t@e.com"]);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/f.txt"), "a\nb\nc\nd\ne\nf\ng\nh\n").unwrap();
    std::fs::write(dir.join("gone.txt"), "bye\n").unwrap();
    run(git, &dir, &["add", "-A"]);
    run(git, &dir, &["commit", "-qm", "base"]);
    let t1 = run(git, &dir, &["rev-parse", "HEAD^{tree}"]);

    std::fs::write(dir.join("sub/f.txt"), "a\nb\nX\nc\nd\ne\nf\ng\nh\nY\n").unwrap();
    std::fs::write(dir.join("added.txt"), "new\n").unwrap();
    run(git, &dir, &["rm", "-q", "gone.txt"]);
    run(git, &dir, &["add", "-A"]);
    run(git, &dir, &["commit", "-qm", "next"]);
    let t2 = run(git, &dir, &["rev-parse", "HEAD^{tree}"]);
    (dir, t1.trim().to_string(), t2.trim().to_string())
}

fn ours<C: GitCommand>(cmd: &C, args: &[String]) -> Result<String, CommandError> {
    let mut out = Vec::new();
    cmd.run(args, &mut out)?;
    Ok(String::from_utf8(out).unwrap())
}

/// Run a command, returning (output, exit code). `diff` exits 1 on
/// differences, so tests must compare both.
fn ours_with_code<C: GitCommand>(cmd: &C, args: &[String]) -> (String, i32) {
    let mut out = Vec::new();
    let code = match cmd.run(args, &mut out) {
        Ok(()) => 0,
        Err(e) => e.code,
    };
    (String::from_utf8(out).unwrap(), code)
}

#[test]
fn diff_tree_matches_real_git() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let (dir, t1, t2) = build_repo();
    for (label, extra) in [
        ("raw", vec![]),
        ("-r", vec!["-r".to_string()]),
        ("name-status", vec!["-r".to_string(), "--name-status".to_string()]),
        ("name-only", vec!["-r".to_string(), "--name-only".to_string()]),
        ("patch", vec!["-p".to_string()]),
    ] {
        let mut real_args = vec!["diff-tree".to_string()];
        real_args.extend(extra.iter().cloned());
        real_args.push(t1.clone());
        real_args.push(t2.clone());
        let real: Vec<&str> = real_args.iter().map(String::as_str).collect();
        let expected = run(git, &dir, &real);
        let got = with_cwd(&dir, || {
            let mut a = extra.clone();
            a.push(t1.clone());
            a.push(t2.clone());
            ours(&diff_tree::DiffTree, &a)
        })
        .expect("diff-tree runs");
        assert_eq!(got, expected, "diff-tree {label} mismatch");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn diff_matches_real_git() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let (dir, t1, t2) = build_repo();
    let expected = run(git, &dir, &["diff", t1.as_str(), t2.as_str()]);
    let expected_code = {
        // `git diff` exits 1 when there are differences.
        Command::new(git)
            .args(["diff", t1.as_str(), t2.as_str()])
            .current_dir(&dir)
            .output()
            .unwrap()
            .status
            .code()
            .unwrap()
    };
    let (got, code) = with_cwd(&dir, || ours_with_code(&diff::Diff, &[t1.clone(), t2.clone()]));
    assert_eq!(got, expected, "diff trees mismatch");
    assert_eq!(code, expected_code, "diff exit code mismatch");
    // With --exit-code both should report 1.
    let (_, code1) = with_cwd(&dir, || {
        ours_with_code(&diff::Diff, &["--exit-code".to_string(), t1, t2])
    });
    assert_eq!(code1, 1, "diff --exit-code should exit 1 on differences");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn diff_no_index_matches_real_git() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = tempdir();
    std::fs::write(dir.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    std::fs::write(dir.join("b.txt"), "one\nTWO\nthree\n").unwrap();
    let expected = run(git, &dir, &["diff", "--no-index", "a.txt", "b.txt"]);
    let (got, _code) = with_cwd(&dir, || {
        ours_with_code(&diff::Diff, &["--no-index".to_string(), "a.txt".to_string(), "b.txt".to_string()])
    });
    assert_eq!(got, expected, "diff --no-index mismatch");
    let _ = std::fs::remove_dir_all(&dir);
}