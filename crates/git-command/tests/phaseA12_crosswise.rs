//! Crosswise tests for Phase A item A12 (local-timezone dates and idents)
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
    let dir = std::env::temp_dir().join(format!("git-a12-xwise-{}-{n}", std::process::id()));
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

fn init_repo(base: &Path) -> String {
    let g = git().unwrap();
    let st = Command::new(g).args(["init", "-q", "-b", "main"]).current_dir(base).status().unwrap();
    assert!(st.success());
    std::fs::write(base.join("a.txt"), b"hello\n").unwrap();
    let st = Command::new(g)
        .args(["-c", "user.name=T", "-c", "user.email=t@example.com", "add", "."])
        .current_dir(base)
        .status()
        .unwrap();
    assert!(st.success());
    let out = Command::new(g)
        .args(["write-tree"])
        .current_dir(base)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn run(dir: &Path, exe: &Path, tz: &str, date: &str, tree: &str) -> (String, i32) {
    with_cwd(dir, || {
        let out = Command::new(exe)
            .args(["commit-tree", tree, "-m", "tz"])
            .env("TZ", tz)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .output()
            .unwrap();
        (
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
            out.status.code().unwrap_or(128),
        )
    })
}

#[test]
fn tzless_dates_use_local_zone() {
    if git().is_none() {
        return;
    }
    let dir = tempdir();
    let tree = init_repo(&dir);
    let real_git = PathBuf::from(git().unwrap());
    let ours = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/debug/git")
        .canonicalize()
        .expect("rust git binary must be built");

    // Timezone-free and explicit-offset dates; the local zone must show up
    // in the recorded offsets (and the commit ids must match C git).
    for (tz, date) in [
        ("UTC", "2020-02-18 11:11:14"),
        ("Asia/Tehran", "2020-02-18 11:11:14"),
        ("America/New_York", "2020-07-01 11:11:14"),
        ("America/New_York", "2020-02-18 11:11:14"),
        ("UTC", "@1582024274"),
        ("UTC", "2020-02-18 11:11:14 +0530"),
        ("Asia/Tehran", "2020-02-18 11:11:14 +0530"),
    ] {
        let (rtext, rcode) = run(&dir, &real_git, tz, date, &tree);
        let (otext, ocode) = run(&dir, &ours, tz, date, &tree);
        assert_eq!(rcode, ocode, "tz={tz} date={date}");
        assert_eq!(rtext, otext, "tz={tz} date={date}\nreal: {rtext}\nours: {otext}");
    }
}
