//! Crosswise tests for Phase B item B8 (`git rm`, `mv`, `clean`) against
//! the system C git. Skips when no system `git` is available.
//!
//! Each case runs both binaries in fresh twin directories with identical,
//! hermetic environments and asserts byte-identical stdout/stderr/exit plus
//! identical resulting state (status, index, worktree files).

use std::collections::BTreeSet;
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
    let dir = std::env::temp_dir().join(format!("git-b8-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn hermetic(exe: &Path, dir: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(exe);
    cmd.args(args).current_dir(dir).env_clear();
    cmd.env("PATH", "/usr/bin:/bin");
    cmd.env("HOME", dir);
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
        "{exe:?} {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Fixture: committed a.txt, b.txt, sub/c.txt.
fn tracked_fixture(dir: &Path) {
    let real = Path::new(git().unwrap());
    shell_success(real, dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "a1\n").unwrap();
    std::fs::write(dir.join("b.txt"), "b1\n").unwrap();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/c.txt"), "c1\n").unwrap();
    shell_success(real, dir, &["add", "-A"]);
    shell_success(real, dir, &["commit", "-qm", "c1"]);
}

/// Worktree file inventory (relative paths, dirs with trailing slash).
fn inventory(dir: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let rel = p.strip_prefix(dir).unwrap().to_string_lossy().into_owned();
            if p.is_dir() && !p.is_symlink() {
                if rel == ".git" {
                    continue;
                }
                out.insert(format!("{rel}/"));
                stack.push(p);
            } else {
                out.insert(rel);
            }
        }
    }
    out
}

fn check_case(name: &str, setup: &dyn Fn(&Path), args: &[&str]) {
    let real = git().expect("system git required");
    let ours = rust_git().expect("rust binary required");
    let c = tempdir(&format!("{name}-c"));
    let r = tempdir(&format!("{name}-r"));
    tracked_fixture(&c);
    tracked_fixture(&r);
    setup(&c);
    setup(&r);
    let a = hermetic(Path::new(real), &c, args);
    let b = hermetic(&ours, &r, args);
    assert_eq!(a.status.code(), b.status.code(), "[{name}] exit code");
    assert_eq!(a.stdout, b.stdout, "[{name}] stdout");
    assert_eq!(a.stderr, b.stderr, "[{name}] stderr");
    let sa = hermetic(Path::new(real), &c, &["status", "--porcelain", "--untracked-files=all"]);
    let sb = hermetic(Path::new(real), &r, &["status", "--porcelain", "--untracked-files=all"]);
    assert_eq!(sa.stdout, sb.stdout, "[{name}] status");
    let ia = hermetic(Path::new(real), &c, &["ls-files", "--stage"]);
    let ib = hermetic(Path::new(real), &r, &["ls-files", "--stage"]);
    assert_eq!(ia.stdout, ib.stdout, "[{name}] index");
    assert_eq!(inventory(&c), inventory(&r), "[{name}] worktree");
    // File bytes for surviving tracked files.
    for f in ["a.txt", "b.txt", "sub/c.txt", "d.txt", "sub2/c.txt", "sub/d.txt"] {
        assert_eq!(
            std::fs::read(c.join(f)).ok(),
            std::fs::read(r.join(f)).ok(),
            "[{name}] bytes {f}"
        );
    }
}

