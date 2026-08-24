//! Crosswise tests for Phase 6 (index & status) against the system C git.
//! Skips when no system `git` is available.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use git_command::Command as GitCommand;
use git_command::{ls_files, status, update_index};
use git_command::CommandError;

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
    let dir = std::env::temp_dir().join(format!("git-p6-xwise-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn run(git: &str, dir: &Path, args: &[&str]) -> String {
    String::from_utf8(Command::new(git).args(args).current_dir(dir).output().expect("git").stdout).unwrap()
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

fn ours<C: GitCommand>(cmd: &C, args: &[&str]) -> Result<String, CommandError> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    cmd.run(&args, &mut out)?;
    Ok(String::from_utf8(out).unwrap())
}

/// Build a repo with a couple of files and one commit.
fn build_repo() -> PathBuf {
    let git = git().unwrap();
    let dir = tempdir();
    run(git, &dir, &["init", "-q"]);
    run(git, &dir, &["config", "user.name", "T"]);
    run(git, &dir, &["config", "user.email", "t@e.com"]);
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    std::fs::write(dir.join("b.txt"), "two\n").unwrap();
    run(git, &dir, &["add", "-A"]);
    run(git, &dir, &["commit", "-qm", "base"]);
    dir
}

#[test]
fn our_index_is_read_by_real_git() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    // Our update-index adds two files.
    std::fs::write(dir.join("x.txt"), "xx\n").unwrap();
    std::fs::write(dir.join("y.txt"), "yy\n").unwrap();
    let res = with_cwd(&dir, || ours(&update_index::UpdateIndex, &["--add", "x.txt", "y.txt"]));
    res.expect("our update-index runs");

    let ours_stage = with_cwd(&dir, || ours(&ls_files::LsFiles, &["--stage"])).expect("ls-files");
    let real_stage = run(git, &dir, &["ls-files", "--stage"]);
    assert_eq!(ours_stage, real_stage, "stage listing mismatch");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn we_read_real_git_index() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    std::fs::write(dir.join("x.txt"), "xx\n").unwrap();
    std::fs::write(dir.join("y.txt"), "yy\n").unwrap();
    run(git, &dir, &["update-index", "--add", "x.txt", "y.txt"]);

    let ours_stage = with_cwd(&dir, || ours(&ls_files::LsFiles, &["--stage"])).expect("ls-files");
    let real_stage = run(git, &dir, &["ls-files", "--stage"]);
    assert_eq!(ours_stage, real_stage, "stage listing mismatch");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn status_porcelain_matches_real_git() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    // A modified tracked file, an added file, a deleted file, and an
    // untracked file.
    std::fs::write(dir.join("a.txt"), "one modified\n").unwrap();
    std::fs::write(dir.join("c.txt"), "new\n").unwrap();
    std::fs::write(dir.join("u.txt"), "untracked\n").unwrap();
    with_cwd(&dir, || ours(&update_index::UpdateIndex, &["--add", "c.txt"])).expect("update-index");
    with_cwd(&dir, || ours(&update_index::UpdateIndex, &["--remove", "b.txt"])).expect("remove b.txt");
    std::fs::remove_file(dir.join("b.txt")).ok();

    let ours_status = with_cwd(&dir, || ours(&status::Status, &["--porcelain"])).expect("status");
    let real_status = run(git, &dir, &["status", "--porcelain"]);
    assert_eq!(ours_status, real_status, "status --porcelain mismatch");
    let _ = std::fs::remove_dir_all(&dir);
}