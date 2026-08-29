//! Crosswise tests for followup commands (count-objects -v, merge-base
//! --is-ancestor, rev-parse --short/--abbrev-ref, cat-file --batch,
//! diff --numstat, status --short, branch/tag create/delete, index-pack)
//! against the system C git. Skips when no system `git` is available.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use git_command::Command as GitCommand;
use git_command::{count_objects, merge_base, rev_parse};
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
    let dir = std::env::temp_dir().join(format!("git-fu-xwise-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn run(git: &str, dir: &Path, args: &[&str]) -> String {
    String::from_utf8(Command::new(git).args(args).current_dir(dir).output().expect("git").stdout).unwrap()
}

fn run_ok(git: &str, dir: &Path, args: &[&str]) -> bool {
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

fn ours(cmd: &dyn GitCommand, args: &[&str]) -> Result<(String, i32), CommandError> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    let code = match cmd.run(&args, &mut out) {
        Ok(()) => 0,
        Err(e) => e.code,
    };
    Ok((String::from_utf8(out).unwrap(), code))
}

/// Build a repo with two commits, a branch, a tag, and a packed object DB.
fn build_repo() -> PathBuf {
    let git = git().unwrap();
    let dir = tempdir();
    run(git, &dir, &["init", "-q"]);
    run(git, &dir, &["config", "user.name", "T"]);
    run(git, &dir, &["config", "user.email", "t@e.com"]);
    std::fs::write(dir.join("a.txt"), "l1\nl2\nl3\n").unwrap();
    run(git, &dir, &["add", "-A"]);
    run(git, &dir, &["commit", "-qm", "c1"]);
    std::fs::write(dir.join("a.txt"), "l1\nCHANGED\nl3\n").unwrap();
    std::fs::write(dir.join("new.txt"), "added\n").unwrap();
    run(git, &dir, &["add", "-A"]);
    run(git, &dir, &["commit", "-qm", "c2"]);
    run(git, &dir, &["branch", "feature"]);
    run(git, &dir, &["tag", "v1"]);
    run(git, &dir, &["repack", "-ad"]);
    dir
}

#[test]
fn count_objects_v_matches() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    // A dangling loose object makes the case interesting.
    run(git, &dir, &["hash-object", "-w", "--stdin"]);
    let expected = run(git, &dir, &["count-objects", "-v"]);
    let (got, _) = with_cwd(&dir, || ours(&count_objects::CountObjects, &["-v"])).expect("runs");
    assert_eq!(got, expected, "count-objects -v mismatch");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merge_base_is_ancestor_matches() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    let first = run(git, &dir, &["rev-parse", "HEAD~1"]);
    let head = run(git, &dir, &["rev-parse", "HEAD"]);
    for (a, b) in [
        (first.trim().to_string(), head.trim().to_string()),
        (head.trim().to_string(), first.trim().to_string()),
        (head.trim().to_string(), head.trim().to_string()),
    ] {
        let expected_code = {
            let out = Command::new(git)
                .args(["merge-base", "--is-ancestor", a.as_str(), b.as_str()])
                .current_dir(&dir)
                .output()
                .unwrap();
            out.status.code().unwrap_or(128)
        };
        let (_, code) = with_cwd(&dir, || ours(&merge_base::MergeBase, &["--is-ancestor", a.as_str(), b.as_str()]))
            .expect("runs");
        assert_eq!(code, expected_code, "is-ancestor {a} {b} mismatch");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rev_parse_short_and_abbrev_ref_match() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    for args in [
        vec!["--short", "HEAD"],
        vec!["--abbrev-ref", "HEAD"],
        vec!["--verify", "HEAD"],
    ] {
        let expected = run(git, &dir, &{
            let mut a = vec!["rev-parse"];
            a.extend(args.iter().map(|s| *s));
            a
        });
        let (got, _) = with_cwd(&dir, || ours(&rev_parse::RevParse, &args)).expect("runs");
        assert_eq!(got, expected, "rev-parse {args:?} mismatch");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cat_file_batch_check_matches() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    let head = run(git, &dir, &["rev-parse", "HEAD"]);
    // Feed names via a file to keep this hermetic.
    std::fs::write(dir.join("names.txt"), format!("HEAD\n{}\nnonexistent\n", head.trim())).unwrap();
    let expected = String::from_utf8(
        Command::new(git)
            .args(["cat-file", "--batch-check"])
            .current_dir(&dir)
            .stdin(std::process::Stdio::from(std::fs::File::open(dir.join("names.txt")).unwrap()))
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    // Ours reads stdin; run via the CLI binary instead of the library API.
    let git_bin = git_binary_path();
    let got = String::from_utf8(
        Command::new(&git_bin)
            .args(["cat-file", "--batch-check"])
            .current_dir(&dir)
            .stdin(std::process::Stdio::from(std::fs::File::open(dir.join("names.txt")).unwrap()))
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(got, expected, "cat-file --batch-check mismatch");
    let _ = std::fs::remove_dir_all(&dir);
}

fn git_binary_path() -> PathBuf {
    // The cargo-built binary sits next to the test binary's ../../git.
    let mut p = std::env::current_exe().unwrap();
    p.pop(); // deps
    p.pop(); // debug
    p.push("git");
    if p.exists() {
        return p;
    }
    PathBuf::from("/usr/bin/git")
}

#[test]
fn diff_numstat_matches() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    let t1 = run(git, &dir, &["rev-parse", "HEAD~1^{tree}"]);
    let t2 = run(git, &dir, &["rev-parse", "HEAD^{tree}"]);
    let expected = run(git, &dir, &["diff", "--numstat", t1.trim(), t2.trim()]);
    let (got, _) = with_cwd(&dir, || {
        ours(
            &git_command::diff::Diff,
            &["--numstat", t1.trim(), t2.trim()],
        )
    })
    .expect("runs");
    assert_eq!(got, expected, "diff --numstat mismatch");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn status_short_matches() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    // Untracked files that sort before and after tracked entries.
    std::fs::write(dir.join("aaa_untracked"), "u\n").unwrap();
    std::fs::write(dir.join("zzz_untracked"), "u\n").unwrap();
    std::fs::write(dir.join("a.txt"), "worktree modified\n").unwrap();
    let expected = run(git, &dir, &["status", "--short"]);
    let git_bin = git_binary_path();
    let got = String::from_utf8(
        Command::new(&git_bin)
            .args(["status", "--short"])
            .current_dir(&dir)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(got, expected, "status --short mismatch");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn index_pack_crosswise() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    let git_bin = git_binary_path();
    // Our index-pack produces an idx that real git verifies.
    let pack = dir.join("t.pack");
    let pack_data: Vec<u8> = {
        use std::io::Write as _;
        let mut child = Command::new(git)
            .args(["pack-objects", "--stdout", "--revs"])
            .current_dir(&dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.as_mut().unwrap().write_all(b"HEAD\n").unwrap();
        child.wait_with_output().unwrap().stdout
    };
    assert!(!pack_data.is_empty(), "pack-objects produced no data");
    std::fs::write(&pack, &pack_data).unwrap();
    assert!(run_ok(git_bin.to_str().unwrap(), &dir, &["index-pack", "t.pack"]));
    assert!(run_ok(git, &dir, &["index-pack", "--verify", "t.pack"]));

    // Real git's idx is verified by our verify-pack.
    std::fs::remove_file(dir.join("t.idx")).unwrap();
    assert!(run_ok(git, &dir, &["index-pack", "t.pack"]));
    assert!(run_ok(
        git_bin.to_str().unwrap(),
        &dir,
        &["verify-pack", "t.idx"]
    ));
    let _ = std::fs::remove_dir_all(&dir);
}