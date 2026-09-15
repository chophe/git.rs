//! Crosswise tests for Phase B item B5 (`git commit`) against the system C
//! git. Skips when no system `git` is available.
//!
//! Commit objects, refs and reflogs are all deterministic once the dates are
//! fixed (`GIT_AUTHOR_DATE`/`GIT_COMMITTER_DATE`), so every check compares the
//! exact bytes C git produces.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn git() -> Option<&'static str> {
    for cand in ["/usr/bin/git", "/usr/local/bin/git", "/opt/homebrew/bin/git"] {
        if Path::new(cand).exists() {
            return Some(cand);
        }
    }
    None
}

fn rust_git() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/git");
    p.canonicalize().ok()
}

fn tempdir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("git-b5-xwise-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn run(exe: &Path, dir: &Path, args: &[&str], stdin: Option<&str>) -> (String, String, i32, Option<Vec<u8>>, Option<Vec<u8>>) {
    let out = run_raw(exe, dir, args, stdin);
    let head = run_raw(exe, dir, &["rev-parse", "HEAD"], None);
    let head_hex = String::from_utf8_lossy(&head.stdout).trim().to_string();
    let commit = if out.status.success() && !head_hex.is_empty() {
        let c = run_raw(exe, dir, &["cat-file", "commit", &head_hex], None);
        if c.status.success() { Some(c.stdout) } else { None }
    } else {
        None
    };
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(128),
        Some(head_hex.into_bytes()),
        commit,
    )
}

fn run_raw(exe: &Path, dir: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let home = tempdir("h");
    let mut cmd = Command::new(exe);
    cmd.args(args).current_dir(dir).env_clear();
    cmd.env("PATH", "/usr/bin:/bin");
    cmd.env("HOME", &home);
    cmd.env("TMPDIR", std::env::temp_dir());
    cmd.env("LC_ALL", "C");
    cmd.env("GIT_CONFIG_NOSYSTEM", "1");
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null");
    cmd.env("GIT_AUTHOR_NAME", "T");
    cmd.env("GIT_AUTHOR_EMAIL", "t@example.com");
    cmd.env("GIT_COMMITTER_NAME", "T");
    cmd.env("GIT_COMMITTER_EMAIL", "t@example.com");
    cmd.env("GIT_AUTHOR_DATE", "2020-01-01 10:00:00 +0000");
    cmd.env("GIT_COMMITTER_DATE", "2020-01-01 10:00:00 +0000");
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn");
    if let Some(s) = stdin {
        child.stdin.take().unwrap().write_all(s.as_bytes()).unwrap();
    }
    child.wait_with_output().expect("wait")
}

/// Run `git add -A` in a fresh repo (both sides), then the commit sequence.
fn setup(dir: &Path, exe: &Path, files: &[(&str, &str)]) {
    let out = run_raw(exe, dir, &["init", "-q", "-b", "main"], None);
    assert!(out.status.success());
    for (p, c) in files {
        let full = dir.join(p);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(full, c).unwrap();
    }
    let out = run_raw(exe, dir, &["add", "-A"], None);
    assert!(out.status.success(), "add failed: {}", String::from_utf8_lossy(&out.stderr));
}

fn check(cdir: &Path, rdir: &Path, args: &[&str], stdin: Option<&str>) {
    let git = git().unwrap();
    let rust = rust_git().unwrap();
    let c = run(Path::new(git), cdir, args, stdin);
    let r = run(&rust, rdir, args, stdin);
    assert_eq!(r.0, c.0, "stdout differs for commit {args:?}");
    assert_eq!(r.1, c.1, "stderr differs for commit {args:?}");
    assert_eq!(r.2, c.2, "exit differs for commit {args:?}");
    assert_eq!(r.3, c.3, "HEAD differs for commit {args:?}");
    assert_eq!(r.4, c.4, "commit object differs for commit {args:?}");
    // Reflog + branch ref bytes.
    for rel in [".git/logs/HEAD", ".git/logs/refs/heads/main", ".git/refs/heads/main"] {
        let cb = std::fs::read(cdir.join(rel)).ok();
        let rb = std::fs::read(rdir.join(rel)).ok();
        assert_eq!(rb, cb, "{rel} differs for commit {args:?}");
    }
}

fn pair(tag: &str, files: &[(&str, &str)]) -> (PathBuf, PathBuf) {
    let c = tempdir(&format!("{tag}-c"));
    let r = tempdir(&format!("{tag}-r"));
    setup(&c, Path::new(git().unwrap()), files);
    setup(&r, &rust_git().unwrap(), files);
    (c, r)
}

