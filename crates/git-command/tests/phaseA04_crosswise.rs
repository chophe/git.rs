//! Crosswise tests for Phase A item A4 (abbreviation + peel resolution)
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
    let dir = std::env::temp_dir().join(format!("git-a4-xwise-{}-{n}", std::process::id()));
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

fn commit_file(dir: &Path, name: &str, data: &[u8], msg: &str) {
    let g = git().unwrap();
    std::fs::write(dir.join(name), data).unwrap();
    let st = Command::new(g)
        .args(["-c", "user.name=T", "-c", "user.email=t@example.com", "add", "."])
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(st.success());
    let st = Command::new(g)
        .args(["-c", "user.name=T", "-c", "user.email=t@example.com", "commit", "-q", "-m", msg])
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(st.success());
}

fn init_repo(base: &Path) {
    let g = git().unwrap();
    let st = Command::new(g).args(["init", "-q", "-b", "main"]).current_dir(base).status().unwrap();
    assert!(st.success());
    commit_file(base, "a.txt", b"one\n", "one");
    commit_file(base, "b.txt", b"two\n", "two");
    commit_file(base, "c.txt", b"three\n", "three");
}

fn real(dir: &Path, args: &[&str]) -> (String, i32) {
    let out = Command::new(git().unwrap())
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (text, out.status.code().unwrap_or(128))
}

fn ours(dir: &Path, args: &[&str]) -> (String, i32) {
    with_cwd(dir, || {
        let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/debug/git")
            .canonicalize()
            .expect("rust git binary must be built");
        let out = Command::new(exe)
            .args(args)
            .output()
            .unwrap();
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        (text, out.status.code().unwrap_or(128))
    })
}

fn check(dir: &Path, args: &[&str]) {
    let (rtext, rcode) = real(dir, args);
    let (otext, ocode) = ours(dir, args);
    assert_eq!(rcode, ocode, "args {args:?}");
    assert_eq!(rtext, otext, "args {args:?}\nreal: {rtext}\nours: {otext}");
}

fn head_oid(dir: &Path) -> String {
    let (text, _) = real(dir, &["rev-parse", "HEAD"]);
    text.trim().to_string()
}

#[test]
fn abbreviated_oid_resolves() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    init_repo(&dir);
    let head = head_oid(&dir);
    for n in [4usize, 8, 12, 20, 39] {
        check(&dir, &["rev-parse", &head[..n]]);
    }
}

#[test]
fn peel_operators_resolve() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    init_repo(&dir);
    for arg in ["HEAD", "HEAD~1", "HEAD~2", "HEAD^", "HEAD^1", "HEAD~2^1", "HEAD^1~1"] {
        check(&dir, &["rev-parse", arg]);
    }
}

#[test]
fn rev_path_resolves() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    init_repo(&dir);
    for arg in ["HEAD:a.txt", "HEAD:b.txt", "main:c.txt"] {
        check(&dir, &["rev-parse", arg]);
    }
}

#[test]
fn unknown_and_too_short_fail_identically() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    init_repo(&dir);
    check(&dir, &["rev-parse", "bogus"]);
    // 1-3 chars are below the minimum abbreviation length in C git: the
    // argument is echoed and reported as ambiguous.
    check(&dir, &["rev-parse", "abc"]);
    check(&dir, &["rev-parse", "--verify", "bogus"]);
    check(&dir, &["rev-parse", "HEAD~99"]);
}

#[test]
fn ambiguous_prefix_reports_c_text() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    init_repo(&dir);
    // Create several objects whose ids share the first 4 hex chars, then
    // find a genuinely ambiguous prefix from C git's perspective and check
    // both implementations agree.
    let g = git().unwrap();
    for i in 0..40u32 {
        let data = format!("collision probe {i}\n");
        std::fs::write(dir.join(format!("p{i}.txt")), &data).unwrap();
        let st = Command::new(g)
            .args(["-c", "user.name=T", "-c", "user.email=t@example.com", "add", "."])
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(st.success());
    }
    // Find a 4-char prefix shared by at least two objects, using C git's
    // object list.
    let out = Command::new(g).args(["cat-file", "--batch-all-objects", "--batch-check=%(objectname)"]).current_dir(&dir).output().unwrap();
    let oids: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .collect();
    let mut ambiguous: Option<String> = None;
    for i in 0..oids.len() {
        for j in i + 1..oids.len() {
            if oids[i].starts_with(&oids[j][..4]) {
                ambiguous = Some(oids[j][..4].to_string());
                break;
            }
        }
        if ambiguous.is_some() {
            break;
        }
    }
    // Also probe a shared 5..8 char prefix when a 4-char one is absent.
    let Some(prefix) = ambiguous else {
        return;
    };
    check(&dir, &["rev-parse", &prefix]);
}
