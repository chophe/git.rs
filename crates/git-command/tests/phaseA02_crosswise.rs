//! Crosswise tests for Phase A item A2 (`--git-dir`/`--work-tree`/global
//! option handling) against the system C git.
//! Skips when no system `git` is available.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

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
    let dir = std::env::temp_dir().join(format!("git-a2-xwise-{}-{n}", std::process::id()));
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
    let st = Command::new(g)
        .args(["init", "-q", "-b", "main"])
        .current_dir(base)
        .status()
        .unwrap();
    assert!(st.success());
    std::fs::write(base.join("a.txt"), "hello\n").unwrap();
    let st = Command::new(g)
        .args(["-c", "user.name=T", "-c", "user.email=t@example.com", "add", "."])
        .current_dir(base)
        .status()
        .unwrap();
    assert!(st.success());
    let st = Command::new(g)
        .args([
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-q",
            "-m",
            "initial",
        ])
        .current_dir(base)
        .status()
        .unwrap();
    assert!(st.success());
}

/// Run system git with a given global-args + subcommand split. Takes the
/// CWD lock: `ours()` mutates process-global env vars under the same lock, so
/// a concurrent spawn here would inherit another test's `GIT_DIR`/etc.
fn real(dir: &Path, envs: &[(&str, &str)], all_args: &[&str]) -> (String, i32) {
    let g = git().unwrap();
    with_cwd(dir, || {
        let mut c = Command::new(g);
        c.current_dir(dir);
        for (k, v) in envs {
            c.env(k, v);
        }
        let out = c.args(all_args).output().unwrap();
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        (text, out.status.code().unwrap_or(128))
    })
}

fn ours(dir: &Path, envs: &[(&str, &str)], all_args: &[&str]) -> (String, i32) {
    with_cwd(dir, || {
        for (k, v) in envs {
            std::env::set_var(k, v);
        }
        let args: Vec<String> = all_args.iter().map(|s| s.to_string()).collect();
        let (ctx, rest) = RepoContext::from_global_args(&args).unwrap();
        let cmd = rest[0].clone();
        let sub: Vec<String> = rest[1..].to_vec();
        let mut out: Vec<u8> = Vec::new();
        let code = match git_command::dispatch_with(&ctx, &cmd, &sub, &mut out) {
            Some(Ok(())) => 0,
            Some(Err(e)) => {
                if !e.message.is_empty() {
                    out.extend_from_slice(e.message.as_bytes());
                    out.push(b'\n');
                }
                e.code
            }
            None => 1,
        };
        for (k, _) in envs {
            std::env::remove_var(k);
        }
        (String::from_utf8_lossy(&out).into_owned(), code)
    })
}

/// rev-parse reporting: both implementations must agree on the resolved
/// git dir / work tree (normalized to the repo path prefix).
#[test]
fn git_dir_flag_rev_parse() {
    if git().is_none() {
        return;
    }
    let base = tempdir();
    init_repo(&base);
    let sub = base.join("deep/deeper");
    std::fs::create_dir_all(&sub).unwrap();

    let gd = base.join(".git").to_string_lossy().into_owned();
    let cases: Vec<Vec<&str>> = vec![
        vec!["--git-dir", &gd, "rev-parse", "--git-dir"],
        vec!["--work-tree", base.to_str().unwrap(), "rev-parse", "--show-toplevel"],
        vec!["-C", base.to_str().unwrap(), "rev-parse", "--git-dir"],
        vec!["--git-dir=.", "rev-parse", "--git-dir"],
    ];
    for case in cases {
        // Only run from inside the repo (work-tree tests need it).
        let (rtext, rcode) = real(&base, &[], &case);
        let (otext, ocode) = ours(&base, &[], &case);
        assert_eq!(rcode, ocode, "case {case:?}");
        assert_eq!(rtext, otext, "case {case:?}\nreal: {rtext}\nours: {otext}");
    }
    let _ = sub;
}

#[test]
fn git_dir_env_var_rev_parse() {
    if git().is_none() {
        return;
    }
    let base = tempdir();
    init_repo(&base);
    let gd = base.join(".git").to_string_lossy().into_owned();
    std::fs::create_dir_all(base.join("deep")).unwrap();

    for dir in [&base, &base.join("deep")] {
        let (rtext, rcode) = real(dir, &[("GIT_DIR", &gd)], &["rev-parse", "--git-dir"]);
        let (otext, ocode) = ours(dir, &[("GIT_DIR", &gd)], &["rev-parse", "--git-dir"]);
        assert_eq!(rcode, ocode);
        assert_eq!(rtext, otext, "real: {rtext}\nours: {otext}");
    }
}

#[test]
fn cat_file_with_git_dir_flag() {
    if git().is_none() {
        return;
    }
    let base = tempdir();
    init_repo(&base);
    let head = {
        let out = Command::new(git().unwrap())
            .args(["rev-parse", "HEAD"])
            .current_dir(&base)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let gd = base.join(".git").to_string_lossy().into_owned();

    let (rtext, rcode) = real(&base, &[], &["--git-dir", &gd, "cat-file", "-p", &head]);
    let (otext, ocode) = ours(&base, &[], &["--git-dir", &gd, "cat-file", "-p", &head]);
    assert_eq!(rcode, ocode);
    assert_eq!(rtext, otext);
}

#[test]
fn bare_flag_and_work_tree_env_agree() {
    if git().is_none() {
        return;
    }
    let base = tempdir();
    init_repo(&base);
    let wt = base.to_str().unwrap().to_string();

    // GIT_WORK_TREE alone (with GIT_DIR) reports the work tree.
    let (rtext, rcode) = real(
        &base,
        &[("GIT_DIR", ".git"), ("GIT_WORK_TREE", &wt)],
        &["rev-parse", "--show-toplevel"],
    );
    let (otext, ocode) = ours(
        &base,
        &[("GIT_DIR", ".git"), ("GIT_WORK_TREE", &wt)],
        &["rev-parse", "--show-toplevel"],
    );
    assert_eq!(rcode, ocode);
    assert_eq!(rtext, otext, "real: {rtext}\nours: {otext}");
}

#[test]
fn unknown_global_option_is_usage_error() {
    let base = tempdir();
    let args: Vec<String> = vec![
        "--definitely-not-a-global-option".to_string(),
        "rev-parse".to_string(),
    ];
    let res = RepoContext::from_global_args(&args);
    let err = res.unwrap_err();
    assert_eq!(err.code, 129);
    let _ = base;
}

#[test]
fn minus_c_cumulative() {
    if git().is_none() {
        return;
    }
    let base = tempdir();
    init_repo(&base);
    let sub = base.join("a/b");
    std::fs::create_dir_all(&sub).unwrap();

    let (rtext, rcode) = real(
        &sub,
        &[],
        &["-C", "../..", "rev-parse", "--show-toplevel"],
    );
    let (otext, ocode) = ours(
        &sub,
        &[],
        &["-C", "../..", "rev-parse", "--show-toplevel"],
    );
    assert_eq!(rcode, ocode);
    assert_eq!(rtext, otext, "real: {rtext}\nours: {otext}");
}