#[test]
fn commit_sequence_matches_c_git() {
    let (Some(_git), Some(_rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let (c, r) = pair(
        "seq",
        &[("a.txt", "alpha\n"), ("sub/b.txt", "beta\n")],
    );

    check(&c, &r, &["commit", "-m", "initial commit"], None);
    std::fs::write(c.join("a.txt"), "alpha two\n").unwrap();
    std::fs::write(r.join("a.txt"), "alpha two\n").unwrap();
    check(&c, &r, &["commit", "-am", "second"], None);
    // Nothing staged.
    check(&c, &r, &["commit", "-m", "again"], None);
    check(&c, &r, &["commit", "--amend", "--no-edit"], None);
    check(&c, &r, &["commit", "--allow-empty", "-m", "empty"], None);
    // New file, two -m paragraphs.
    std::fs::write(c.join("new.txt"), "new\n").unwrap();
    std::fs::write(r.join("new.txt"), "new\n").unwrap();
    let out = run_raw(Path::new(git().unwrap()), &c, &["add", "new.txt"], None);
    assert!(out.status.success());
    let out = run_raw(&rust_git().unwrap(), &r, &["add", "new.txt"], None);
    assert!(out.status.success());
    check(&c, &r, &["commit", "-m", "one", "-m", "two"], None);
    // Author + explicit date.
    std::fs::write(c.join("sub/b.txt"), "beta two\n").unwrap();
    std::fs::write(r.join("sub/b.txt"), "beta two\n").unwrap();
    check(
        &c,
        &r,
        &[
            "commit",
            "-a",
            "--author=A U Thor <author@example.com>",
            "--date=2021-02-03 04:05:06 +0100",
            "-m",
            "authored",
        ],
        None,
    );
    // Message from a file (incl. trailing whitespace cleanup).
    std::fs::write(c.join("a.txt"), "alpha three\n").unwrap();
    std::fs::write(r.join("a.txt"), "alpha three\n").unwrap();
    let msg = "subject line  \n\nbody line\n\n";
    check(&c, &r, &["commit", "-a", "-F", "-"], Some(msg));

    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn nothing_to_commit_variants_match_c_git() {
    let (Some(_git), Some(_rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    for (tag, mutate) in [
        ("clean", 0),
        ("modified", 1),
        ("untracked", 2),
        ("both", 3),
        ("deleted", 4),
        ("untracked-dir", 5),
    ] {
        let (c, r) = pair(tag, &[("a.txt", "alpha\n"), ("b.txt", "beta\n")]);
        // Baseline commit so HEAD exists.
        check(&c, &r, &["commit", "-m", "init"], None);
        match mutate {
            1 => {
                std::fs::write(c.join("a.txt"), "changed\n").unwrap();
                std::fs::write(r.join("a.txt"), "changed\n").unwrap();
            }
            2 => {
                std::fs::write(c.join("u.txt"), "u\n").unwrap();
                std::fs::write(r.join("u.txt"), "u\n").unwrap();
            }
            3 => {
                std::fs::write(c.join("a.txt"), "changed\n").unwrap();
                std::fs::write(r.join("a.txt"), "changed\n").unwrap();
                std::fs::write(c.join("u.txt"), "u\n").unwrap();
                std::fs::write(r.join("u.txt"), "u\n").unwrap();
            }
            4 => {
                std::fs::remove_file(c.join("b.txt")).unwrap();
                std::fs::remove_file(r.join("b.txt")).unwrap();
            }
            5 => {
                std::fs::create_dir_all(c.join("nd")).unwrap();
                std::fs::create_dir_all(r.join("nd")).unwrap();
                std::fs::write(c.join("nd/x"), "x\n").unwrap();
                std::fs::write(r.join("nd/x"), "x\n").unwrap();
                std::fs::write(c.join("nd/y"), "y\n").unwrap();
                std::fs::write(r.join("nd/y"), "y\n").unwrap();
            }
            _ => {}
        }
        check(&c, &r, &["commit", "-m", "x"], None);
        let _ = std::fs::remove_dir_all(&c);
        let _ = std::fs::remove_dir_all(&r);
    }
}

#[test]
fn empty_repo_and_empty_message_match_c_git() {
    let (Some(_git), Some(_rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let c = tempdir("empty-c");
    let r = tempdir("empty-r");
    setup(&c, Path::new(git().unwrap()), &[]);
    setup(&r, &rust_git().unwrap(), &[]);
    check(&c, &r, &["commit", "-m", "x"], None);

    // Empty message aborts.
    std::fs::write(c.join("a.txt"), "a\n").unwrap();
    std::fs::write(r.join("a.txt"), "a\n").unwrap();
    let out = run_raw(Path::new(git().unwrap()), &c, &["add", "-A"], None);
    assert!(out.status.success());
    let out = run_raw(&rust_git().unwrap(), &r, &["add", "-A"], None);
    assert!(out.status.success());
    check(&c, &r, &["commit", "-m", ""], None);
    // `--allow-empty-message` accepts it.
    check(&c, &r, &["commit", "--allow-empty-message", "-m", ""], None);

    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}
