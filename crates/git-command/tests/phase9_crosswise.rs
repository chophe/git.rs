//! Crosswise tests for Phase 9 (fsck) against the system C git.
//! Skips when no system `git` is available.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use git_command::Command as GitCommand;
use git_command::fsck::Fsck;

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
    let dir = std::env::temp_dir().join(format!("git-p9-xwise-{}-{n}", std::process::id()));
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

/// Real git fsck output (stdout + stderr) and exit code.
fn real_fsck(dir: &Path) -> (String, i32) {
    let out = Command::new(git().unwrap())
        .arg("fsck")
        .current_dir(dir)
        .output()
        .unwrap();
    let mut text = String::from_utf8(out.stdout).unwrap();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (text, out.status.code().unwrap_or(128))
}

/// Our fsck output and exit code.
fn our_fsck(dir: &Path) -> (String, i32) {
    with_cwd(dir, || {
        let args: Vec<String> = Vec::new();
        let mut out_buf = Vec::new();
        let code = match Fsck.run(&args, &mut out_buf) {
            Ok(()) => 0,
            Err(e) => e.code,
        };
        (String::from_utf8(out_buf).unwrap(), code)
    })
}

#[test]
fn fsck_matches_real_git() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = tempdir();
    let run = |d: &Path, args: &[&str]| {
        Command::new(git).args(args).current_dir(d).output().unwrap().status.success()
    };
    run(&dir, &["init", "-q"]);
    run(&dir, &["config", "user.name", "T"]);
    run(&dir, &["config", "user.email", "t@e.com"]);
    std::fs::write(dir.join("f"), "content\n").unwrap();
    run(&dir, &["add", "-A"]);
    run(&dir, &["commit", "-qm", "c1"]);

    // 1. Clean repo: nothing reported, exit 0.
    let (_r_text, r_code) = real_fsck(&dir);
    let (o_text, o_code) = our_fsck(&dir);
    assert_eq!(o_text, _r_text, "clean fsck output mismatch");
    assert_eq!(o_code, r_code, "clean fsck exit code mismatch");

    // 2. Add a dangling blob: both report `dangling blob <oid>`.
    let blob_oid = String::from_utf8(
        Command::new(git)
            .args(["hash-object", "-w", "--stdin"])
            .current_dir(&dir)
            .arg("dangling payload\n")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let (r_text, r_code) = real_fsck(&dir);
    let (o_text, o_code) = our_fsck(&dir);
    assert_eq!(o_text, r_text, "dangling fsck output mismatch");
    assert_eq!(o_code, r_code, "dangling fsck exit code mismatch");
    assert!(o_text.contains(&format!("dangling blob {}", blob_oid.trim())), "got: {o_text}");

    // 3. Break a referenced object: both report `missing blob <oid>` and exit 2.
    let tree_out = String::from_utf8(
        Command::new(git).args(["ls-tree", "HEAD"]).current_dir(&dir).output().unwrap().stdout,
    )
    .unwrap();
    let ref_blob = tree_out.split_whitespace().nth(2).unwrap().to_string();
    let p = dir.join(".git/objects").join(&ref_blob[..2]).join(&ref_blob[2..]);
    std::fs::remove_file(&p).ok();

    let (_r_text, r_code) = real_fsck(&dir);
    let (o_text, o_code) = our_fsck(&dir);
    assert!(
        o_text.contains(&format!("missing blob {ref_blob}")),
        "our fsck should report the missing blob, got: {o_text}"
    );
    assert_eq!(o_code, r_code, "missing fsck exit code mismatch");
    assert_eq!(o_code, 2, "expected exit 2 for missing object");

    let _ = std::fs::remove_dir_all(&dir);
}