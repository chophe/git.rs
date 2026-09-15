//! Crosswise tests for Phase B item B3 (`git add`) against the system C git.
//! Skips when no system `git` is available.
//!
//! `git add` writes stat-accurate index entries, so the whole index is
//! expected to be byte-identical to C git's after the same operation on the
//! same worktree (same filesystem → same dev/ino/mtime/ctime).

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
    let dir = std::env::temp_dir().join(format!("git-b3-xwise-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn run(exe: &Path, dir: &Path, args: &[&str]) -> Output {
    Command::new(exe)
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", tempdir("h"))
        .env("TMPDIR", std::env::temp_dir())
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .expect("spawn")
}

/// Give the index file a far-future mtime so no entry is "racily clean"
/// (`ce_mtime >= index_mtime`), making C's `ie_match_stat`/racy handling
/// deterministic and identical for both implementations.
fn pin_index_mtime(path: &Path) {
    let t = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
    if let Ok(f) = std::fs::File::options().write(true).open(path) {
        let _ = f.set_modified(t);
    }
}

struct Report {
    stdout: String,
    stderr: String,
    code: i32,
    index: Option<Vec<u8>>,
}

fn add_report(exe: &Path, dir: &Path, args: &[&str]) -> Report {
    let index = dir.join(".git/index");
    let out = run(exe, dir, args);
    Report {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        code: out.status.code().unwrap_or(128),
        index: std::fs::read(&index).ok(),
    }
}

/// Compare C vs Rust for `git add <args>` after resetting the index to
/// `snapshot` (or absent) before each run.
fn check_add(dir: &Path, snapshot: Option<&[u8]>, args: &[&str]) {
    let (git, rust) = (git().unwrap(), rust_git().unwrap());
    let index = dir.join(".git/index");
    let restore = |snap: Option<&[u8]>| match snap {
        Some(b) => {
            std::fs::write(&index, b).unwrap();
            pin_index_mtime(&index);
        }
        None => {
            let _ = std::fs::remove_file(&index);
        }
    };

    restore(snapshot);
    let c = add_report(Path::new(git), dir, args);
    restore(snapshot);
    let r = add_report(&rust, dir, args);

    assert_eq!(r.stdout, c.stdout, "stdout differs for add {args:?}");
    assert_eq!(r.stderr, c.stderr, "stderr differs for add {args:?}");
    assert_eq!(r.code, c.code, "exit differs for add {args:?}");
    assert_eq!(r.index, c.index, "index differs for add {args:?}");
}

/// A repo with a nested tree, an executable, a symlink, and an ignored file.
fn build_fixture(git: &str, dir: &Path) {
    run(Path::new(git), dir, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(dir.join("sub/deep")).unwrap();
    std::fs::write(dir.join("a.txt"), "alpha\n").unwrap();
    std::fs::write(dir.join("z.txt"), "zeta\n").unwrap();
    std::fs::write(dir.join("sub/b.txt"), "beta\n").unwrap();
    std::fs::write(dir.join("sub/deep/c.txt"), "gamma\n").unwrap();
    std::fs::write(dir.join(".gitignore"), "*.log\n").unwrap();
    std::fs::write(dir.join("x.log"), "ignored\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.join("sub/b.txt"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::os::unix::fs::symlink("a.txt", dir.join("link.txt"));
    }
    let out = run(Path::new(git), dir, &["add", "-A"]);
    assert!(out.status.success());
    let out = run(Path::new(git), dir, &["commit", "-qm", "init"]);
    assert!(out.status.success(), "commit failed: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn add_all_and_update_match_c() {
    let (Some(git), Some(_rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let dir = tempdir("all");
    build_fixture(git, &dir);
    let snap = std::fs::read(dir.join(".git/index")).unwrap();

    // Nothing changed: refreshing must not perturb the index.
    check_add(&dir, Some(&snap), &["add", "-A"]);
    check_add(&dir, Some(&snap), &["add", "-u"]);

    // Modify, add a new file, delete a tracked file.
    std::fs::write(dir.join("a.txt"), "alpha changed\n").unwrap();
    std::fs::write(dir.join("new.txt"), "new\n").unwrap();
    std::fs::remove_file(dir.join("sub/deep/c.txt")).unwrap();
    check_add(&dir, Some(&snap), &["add", "-A"]);
    check_add(&dir, Some(&snap), &["add", "-u"]);
    check_add(&dir, Some(&snap), &["add", "."]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn add_pathspecs_and_globs_match_c() {
    let (Some(git), Some(_rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let dir = tempdir("spec");
    build_fixture(git, &dir);
    let snap = std::fs::read(dir.join(".git/index")).unwrap();
    std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
    std::fs::write(dir.join("new.txt"), "new\n").unwrap();

    for args in [
        vec!["add", "a.txt"],
        vec!["add", "sub"],
        vec!["add", "sub/b.txt"],
        vec!["add", "*.txt"],
        vec!["add", "sub/*"],
        vec!["add", "-n", "-A"],
        vec!["add", "-v", "a.txt"],
        vec!["add", "nope"],
        vec!["add"],
    ] {
        check_add(&dir, Some(&snap), &args);
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn add_ignored_and_force_match_c() {
    let (Some(git), Some(_rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let dir = tempdir("ignore");
    build_fixture(git, &dir);
    let snap = std::fs::read(dir.join(".git/index")).unwrap();
    std::fs::write(dir.join("y.log"), "ignored\n").unwrap();

    // Explicit ignored file: warning + exit 1, index unchanged.
    check_add(&dir, Some(&snap), &["add", "y.log"]);
    // Glob matching only ignored files: "did not match".
    check_add(&dir, Some(&snap), &["add", "*.log"]);
    // Directory traversal skips ignored files silently.
    check_add(&dir, Some(&snap), &["add", "."]);
    // `-f` stages them.
    check_add(&dir, Some(&snap), &["add", "-f", "y.log"]);
    check_add(&dir, Some(&snap), &["add", "-f", "-A"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn add_from_subdir_matches_c() {
    let (Some(git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let dir = tempdir("subdir");
    build_fixture(git, &dir);
    let snap = std::fs::read(dir.join(".git/index")).unwrap();
    std::fs::write(dir.join("sub/b.txt"), "changed\n").unwrap();
    std::fs::write(dir.join("a.txt"), "changed too\n").unwrap();

    let sub = dir.join("sub");
    let index = dir.join(".git/index");
    let restore = |s: &[u8]| {
        std::fs::write(&index, s).unwrap();
        pin_index_mtime(&index);
    };
    for args in [vec!["add", "."], vec!["add", "b.txt"], vec!["add", "../a.txt"]] {
        restore(&snap);
        let c = run(Path::new(git), &sub, &args);
        let c_index = std::fs::read(&index).unwrap();
        restore(&snap);
        let r = run(&rust, &sub, &args);
        let r_index = std::fs::read(&index).unwrap();
        assert_eq!(String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&c.stdout), "{args:?}");
        assert_eq!(String::from_utf8_lossy(&r.stderr), String::from_utf8_lossy(&c.stderr), "{args:?}");
        assert_eq!(r.status.code(), c.status.code(), "{args:?}");
        assert_eq!(r_index, c_index, "index differs for {args:?}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}