#[test]
fn rm_mv_basics() {
    if git().is_none() || rust_git().is_none() {
        return;
    }
    let no = |_: &Path| {};
    let staged = |d: &Path| {
        std::fs::write(d.join("a.txt"), "mod\n").unwrap();
        shell_success(Path::new(git().unwrap()), d, &["add", "a.txt"]);
    };
    let wtmod = |d: &Path| {
        std::fs::write(d.join("a.txt"), "mod\n").unwrap();
    };
    let bothmod = |d: &Path| {
        std::fs::write(d.join("a.txt"), "staged\n").unwrap();
        shell_success(Path::new(git().unwrap()), d, &["add", "a.txt"]);
        std::fs::write(d.join("a.txt"), "worktree\n").unwrap();
    };
    let newfile = |d: &Path| {
        std::fs::write(d.join("u.txt"), "u\n").unwrap();
    };

    check_case("rm-basic", &no, &["rm", "a.txt"]);
    check_case("rm-multi", &no, &["rm", "b.txt", "a.txt"]);
    check_case("rm-cached", &no, &["rm", "--cached", "a.txt"]);
    check_case("rm-cached-dir", &no, &["rm", "--cached", "-r", "sub"]);
    check_case("rm-dir", &no, &["rm", "-r", "sub"]);
    check_case("rm-dir-nor", &no, &["rm", "sub"]);
    check_case("rm-dry", &no, &["rm", "-n", "a.txt"]);
    check_case("rm-quiet", &no, &["rm", "-q", "a.txt"]);
    check_case("rm-noargs", &no, &["rm"]);
    check_case("rm-nomatch", &no, &["rm", "nosuch"]);
    check_case("rm-ignore-unmatch", &no, &["rm", "--ignore-unmatch", "nosuch"]);
    check_case("rm-staged", &staged, &["rm", "a.txt"]);
    check_case("rm-wtmod", &wtmod, &["rm", "a.txt"]);
    check_case("rm-bothmod", &bothmod, &["rm", "a.txt"]);
    check_case("rm-force-both", &bothmod, &["rm", "-f", "a.txt"]);
    check_case("rm-cached-wtmod", &wtmod, &["rm", "--cached", "a.txt"]);
    check_case("rm-new-staged", &|d: &Path| {
        std::fs::write(d.join("n.txt"), "n\n").unwrap();
        shell_success(Path::new(git().unwrap()), d, &["add", "n.txt"]);
    }, &["rm", "n.txt"]);
    check_case("rm-badopt", &no, &["rm", "--bogus"]);
    check_case("rm-newfile", &newfile, &["rm", "u.txt"]);

    check_case("mv-basic", &no, &["mv", "a.txt", "c.txt"]);
    check_case("mv-verbose", &no, &["mv", "-v", "a.txt", "c.txt"]);
    check_case("mv-dry", &no, &["mv", "-n", "a.txt", "c.txt"]);
    check_case("mv-dir", &no, &["mv", "sub", "sub2"]);
    check_case("mv-into-dir", &no, &["mv", "a.txt", "sub"]);
    check_case("mv-multi", &no, &["mv", "a.txt", "b.txt", "sub"]);
    check_case("mv-exists", &no, &["mv", "a.txt", "b.txt"]);
    check_case("mv-force", &no, &["mv", "-f", "a.txt", "b.txt"]);
    check_case("mv-badsource", &no, &["mv", "nosuch", "d.txt"]);
    check_case("mv-untracked", &newfile, &["mv", "u.txt", "v.txt"]);
    check_case("mv-noargs", &no, &["mv"]);
    check_case("mv-onearg", &no, &["mv", "a.txt"]);
    check_case("mv-same", &no, &["mv", "a.txt", "a.txt"]);
    check_case("mv-nondir", &no, &["mv", "a.txt", "b.txt", "c.txt"]);
    check_case("mv-dashdash", &no, &["mv", "--", "a.txt", "q.txt"]);
    check_case("mv-badopt", &no, &["mv", "--bogus", "a.txt", "b.txt"]);
}

#[test]
fn clean_modes() {
    if git().is_none() || rust_git().is_none() {
        return;
    }
    let messy = |d: &Path| {
        std::fs::write(d.join("u.txt"), "u\n").unwrap();
        std::fs::write(d.join("x.log"), "l\n").unwrap();
        std::fs::write(d.join(".gitignore"), "*.log\n").unwrap();
        std::fs::create_dir_all(d.join("emptydir")).unwrap();
        std::fs::create_dir_all(d.join("newdir")).unwrap();
        std::fs::write(d.join("newdir/f.txt"), "f\n").unwrap();
        std::fs::write(d.join("sub/u2.txt"), "u2\n").unwrap();
    };
    check_case("clean-noforce", &messy, &["clean"]);
    check_case("clean-dry", &messy, &["clean", "-n"]);
    check_case("clean-dry-d", &messy, &["clean", "-nd"]);
    check_case("clean-dry-q", &messy, &["clean", "-nq"]);
    check_case("clean-force", &messy, &["clean", "-f"]);
    check_case("clean-force-d", &messy, &["clean", "-fd"]);
    check_case("clean-force-q", &messy, &["clean", "-fq"]);
    check_case("clean-dry-x", &messy, &["clean", "-nx"]);
    check_case("clean-dry-X", &messy, &["clean", "-nX"]);
    check_case("clean-force-X", &messy, &["clean", "-fX"]);
    check_case("clean-exclude", &messy, &["clean", "-n", "-e", "*.txt"]);
    check_case("clean-exclude-x", &messy, &["clean", "-nx", "-e", "*.log"]);
    check_case("clean-path", &messy, &["clean", "-n", "sub"]);
    check_case("clean-force-path", &messy, &["clean", "-f", "sub/u2.txt"]);
    check_case("clean-glob", &messy, &["clean", "-n", "*.txt"]);
    check_case("clean-glob-d", &messy, &["clean", "-nd", "newdir/*"]);
    check_case("clean-nested", &|d: &Path| {
        messy(d);
        std::fs::create_dir_all(d.join("nested")).unwrap();
        std::fs::write(d.join("nested/f.txt"), "f\n").unwrap();
    }, &["clean", "-nfd"]);
    check_case("clean-badopt", &messy, &["clean", "--bogus"]);
    check_case("clean-xx", &messy, &["clean", "-n", "-x", "-X"]);
    check_case("clean-requireforce-off", &messy, &["-c", "clean.requireForce=false", "clean"]);
}
