//! Crosswise tests for Phase 8 (merge-base, merge-file) against the system
//! C git. Skips when no system `git` is available.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use git_command::Command as GitCommand;
use git_command::{merge_base, merge_file};
use git_command::{CommandError, RepoContext};

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
    let dir = std::env::temp_dir().join(format!("git-p8-xwise-{}-{n}", std::process::id()));
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

fn ours(cmd: &dyn GitCommand, args: &[&str]) -> Result<(String, i32), CommandError> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    let ctx = RepoContext::new();
    let code = match cmd.run(&ctx, &args, &mut out) {
        Ok(()) => 0,
        Err(e) => e.code,
    };
    Ok((String::from_utf8(out).unwrap(), code))
}

/// Build a merge DAG: base, then a feature branch and a main-line change, then
/// a merge. Returns (dir, head, feature).
fn build_merge_dag() -> (PathBuf, String, String) {
    let git = git().unwrap();
    let dir = tempdir();
    run(git, &dir, &["init", "-q"]);
    run(git, &dir, &["config", "user.name", "T"]);
    run(git, &dir, &["config", "user.email", "t@e.com"]);
    std::fs::write(dir.join("f"), "base\n").unwrap();
    run(git, &dir, &["add", "-A"]);
    run(git, &dir, &["commit", "-qm", "base"]);
    run(git, &dir, &["checkout", "-qb", "feature"]);
    std::fs::write(dir.join("g"), "feat\n").unwrap();
    run(git, &dir, &["add", "-A"]);
    run(git, &dir, &["commit", "-qm", "feat"]);
    run(git, &dir, &["checkout", "-q", "-"]);
    std::fs::write(dir.join("h"), "main\n").unwrap();
    run(git, &dir, &["add", "-A"]);
    run(git, &dir, &["commit", "-qm", "main"]);
    run(git, &dir, &["merge", "-q", "--no-edit", "feature"]);
    let head = run(git, &dir, &["rev-parse", "HEAD"]);
    let feature = run(git, &dir, &["rev-parse", "feature"]);
    (dir, head.trim().to_string(), feature.trim().to_string())
}

#[test]
fn merge_base_matches_real_git() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let (dir, head, feature) = build_merge_dag();
    for (label, args) in [
        ("single", vec!["HEAD", "feature"]),
        ("all", vec!["--all", "HEAD", "feature"]),
    ] {
        let mut real_args = vec!["merge-base".to_string()];
        real_args.extend(args.iter().map(|s| s.to_string()));
        let real: Vec<&str> = real_args.iter().map(String::as_str).collect();
        let expected = run(git, &dir, &real);
        let mut a: Vec<&str> = args.iter().map(|s| *s).collect();
        a[args.len() - 2] = head.as_str();
        a[args.len() - 1] = feature.as_str();
        let (got, _) = with_cwd(&dir, || ours(&merge_base::MergeBase, &a)).expect("merge-base runs");
        assert_eq!(got, expected, "merge-base {label} mismatch");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merge_file_matches_real_git() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = tempdir();
    // Conflict case. Both runs use the same path so the conflict markers
    // (which embed the file name) match.
    std::fs::write(dir.join("base.txt"), "a\nb\nc\n").unwrap();
    std::fs::write(dir.join("ours.txt"), "a\nX\nc\n").unwrap();
    std::fs::write(dir.join("theirs.txt"), "a\nY\nc\n").unwrap();
    std::fs::copy(dir.join("ours.txt"), dir.join("m.txt")).unwrap();
    let ours_code = with_cwd(&dir, || ours(&merge_file::MergeFile, &["m.txt", "base.txt", "theirs.txt"])).expect("ours");
    let ours_result = std::fs::read(dir.join("m.txt")).unwrap();
    std::fs::copy(dir.join("ours.txt"), dir.join("m.txt")).unwrap();
    let _ = Command::new(git)
        .args(["merge-file", "m.txt", "base.txt", "theirs.txt"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let real_result = std::fs::read(dir.join("m.txt")).unwrap();
    assert_eq!(ours_result, real_result, "conflict result mismatch");
    assert_eq!(ours_code.1, 1, "conflict should exit 1");

    // Clean case (changes far apart).
    std::fs::write(dir.join("base.txt"), "a\nb\nc\nd\ne\nf\n").unwrap();
    std::fs::write(dir.join("ours.txt"), "a\nB\nc\nd\ne\nf\n").unwrap();
    std::fs::write(dir.join("theirs.txt"), "a\nb\nc\nd\nE\nf\n").unwrap();
    std::fs::copy(dir.join("ours.txt"), dir.join("m.txt")).unwrap();
    let ours_code = with_cwd(&dir, || ours(&merge_file::MergeFile, &["m.txt", "base.txt", "theirs.txt"])).expect("ours");
    let ours_result = std::fs::read(dir.join("m.txt")).unwrap();
    std::fs::copy(dir.join("ours.txt"), dir.join("m.txt")).unwrap();
    let _ = Command::new(git)
        .args(["merge-file", "m.txt", "base.txt", "theirs.txt"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let real_result = std::fs::read(dir.join("m.txt")).unwrap();
    assert_eq!(ours_result, real_result, "clean result mismatch");
    assert_eq!(ours_code.1, 0, "clean merge should exit 0");
    let _ = std::fs::remove_dir_all(&dir);
}