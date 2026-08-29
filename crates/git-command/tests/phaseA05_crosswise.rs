//! Crosswise tests for Phase A item A5 (rev-parse completion) against the
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
    let dir = std::env::temp_dir().join(format!("git-a5-xwise-{}-{n}", std::process::id()));
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

fn init_repo(base: &Path) {
    let g = git().unwrap();
    let st = Command::new(g).args(["init", "-q", "-b", "main"]).current_dir(base).status().unwrap();
    assert!(st.success());
    for (name, msg) in [("a.txt", "one"), ("b.txt", "two")] {
        std::fs::write(base.join(name), name).unwrap();
        let st = Command::new(g)
            .args(["-c", "user.name=T", "-c", "user.email=t@example.com", "add", "."])
            .current_dir(base)
            .status()
            .unwrap();
        assert!(st.success());
        let st = Command::new(g)
            .args(["-c", "user.name=T", "-c", "user.email=t@example.com", "commit", "-q", "-m", msg])
            .current_dir(base)
            .status()
            .unwrap();
        assert!(st.success());
    }
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
fn repo_shape_flags() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    init_repo(&dir);
    for flag in [
        "--is-bare-repository",
        "--is-shallow-repository",
        "--show-prefix",
        "--show-cdup",
        "--absolute-git-dir",
        "--shared-index-path",
        "--show-toplevel",
        "--is-inside-work-tree",
        "--git-dir",
        "--git-common-dir",
        "--local-env-vars",
    ] {
        check(&dir, &["rev-parse", flag]);
    }
    // From a subdirectory.
    std::fs::create_dir_all(dir.join("sub/deep")).unwrap();
    let sub = dir.join("sub/deep");
    for flag in ["--show-prefix", "--show-cdup", "--is-inside-work-tree"] {
        check(&sub, &["rev-parse", flag]);
    }
}

#[test]
fn ranges_and_symbolic() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    init_repo(&dir);
    check(&dir, &["rev-parse", "main..HEAD"]);
    check(&dir, &["rev-parse", "HEAD~1..HEAD"]);
    check(&dir, &["rev-parse", "HEAD...main"]);
    check(&dir, &["rev-parse", "--symbolic", "HEAD", "main"]);
    check(&dir, &["rev-parse", "--symbolic-full-name", "HEAD", "main", "bogus"]);
    check(&dir, &["rev-parse", "--abbrev-ref", "HEAD", "main"]);
    check(&dir, &["rev-parse", "--sq", "HEAD", "main"]);
}

#[test]
fn abbrev_lengths() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    init_repo(&dir);
    check(&dir, &["rev-parse", "--short", "HEAD"]);
    check(&dir, &["rev-parse", "--short=10", "HEAD"]);
    // Unrecognized options are echoed verbatim by both implementations.
    check(&dir, &["rev-parse", "--abbrev=12", "HEAD"]);
}

#[test]
fn verify_quiet_and_default() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    init_repo(&dir);
    check(&dir, &["rev-parse", "--verify", "HEAD"]);
    check(&dir, &["rev-parse", "--verify", "bogus"]);
    check(&dir, &["rev-parse", "--verify", "--quiet", "bogus"]);
    check(&dir, &["rev-parse", "--verify", "--quiet", "HEAD"]);
    check(&dir, &["rev-parse", "--sq-quote", "a b", "c\"d"]);
    check(&dir, &["rev-parse", "HEAD^^"]);
}
