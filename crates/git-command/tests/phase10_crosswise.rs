//! Crosswise tests for Phase 10 stretch (`git apply`) against the system C
//! git. Skips when no system `git` is available.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use git_command::Command as GitCommand;
use git_command::apply::Apply;
use git_command::RepoContext;

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
    let dir = std::env::temp_dir().join(format!("git-p10-xwise-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn run(git: &str, dir: &Path, args: &[&str]) -> bool {
    Command::new(git).args(args).current_dir(dir).output().unwrap().status.success()
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

#[test]
fn apply_matches_real_git() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = tempdir();
    run(git, &dir, &["init", "-q"]);
    run(git, &dir, &["config", "user.name", "T"]);
    run(git, &dir, &["config", "user.email", "t@e.com"]);
    std::fs::write(dir.join("a.txt"), "line1\nline2\nline3\nline4\n").unwrap();
    std::fs::write(dir.join("b.txt"), "keep\n").unwrap();
    run(git, &dir, &["add", "-A"]);
    run(git, &dir, &["commit", "-qm", "base"]);

    // Produce a patch: modify a.txt, add c.txt, delete b.txt.
    std::fs::write(dir.join("a.txt"), "line1\nINSERTED\nline2\nline3\nline4\nEND\n").unwrap();
    std::fs::write(dir.join("c.txt"), "new file\n").unwrap();
    std::fs::remove_file(dir.join("b.txt")).unwrap();
    run(git, &dir, &["add", "-A"]);
    let patch = String::from_utf8(
        Command::new(git).args(["diff", "--cached"]).current_dir(&dir).output().unwrap().stdout,
    )
    .unwrap();
    std::fs::write(dir.join("changes.patch"), &patch).unwrap();

    // Restore the pristine base state explicitly (reset alone can leave
    // staged-new files on disk).
    let restore_base = |dir: &Path| {
        run(git, dir, &["reset", "--hard", "HEAD"]);
        std::fs::write(dir.join("a.txt"), "line1\nline2\nline3\nline4\n").unwrap();
        std::fs::write(dir.join("b.txt"), "keep\n").unwrap();
        std::fs::remove_file(dir.join("c.txt")).ok();
    };

    // Apply with ours.
    restore_base(&dir);
    with_cwd(&dir, || {
        let args: Vec<String> = vec!["changes.patch".to_string()];
        let mut out = Vec::new();
        let ctx = RepoContext::new();
        Apply.run(&ctx, &args, &mut out).expect("our apply")
    });

    // Now apply the same patch with real git on the same base state.
    let ours_a = std::fs::read(dir.join("a.txt")).unwrap();
    let ours_c = std::fs::read(dir.join("c.txt")).unwrap();
    assert!(!dir.join("b.txt").exists(), "b.txt should be deleted");

    restore_base(&dir);
    assert!(run(git, &dir, &["apply", "changes.patch"]), "real git apply failed");
    assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), ours_a, "a.txt mismatch");
    assert_eq!(std::fs::read(dir.join("c.txt")).unwrap(), ours_c, "c.txt mismatch");
    assert!(!dir.join("b.txt").exists(), "b.txt should be deleted by real git too");

    // --check on a clean base tree should pass.
    restore_base(&dir);
    let check_ok = with_cwd(&dir, || {
        let args: Vec<String> = vec!["--check".to_string(), "changes.patch".to_string()];
        let mut out = Vec::new();
        let ctx = RepoContext::new();
        Apply.run(&ctx, &args, &mut out).is_ok()
    });
    assert!(check_ok, "--check should pass on a clean tree");

    let _ = std::fs::remove_dir_all(&dir);
}