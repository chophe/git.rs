//! Crosswise tests for Phase A item A6 (rev-list/log options) against the
//! system C git. Skips when no system `git` is available.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

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
    let dir = std::env::temp_dir().join(format!("git-a6-xwise-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
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

/// A repo with distinct dates, a side branch, and a merge.
fn build_fixture() -> Option<PathBuf> {
    let g = git()?;
    let dir = tempdir();
    let env = [
        ("GIT_AUTHOR_NAME", "T"),
        ("GIT_AUTHOR_EMAIL", "t@example.com"),
        ("GIT_COMMITTER_NAME", "T"),
        ("GIT_COMMITTER_EMAIL", "t@example.com"),
    ];
    let commit = |msg: &str, date: &str| {
        let mut c = Command::new(g);
        c.args(["commit", "-q", "--allow-empty", "-m", msg]);
        for (k, v) in env {
            c.env(k, v);
        }
        c.env("GIT_AUTHOR_DATE", date).env("GIT_COMMITTER_DATE", date);
        assert!(c.current_dir(&dir).status().unwrap().success());
    };
    let run = |args: &[&str]| {
        let st = Command::new(g).args(args).current_dir(&dir).status().unwrap();
        assert!(st.success());
    };
    run(&["init", "-q", "-b", "main"]);
    commit("commit 1", "2020-01-01 10:00:00 +0000");
    commit("commit 2", "2020-01-02 10:00:00 +0000");
    run(&["checkout", "-qb", "side", "HEAD~1"]);
    commit("side commit", "2020-01-05 10:00:00 +0000");
    run(&["checkout", "-q", "main"]);
    commit("commit 3", "2020-01-03 10:00:00 +0000");
    let mut c = Command::new(g);
    c.args(["merge", "-q", "--no-ff", "side", "-m", "merge side"]);
    for (k, v) in env {
        c.env(k, v);
    }
    c.env("GIT_AUTHOR_DATE", "2020-01-06 10:00:00 +0000").env("GIT_COMMITTER_DATE", "2020-01-06 10:00:00 +0000");
    assert!(c.current_dir(&dir).status().unwrap().success());
    Some(dir)
}

fn ours(dir: &Path, args: &[&str]) -> (String, i32) {
    with_cwd(dir, || {
        let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/debug/git")
            .canonicalize()
            .expect("rust git binary must be built");
        let out = Command::new(exe).args(args).output().unwrap();
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        (text, out.status.code().unwrap_or(128))
    })
}

fn check(dir: &Path, args: &[&str]) {
    let out = Command::new(git().unwrap())
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let mut rtext = String::from_utf8_lossy(&out.stdout).into_owned();
    rtext.push_str(&String::from_utf8_lossy(&out.stderr));
    let rcode = out.status.code().unwrap_or(128);
    let (otext, ocode) = ours(dir, args);
    assert_eq!(rcode, ocode, "args {args:?}");
    assert_eq!(rtext, otext, "args {args:?}\nreal: {rtext}\nours: {otext}");
}

#[test]
fn rev_list_option_matrix() {
    if git().is_none() {
        return;
    }
    let dir = build_fixture().unwrap();
    let cases: Vec<Vec<&str>> = vec![
        vec!["rev-list", "--all"],
        vec!["rev-list", "HEAD"],
        vec!["rev-list", "--topo-order", "--all"],
        vec!["rev-list", "--date-order", "--all"],
        vec!["rev-list", "main..side"],
        vec!["rev-list", "main...side"],
        vec!["rev-list", "--not", "side", "main"],
        vec!["rev-list", "--count", "--all"],
        vec!["rev-list", "--objects", "--all"],
        vec!["rev-list", "--objects", "HEAD"],
        vec!["rev-list", "--merges", "--all"],
        vec!["rev-list", "--no-merges", "--all"],
        vec!["rev-list", "--min-parents=2", "--all"],
        vec!["rev-list", "--max-parents=0", "--all"],
        vec!["rev-list", "-n", "2", "HEAD"],
        vec!["rev-list", "--skip=1", "HEAD"],
        vec!["rev-list", "--reverse", "--all"],
        vec!["rev-list", "--first-parent", "HEAD"],
        vec!["rev-list", "--branches"],
        vec!["rev-list", "--author=T", "--all"],
        vec!["rev-list", "--grep=side", "--all"],
        vec!["rev-list", "--invert-grep", "--grep=side", "--all"],
        vec!["rev-list", "--no-walk", "main", "side"],
        vec!["rev-list", "HEAD", "--", "missing-path"],
    ];
    for case in &cases {
        check(&dir, case);
    }
}

#[test]
fn log_option_matrix() {
    if git().is_none() {
        return;
    }
    let dir = build_fixture().unwrap();
    let cases: Vec<Vec<&str>> = vec![
        vec!["log", "--oneline", "--all"],
        vec!["log", "--topo-order", "--oneline"],
        vec!["log", "--oneline", "main..side"],
        vec!["log", "--oneline", "--merges"],
        vec!["log", "--oneline", "--no-merges"],
        vec!["log", "--oneline", "-n", "2"],
        vec!["log", "--oneline", "--skip=1"],
        vec!["log", "--reverse", "--oneline"],
        vec!["log", "--oneline", "--author=T"],
        vec!["log", "--oneline", "--grep=side"],
        vec!["log", "--oneline", "HEAD", "--", "missing-path"],
    ];
    for case in &cases {
        check(&dir, case);
    }
}
