//! Crosswise tests for Phase A item A8 (diff engine completion) against
//! the system C git. Skips when no system `git` is available.

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
    let dir = std::env::temp_dir().join(format!("git-a8-xwise-{}-{n}", std::process::id()));
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

/// A fixture with modify (incl. no-EOL), staged add, staged delete,
/// exact rename, and a binary file.
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
    std::fs::write(dir.join("f.txt"), b"a\nb\nc\n").unwrap();
    std::fs::write(dir.join("g.txt"), b"x\n").unwrap();
    std::fs::write(dir.join("bin.dat"), [0u8, 1, 2, 0, 3].as_slice()).unwrap();
    run(&["add", "."]);
    commit("one", "2020-01-01 10:00:00 +0000");

    // Modify (no trailing EOL), exact rename, and stage everything.
    std::fs::write(dir.join("f.txt"), b"a\nB\nc\nd").unwrap();
    run(&["mv", "g.txt", "h.txt"]);
    std::fs::write(dir.join("bin.dat"), [0u8, 9, 0].as_slice()).unwrap();
    run(&["add", "."]);
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
fn diff_option_matrix() {
    if git().is_none() {
        return;
    }
    let dir = build_fixture().unwrap();
    let cases: Vec<Vec<&str>> = vec![
        vec!["diff", "HEAD"],
        vec!["diff", "--stat", "HEAD"],
        vec!["diff", "--numstat", "HEAD"],
        vec!["diff", "--shortstat", "HEAD"],
        vec!["diff", "--name-only", "HEAD"],
        vec!["diff", "--name-status", "HEAD"],
        vec!["diff", "--raw", "HEAD"],
        vec!["diff", "--summary", "HEAD"],
        vec!["diff", "--cached", "HEAD"],
        vec!["diff", "--no-renames", "HEAD"],
        vec!["diff", "--find-renames=90", "HEAD"],
        vec!["diff", "-U1", "HEAD"],
        vec!["diff", "-U0", "HEAD"],
        vec!["diff", "HEAD", "--", "f.txt"],
        vec!["diff", "HEAD", "--", "bin.dat"],
        vec!["diff", "--diff-filter=M", "HEAD"],
        vec!["diff", "--diff-filter=R", "HEAD"],
        vec!["diff", "--exit-code", "HEAD"],
    ];
    for case in &cases {
        check(&dir, case);
    }
}

#[test]
fn diff_exit_code_semantics() {
    if git().is_none() {
        return;
    }
    let dir = build_fixture().unwrap();
    // Clean comparison exits 0 even with --exit-code.
    check(&dir, &["diff", "--exit-code", "HEAD", "--", "g.txt"]);
    // Differences exit 1.
    check(&dir, &["diff", "--exit-code", "HEAD", "--", "f.txt"]);
}

/// A second fixture with two commits for `diff-tree` tree-vs-tree checks.
fn build_two_commit_fixture() -> Option<PathBuf> {
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
    std::fs::write(dir.join("f.txt"), b"a\nb\nc\n").unwrap();
    std::fs::write(dir.join("g.txt"), b"x\n").unwrap();
    std::fs::write(dir.join("bin.dat"), [0u8, 1, 2, 0, 3].as_slice()).unwrap();
    run(&["add", "."]);
    commit("one", "2020-01-01 10:00:00 +0000");

    std::fs::write(dir.join("f.txt"), b"a\nB\nc\nd").unwrap();
    run(&["mv", "g.txt", "h.txt"]);
    std::fs::write(dir.join("bin.dat"), [0u8, 9, 0].as_slice()).unwrap();
    run(&["add", "."]);
    commit("two", "2020-01-01 10:01:00 +0000");
    Some(dir)
}

#[test]
fn diff_tree_exit_codes() {
    if git().is_none() {
        return;
    }
    let dir = build_two_commit_fixture().unwrap();
    // Plain diff-tree exits 0 even with differences (plumbing default).
    check(&dir, &["diff-tree", "HEAD~1", "HEAD"]);
    check(&dir, &["diff-tree", "-r", "HEAD~1", "HEAD"]);
    // --exit-code: normal output, exit 1 on differences.
    check(&dir, &["diff-tree", "--exit-code", "HEAD~1", "HEAD"]);
    check(&dir, &["diff-tree", "--exit-code", "HEAD", "HEAD"]);
    // --quiet: no output at all, exit status only.
    check(&dir, &["diff-tree", "--quiet", "HEAD~1", "HEAD"]);
    check(&dir, &["diff-tree", "--quiet", "HEAD", "HEAD"]);
}

#[test]
fn diff_quiet_suppresses_output() {
    if git().is_none() {
        return;
    }
    let dir = build_fixture().unwrap();
    // --quiet prints nothing and exits 1 on differences, 0 when clean.
    check(&dir, &["diff", "--quiet", "HEAD"]);
    check(&dir, &["diff", "--quiet", "HEAD", "--", "g.txt"]);
}
