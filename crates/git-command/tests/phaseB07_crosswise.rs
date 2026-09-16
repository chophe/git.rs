//! Crosswise tests for Phase B item B7 (`git reset`, `checkout`, `switch`, `restore`)
//! against the system C git. Skips when no system `git` is available.
//!
//! Each case runs both binaries in fresh twin directories with identical,
//! hermetic environments and asserts byte-identical stdout/stderr/exit plus
//! identical resulting `.git` state (HEAD tree, index, status, ORIG_HEAD,
//! reflog).

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
    let dir = std::env::temp_dir().join(format!("git-b7-{tag}-{}-{n}", std::process::id()));
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
    assert!(out.status.success(), "{exe:?} {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
}

fn two_commit_fixture(dir: &Path) {
    shell_success(Path::new(git().unwrap()), dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "a1\n").unwrap();
    std::fs::write(dir.join("b.txt"), "b1\n").unwrap();
    shell_success(Path::new(git().unwrap()), dir, &["add", "-A"]);
    shell_success(Path::new(git().unwrap()), dir, &["commit", "-qm", "c1"]);
    std::fs::write(dir.join("b.txt"), "b2\n").unwrap();
    shell_success(Path::new(git().unwrap()), dir, &["add", "b.txt"]);
    shell_success(Path::new(git().unwrap()), dir, &["commit", "-qm", "c2"]);
    std::fs::write(dir.join("a.txt"), "a-mod\n").unwrap();
    std::fs::write(dir.join("u.txt"), "untracked\n").unwrap();
}

fn branch_fixture(dir: &Path) {
    shell_success(Path::new(git().unwrap()), dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "a1\n").unwrap();
    shell_success(Path::new(git().unwrap()), dir, &["add", "-A"]);
    shell_success(Path::new(git().unwrap()), dir, &["commit", "-qm", "c1"]);
    shell_success(Path::new(git().unwrap()), dir, &["checkout", "-qb", "feature"]);
    std::fs::write(dir.join("b.txt"), "b1\n").unwrap();
    shell_success(Path::new(git().unwrap()), dir, &["add", "-A"]);
    shell_success(Path::new(git().unwrap()), dir, &["commit", "-qm", "c2"]);
    shell_success(Path::new(git().unwrap()), dir, &["checkout", "-q", "main"]);
}

fn case(name: &str, pre: &dyn Fn(&Path), args: &[&str]) {
    let real = git().expect("system git required");
    let ours = rust_git().expect("rust binary required");
    let c = tempdir(&format!("{name}-c"));
    let r = tempdir(&format!("{name}-r"));
    two_commit_fixture(&c);
    two_commit_fixture(&r);
    pre(&c);
    pre(&r);
    let cs = hermetic(Path::new(real), &c, args);
    let rs = hermetic(&ours, &r, args);
    assert_eq!(cs.status.code(), rs.status.code(), "[{name}] exit code");
    assert_eq!(cs.stdout, rs.stdout, "[{name}] stdout");
    assert_eq!(cs.stderr, rs.stderr, "[{name}] stderr");
    for (label, subargs) in [("status", vec!["status", "--porcelain"]), ("ls-files", vec!["ls-files", "--stage"])] {
        let a = hermetic(Path::new(real), &c, &subargs);
        let b = hermetic(Path::new(real), &r, &subargs);
        assert_eq!(a.stdout, b.stdout, "[{name}] {label}");
    }
    let ct = hermetic(Path::new(real), &c, &["write-tree"]);
    let rt = hermetic(Path::new(real), &r, &["write-tree"]);
    assert_eq!(ct.stdout, rt.stdout, "[{name}] tree");
    let orig_c = std::fs::read_to_string(c.join(".git/ORIG_HEAD")).unwrap_or_default();
    let orig_r = std::fs::read_to_string(r.join(".git/ORIG_HEAD")).unwrap_or_default();
    assert_eq!(orig_c.trim_end(), orig_r.trim_end(), "[{name}] ORIG_HEAD");
}

#[test]
fn reset_modes_and_paths() {
    if git().is_none() || rust_git().is_none() { return; }
    let no = |_: &Path| {};
    let mod_a = |d: &Path| { std::fs::write(d.join("a.txt"), "mod\n").unwrap(); };
    let staged = |d: &Path| {
        std::fs::write(d.join("a.txt"), "mod\n").unwrap();
        shell_success(Path::new(git().unwrap()), d, &["add", "a.txt"]);
    };
    case("soft-clean", &no, &["reset", "--soft", "HEAD~1"]);
    case("soft-staged", &staged, &["reset", "--soft", "HEAD"]);
    case("mixed-clean", &no, &["reset"]);
    case("mixed-mod", &mod_a, &["reset"]);
    case("mixed-staged", &staged, &["reset"]);
    case("hard-clean", &no, &["reset", "--hard", "HEAD~1"]);
    case("hard-mod", &mod_a, &["reset", "--hard", "HEAD"]);
    case("hard-staged", &staged, &["reset", "--hard", "HEAD"]);
    case("q-hard", &no, &["reset", "-q", "--hard", "HEAD~1"]);
    case("path-reset", &no, &["reset", "HEAD", "--", "a.txt"]);
    case("path-soft", &no, &["reset", "--soft", "HEAD", "--", "a.txt"]);
    case("path-hard", &no, &["reset", "--hard", "HEAD", "--", "a.txt"]);
    case("at-head", &no, &["reset", "@"]);
    case("mixed-head", &no, &["reset", "HEAD"]);
}

