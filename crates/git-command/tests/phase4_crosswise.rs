//! Crosswise tests for Phase 4 commands (object model + revision walking)
//! against the system C git. Skips when no system `git` is available.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use git_command::Command as GitCommand;
use git_command::{cat_file, log, ls_tree, rev_list};
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
    let dir = std::env::temp_dir().join(format!("git-p4-xwise-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn run(git: &str, dir: &Path, args: &[&str]) -> String {
    String::from_utf8(
        Command::new(git).args(args).current_dir(dir).output().expect("git runs").stdout,
    )
    .unwrap()
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

/// Build a repo with a linear history, returning (dir, tree, head).
fn build_repo() -> (PathBuf, String, String) {
    let git = git().unwrap();
    let dir = tempdir();
    run(git, &dir, &["init", "-q"]);
    run(git, &dir, &["config", "user.name", "T"]);
    run(git, &dir, &["config", "user.email", "t@e.com"]);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    for (i, f) in [(0u32, "sub/a.txt"), (1, "b.txt"), (2, "sub/c.txt")] {
        std::fs::write(dir.join(f), format!("content {i}\n")).unwrap();
        run(git, &dir, &["add", "-A"]);
        run(git, &dir, &["commit", "-qm", &format!("commit {i}")]);
    }
    let tree = run(git, &dir, &["rev-parse", "HEAD^{tree}"]);
    let head = run(git, &dir, &["rev-parse", "HEAD"]);
    (dir, tree.trim().to_string(), head.trim().to_string())
}

fn ours_output<C: GitCommand>(cmd: &C, args: &[String]) -> Result<String, CommandError> {
    let mut out = Vec::new();
    cmd.run(args, &mut out)?;
    Ok(String::from_utf8(out).unwrap())
}

#[test]
fn ls_tree_matches_real_git() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let (dir, tree, _head) = build_repo();
    for (label, args) in [
        ("plain", vec![]),
        ("recursive", vec!["-r"]),
        ("recursive-t", vec!["-r", "-t"]),
        ("name-only", vec!["-r", "--name-only"]),
    ] {
        let expected = run(git, &dir, &{
            let mut a = vec!["ls-tree"];
            a.extend(args.iter().copied());
            a.push(tree.as_str());
            a
        });
        let got = with_cwd(&dir, || {
            let mut a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            a.push(tree.to_string());
            ours_output(&ls_tree::LsTree, &a)
        })
        .expect("ls-tree runs");
        assert_eq!(got, expected, "ls-tree {label} mismatch");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cat_file_matches_real_git() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let (dir, tree, head) = build_repo();
    let blob = run(git, &dir, &["ls-tree", tree.as_str(), "b.txt"])
        .split_whitespace()
        .nth(2)
        .unwrap()
        .to_string();

    let cases: Vec<(Vec<String>, String)> = vec![
        (vec!["-t".into(), tree.clone()], run(git, &dir, &["cat-file", "-t", &tree])),
        (vec!["-s".into(), tree.clone()], run(git, &dir, &["cat-file", "-s", &tree])),
        (vec!["-p".into(), tree.clone()], run(git, &dir, &["cat-file", "-p", &tree])),
        (vec!["-p".into(), head.clone()], run(git, &dir, &["cat-file", "-p", &head])),
        (vec!["-p".into(), blob.clone()], run(git, &dir, &["cat-file", "-p", &blob])),
    ];
    for (args, expected) in cases {
        let got = with_cwd(&dir, || ours_output(&cat_file::CatFile, &args)).expect("cat-file runs");
        assert_eq!(got, expected, "cat-file {args:?} mismatch");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rev_list_and_log_match_real_git() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let (dir, _tree, head) = build_repo();
    for (label, args) in [
        ("plain", vec!["rev-list"]),
        ("parents", vec!["rev-list", "--parents"]),
    ] {
        let mut a = args.clone();
        a.push(head.as_str());
        let expected = run(git, &dir, &a);
        let mut a = args.iter().skip(1).map(|s| s.to_string()).collect::<Vec<_>>();
        a.push(head.clone());
        let got = with_cwd(&dir, || ours_output(&rev_list::RevList, &a)).expect("rev-list runs");
        assert_eq!(got, expected, "rev-list {label} mismatch");
    }
    let expected = run(git, &dir, &["log", "--oneline", head.as_str()]);
    let got = with_cwd(&dir, || ours_output(&log::Log, &["--oneline".to_string(), head.clone()])).expect("log runs");
    assert_eq!(got, expected, "log --oneline mismatch");
    let _ = std::fs::remove_dir_all(&dir);
}
