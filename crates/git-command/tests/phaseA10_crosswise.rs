//! Crosswise tests for Phase A item A10 (`count-objects -v` close-out)
//! against the system C git. Skips when no system `git` is available.

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
    let dir = std::env::temp_dir().join(format!("git-a10-xwise-{}-{n}", std::process::id()));
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

/// Build a fixture: loose objects, a pack, a prune-packable object, and
/// planted garbage in both the pack dir and a fanout dir.
fn build_fixture() -> Option<PathBuf> {
    let g = git()?;
    let dir = tempdir();
    let run = |args: &[&str]| {
        let st = Command::new(g)
            .args(["-c", "user.name=T", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(st.success());
    };
    let st = Command::new(g).args(["init", "-q", "-b", "main"]).current_dir(&dir).status().unwrap();
    assert!(st.success());
    for (name, msg) in [("a.txt", "one"), ("b.txt", "two")] {
        std::fs::write(dir.join(name), name).unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", msg]);
    }
    run(&["repack", "-adq"]);
    // Re-add a loose object that duplicates a packed one (prune-packable).
    std::fs::write(dir.join("a.txt"), b"one\n").unwrap();
    run(&["add", "."]);
    // Garbage: stray file in the pack dir (unknown extension), a large one
    // for size accounting, and a bogus file inside a fanout dir.
    std::fs::write(dir.join(".git/objects/pack/garbage.tmp"), b"garbage\n").unwrap();
    std::fs::write(dir.join(".git/objects/pack/big.tmp"), vec![b'x'; 10000]).unwrap();
    let first_hex = "0f";
    std::fs::create_dir_all(dir.join(".git/objects").join(first_hex)).unwrap();
    std::fs::write(dir.join(".git/objects").join(first_hex).join("xx"), b"cruft\n").unwrap();
    // A file directly under objects/ is NOT garbage for C git.
    std::fs::write(dir.join(".git/objects/stray.txt"), b"not garbage\n").unwrap();
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
fn count_objects_verbose_with_garbage() {
    if git().is_none() {
        return;
    }
    let dir = build_fixture().unwrap();
    check(&dir, &["count-objects", "-v"]);
    check(&dir, &["count-objects", "-v", "-H"]);
    check(&dir, &["count-objects"]);
}

#[test]
fn count_objects_clean_repo() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    let g = git().unwrap();
    let st = Command::new(g).args(["init", "-q", "-b", "main"]).current_dir(&dir).status().unwrap();
    assert!(st.success());
    std::fs::write(dir.join("a.txt"), b"hello\n").unwrap();
    let _ = Command::new(g)
        .args(["-c", "user.name=T", "-c", "user.email=t@example.com", "add", "."])
        .current_dir(&dir)
        .status();
    check(&dir, &["count-objects", "-v"]);
    check(&dir, &["count-objects", "-v", "-H"]);
}

#[test]
fn count_objects_usage_error() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    let g = git().unwrap();
    let _ = Command::new(g).args(["init", "-q"]).current_dir(&dir).status();
    // Invalid extra argument must be a usage error (exit 129).
    let out = Command::new(git().unwrap())
        .args(["count-objects", "--frobnicate"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(129));
    let (otext, ocode) = ours(&dir, &["count-objects", "--frobnicate"]);
    assert_eq!(ocode, 129);
    assert!(otext.contains("usage: git count-objects"), "got: {otext}");
}
