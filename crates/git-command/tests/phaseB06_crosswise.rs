//! Crosswise tests for Phase B item B6 (`git status` full output) against the
//! system C git. Skips when no system `git` is available.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
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
    let dir = std::env::temp_dir().join(format!("git-b6-xwise-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn hermetic(exe: &Path, dir: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(exe);
    cmd.args(args).current_dir(dir).env_clear();
    cmd.env("PATH", "/usr/bin:/bin");
    cmd.env("HOME", tempdir("h"));
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
    cmd.output().expect("spawn")
}

fn shell_success(exe: &Path, dir: &Path, args: &[&str]) {
    let out = hermetic(exe, dir, args);
    assert!(
        out.status.success(),
        "{exe:?} {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn check_status(cdir: &Path, rdir: &Path, args: &[&str]) {
    let git = Path::new(git().unwrap());
    let rust = rust_git().unwrap();
    let c = hermetic(git, cdir, args);
    let r = hermetic(&rust, rdir, args);
    assert_eq!(c.status.code(), r.status.code(), "exit differs for status {args:?}");
    assert_eq!(c.stdout, r.stdout, "stdout differs for status {args:?}");
    assert_eq!(c.stderr, r.stderr, "stderr differs for status {args:?}");
}

fn write_tree(root: &Path, tree: &[(&str, &str)]) {
    std::fs::create_dir_all(root.join("sub")).unwrap();
    for (path, contents) in tree {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(full, contents).unwrap();
    }
}

fn mixed_fixture(tag: &str) -> (PathBuf, PathBuf) {
    let git = Path::new(git().unwrap());
    let c = tempdir(&format!("{tag}-c"));
    let r = tempdir(&format!("{tag}-r"));
    for dir in [&c, &r] {
        shell_success(git, dir, &["init", "-q", "-b", "main"]);
    }
    for dir in [&c, &r] {
        write_tree(
            dir,
            &[
                ("a.txt", "a\n"),
                ("b.txt", "b\n"),
                ("c.txt", "c\n"),
                ("sub/d.txt", "d\n"),
                (".gitignore", "*.log\n"),
            ],
        );
        shell_success(git, dir, &["add", "-A"]);
        shell_success(git, dir, &["commit", "-qm", "init"]);

        std::fs::write(dir.join("a.txt"), "a2\n").unwrap();
        std::fs::write(dir.join("new.txt"), "n\n").unwrap();
        std::fs::remove_file(dir.join("c.txt")).unwrap();
        shell_success(git, dir, &["add", "a.txt"]);
        shell_success(git, dir, &["add", "new.txt"]);
        shell_success(git, dir, &["add", "c.txt"]);
        std::fs::write(dir.join("b.txt"), "b2\n").unwrap();
        std::fs::remove_file(dir.join("sub/d.txt")).unwrap();
        std::fs::write(dir.join("u.txt"), "u\n").unwrap();
        std::fs::create_dir_all(dir.join("nd")).unwrap();
        std::fs::write(dir.join("nd/x"), "x\n").unwrap();
        std::fs::write(dir.join("x.log"), "ignored\n").unwrap();
    }
    (c, r)
}

#[test]
fn full_status_matches_c_git() {
    let (Some(_), Some(_)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let (c, r) = mixed_fixture("full");
    for args in [
        vec!["status"],
        vec!["status", "--short"],
        vec!["status", "--porcelain"],
        vec!["status", "--porcelain", "-b"],
        vec!["status", "-b"],
        vec!["status", "--ignored"],
        vec!["status", "--short", "--ignored"],
        vec!["status", "--short", "-z"],
        vec!["status", "--untracked-files=no"],
        vec!["status", "-sb"],
    ] {
        check_status(&c, &r, &args);
    }
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn status_states_match_c_git() {
    let (Some(_), Some(_)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let git = Path::new(git().unwrap());
    let (c, r) = (tempdir("states-c"), tempdir("states-r"));
    for dir in [&c, &r] {
        shell_success(git, dir, &["init", "-q", "-b", "main"]);
    }

    // Clean and untracked states split cleanly by display mode.
    check_status(&c, &r, &["status"]);
    check_status(&c, &r, &["status", "--short"]);
    write_tree(&c, &[("a.txt", "a\n")]);
    write_tree(&r, &[("a.txt", "a\n")]);
    check_status(&c, &r, &["status"]);
    check_status(&c, &r, &["status", "--short"]);
    check_status(&c, &r, &["status", "--untracked-files=no"]);
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn status_subdirectory_matches_c_git() {
    let (Some(_), Some(_)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let (c, r) = mixed_fixture("subdir");
    for args in [vec!["status"], vec!["status", "--short"]] {
        let git = Path::new(git().unwrap());
        let rust = rust_git().unwrap();
        let co = hermetic(git, &c.join("sub"), &args);
        let ro = hermetic(&rust, &r.join("sub"), &args);
        assert_eq!(co.status.code(), ro.status.code(), "exit differs for {args:?}");
        assert_eq!(co.stdout, ro.stdout, "stdout differs for {args:?}");
        assert_eq!(co.stderr, ro.stderr, "stderr differs for {args:?}");
    }
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

#[test]
fn unborn_status_matches_c_git() {
    let (Some(_), Some(_)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let git = Path::new(git().unwrap());
    let (c, r) = (tempdir("unborn-c"), tempdir("unborn-r"));
    for dir in [&c, &r] {
        shell_success(git, dir, &["init", "-q", "-b", "main"]);
    }
    write_tree(&c, &[("a.txt", "a\n"), ("x.log", "ignored\n")]);
    write_tree(&r, &[("a.txt", "a\n"), ("x.log", "ignored\n")]);
    shell_success(git, &c, &["add", "a.txt"]);
    shell_success(git, &r, &["add", "a.txt"]);
    check_status(&c, &r, &["status"]);
    check_status(&c, &r, &["status", "--short"]);
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}