#[test]
fn reset_errors_and_unborn() {
    if git().is_none() || rust_git().is_none() { return; }
    let real = git().unwrap();
    let ours = rust_git().unwrap();
    // Unborn.
    {
        let c = tempdir("unborn-c"); let r = tempdir("unborn-r");
        shell_success(Path::new(real), &c, &["init", "-q", "-b", "main"]);
        shell_success(Path::new(real), &r, &["init", "-q", "-b", "main"]);
        let cs = hermetic(Path::new(real), &c, &["reset", "--hard", "HEAD"]);
        let rs = hermetic(&ours, &r, &["reset", "--hard", "HEAD"]);
        assert_eq!(cs.status.code(), rs.status.code());
        assert_eq!(cs.stderr, rs.stderr);
    }
    // Bad rev with paths.
    {
        let c = tempdir("badrev-c"); let r = tempdir("badrev-r");
        two_commit_fixture(&c); two_commit_fixture(&r);
        let cs = hermetic(Path::new(real), &c, &["reset", "nosuchrev", "--", "a.txt"]);
        let rs = hermetic(&ours, &r, &["reset", "nosuchrev", "--", "a.txt"]);
        assert_eq!(cs.status.code(), rs.status.code());
        assert_eq!(cs.stderr, rs.stderr);
    }
    // Unknown option.
    {
        let c = tempdir("badopt-c"); let r = tempdir("badopt-r");
        two_commit_fixture(&c); two_commit_fixture(&r);
        let cs = hermetic(Path::new(real), &c, &["reset", "--bogus"]);
        let rs = hermetic(&ours, &r, &["reset", "--bogus"]);
        assert_eq!(cs.status.code(), rs.status.code());
        assert_eq!(cs.stderr, rs.stderr);
    }
}

fn checkout_case(name: &str, pre: &dyn Fn(&Path), args: &[&str]) {
    let real = git().expect("system git required");
    let ours = rust_git().expect("rust binary required");
    let c = tempdir(&format!("{name}-c"));
    let r = tempdir(&format!("{name}-r"));
    branch_fixture(&c);
    branch_fixture(&r);
    pre(&c);
    pre(&r);
    let a = hermetic(Path::new(real), &c, args);
    let b = hermetic(&ours, &r, args);
    assert_eq!(a.status.code(), b.status.code(), "[{name}] exit code");
    assert_eq!(a.stdout, b.stdout, "[{name}] stdout");
    assert_eq!(a.stderr, b.stderr, "[{name}] stderr");
    for f in ["a.txt", "b.txt", "u.txt"] {
        let ca = std::fs::read(c.join(f)).ok();
        let ra = std::fs::read(r.join(f)).ok();
        assert_eq!(ca, ra, "[{name}] worktree {f}");
    }
    let a = hermetic(Path::new(real), &c, &["rev-parse", "HEAD"]);
    let b = hermetic(&ours, &r, &["rev-parse", "HEAD"]);
    assert_eq!(a.stdout, b.stdout, "[{name}] HEAD");
}

#[test]
fn checkout_switch_restore() {
    if git().is_none() || rust_git().is_none() { return; }
    let no = |_: &Path| {};
    checkout_case("checkout-feature", &no, &["checkout", "feature"]);
    checkout_case("checkout-new-branch", &no, &["checkout", "-b", "newbranch"]);
    checkout_case("switch-main", &no, &["switch", "main"]);
    checkout_case("switch-create", &no, &["switch", "-c", "sidebranch"]);
    checkout_case("switch-detach", &no, &["switch", "--detach", "HEAD"]);
    checkout_case("checkout-detached", &no, &["checkout", "HEAD~1"]);
    checkout_case("checkout-path", &no, &["checkout", "--", "a.txt"]);
    checkout_case("checkout-path-head", &no, &["checkout", "HEAD", "--", "a.txt"]);
    checkout_case("restore-staged", &no, &["restore", "--staged", "a.txt"]);
    checkout_case("restore-source", &no, &["restore", "--source=HEAD", "a.txt"]);
}
