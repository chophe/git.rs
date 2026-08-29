//! Crosswise tests for Phase A item A7 (pretty-printing engine) against the
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
    let dir = std::env::temp_dir().join(format!("git-a7-xwise-{}-{n}", std::process::id()));
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

/// A repo with several commits: multiline messages, punctuation subjects,
/// a merge, and fixed dates for deterministic output.
fn build_fixture() -> Option<PathBuf> {
    let g = git()?;
    let dir = tempdir();
    let env = [
        ("GIT_AUTHOR_NAME", "A U Thor"),
        ("GIT_AUTHOR_EMAIL", "author@example.com"),
        ("GIT_COMMITTER_NAME", "C O Mitter"),
        ("GIT_COMMITTER_EMAIL", "committer@example.com"),
    ];
    let commit = |msg: &str, date: &str| {
        let mut c = Command::new(g);
        c.args(["commit", "-q", "--allow-empty", "-m", msg]);
        for (k, v) in env {
            c.env(k, v);
        }
        c.env("GIT_AUTHOR_DATE", date).env("GIT_COMMITTER_DATE", date);
        let st = c.current_dir(&dir).status().unwrap();
        assert!(st.success());
    };
    let st = Command::new(g).args(["init", "-q", "-b", "main"]).current_dir(&dir).status().unwrap();
    assert!(st.success());
    commit("initial commit", "2020-02-18 11:11:14 +0000");
    std::fs::write(dir.join("a.txt"), b"one\n").unwrap();
    let mut c = Command::new(g);
    c.args(["-c", "user.name=T", "-c", "user.email=t@e.c", "add", "."]);
    c.current_dir(&dir);
    assert!(c.status().unwrap().success());
    commit("second: subject with.punctuation!? -- dashes...", "2020-06-01 12:00:00 +0530");
    let mut c = Command::new(g);
    c.args(["commit", "-q", "--allow-empty", "-m", "multi line subject", "-m", "and a body\nwith lines"]);
    for (k, v) in env {
        c.env(k, v);
    }
    c.env("GIT_AUTHOR_DATE", "2021-11-05 03:04:05 -0700").env("GIT_COMMITTER_DATE", "2021-11-05 03:04:05 -0700");
    assert!(c.current_dir(&dir).status().unwrap().success());
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
fn builtin_formats_match() {
    if git().is_none() {
        return;
    }
    let dir = build_fixture().unwrap();
    for fmt in ["oneline", "short", "medium", "full", "fuller", "raw", "reference"] {
        check(&dir, &["log", &format!("--pretty={fmt}")]);
    }
    check(&dir, &["log"]);
    check(&dir, &["log", "--oneline"]);
}

#[test]
fn user_formats_match() {
    if git().is_none() {
        return;
    }
    let dir = build_fixture().unwrap();
    let fmts = [
        "%H|%T|%h|%t|%P|%p",
        "%an|%ae|%cn|%ce",
        "%ad|%aD|%ar|%at|%ai|%aI",
        "%cd|%cD|%cr|%ct|%ci|%cI",
        "%s|%f|%b|%B",
        "%e|%n|%%|%x41",
        "%h %s (body %b)",
        "%C(red)%s%C(reset)%n%s",
        "%ad" ,
    ];
    for fmt in fmts {
        check(&dir, &["log", &format!("--format={fmt}")]);
    }
}

#[test]
fn date_modes_match() {
    if git().is_none() {
        return;
    }
    let dir = build_fixture().unwrap();
    for mode in [
        "default", "iso", "iso-strict", "rfc", "short", "raw", "unix",
    ] {
        check(&dir, &["log", &format!("--date={mode}"), "--format=%ad|%cd"]);
    }
    // Relative/human depend on "now"; they only need to parse and be stable.
    check(&dir, &["log", "--date=relative", "--format=%ad"]);
}

#[test]
fn invalid_format_errors_match() {
    if git().is_none() {
        return;
    }
    let dir = build_fixture().unwrap();
    check(&dir, &["log", "--pretty=definitely-not-a-format"]);
}
