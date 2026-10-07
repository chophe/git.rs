//! Crosswise tests for Phase 1 (reflog, update-ref, packed-refs, config)
//! against the tree C git (the 2.55 behavior oracle). Falls back to the
//! system C git when the tree binary is absent; only the lock-contention
//! advisory text differs there and is handled explicitly.
//!
//! Each case runs both binaries in fresh twin directories with identical,
//! hermetic environments and asserts byte-identical stdout/stderr/exit plus
//! identical resulting state (status, show-ref, packed-refs, reflogs with
//! timestamps normalized away).

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// (oracle binary, is_tree_binary). The tree binary is the behavior oracle
/// the port was probed against; the system git is older (2.50) and differs
/// in a few advisory texts.
fn oracle() -> (PathBuf, bool) {
    let tree = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../git");
    if tree.is_file() {
        return (tree.canonicalize().unwrap(), true);
    }
    for cand in ["/usr/bin/git", "/usr/local/bin/git", "/opt/homebrew/bin/git"] {
        if Path::new(cand).exists() {
            return (PathBuf::from(cand), false);
        }
    }
    panic!("no C git available (tree ./git nor system git)");
}

fn rust_git() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/git");
    p.canonicalize().expect("rust binary built")
}

fn tempdir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("git-p1-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn hermetic_env(cmd: &mut Command, dir: &Path, with_global: bool) {
    cmd.current_dir(dir).env_clear();
    cmd.env("PATH", "/usr/bin:/bin");
    cmd.env("HOME", dir);
    cmd.env("TMPDIR", std::env::temp_dir());
    cmd.env("LC_ALL", "C");
    cmd.env("GIT_CONFIG_NOSYSTEM", "1");
    if !with_global {
        cmd.env("GIT_CONFIG_GLOBAL", "/dev/null");
    }
    cmd.env("GIT_AUTHOR_NAME", "T");
    cmd.env("GIT_AUTHOR_EMAIL", "t@example.com");
    cmd.env("GIT_COMMITTER_NAME", "T");
    cmd.env("GIT_COMMITTER_EMAIL", "t@example.com");
    cmd.env("GIT_AUTHOR_DATE", "2020-01-01 10:00:00 +0000");
    cmd.env("GIT_COMMITTER_DATE", "2020-01-01 10:00:00 +0000");
}

fn hermetic_input(exe: &Path, dir: &Path, args: &[&str], input: Option<&[u8]>) -> Output {
    hermetic_cfg_input(exe, dir, args, input, false)
}

fn hermetic_cfg_input(
    exe: &Path,
    dir: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    with_global: bool,
) -> Output {
    let mut cmd = Command::new(exe);
    cmd.args(args);
    hermetic_env(&mut cmd, dir, with_global);
    if let Some(data) = input {
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn");
        child.stdin.take().unwrap().write_all(data).unwrap();
        child.wait_with_output().expect("wait")
    } else {
        cmd.output().expect("spawn")
    }
}

fn shell_success(exe: &Path, dir: &Path, args: &[&str]) {
    let out = hermetic(exe, dir, args);
    assert!(
        out.status.success(),
        "{exe:?} {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn hermetic(exe: &Path, dir: &Path, args: &[&str]) -> Output {
    hermetic_input(exe, dir, args, None)
}

/// Path spellings that may appear in output: the canonical dir and, on
/// macOS, its non-canonical `/tmp` alias (C prints argv paths verbatim).
fn dir_variants(dir: &Path) -> Vec<String> {
    let s = dir.to_string_lossy().into_owned();
    let mut out = vec![s.clone()];
    if let Some(stripped) = s.strip_prefix("/private") {
        out.push(stripped.to_string());
    }
    out
}

fn norm(dir: &Path, bytes: &[u8]) -> Vec<u8> {
    let mut s = String::from_utf8_lossy(bytes).into_owned();
    for v in dir_variants(dir) {
        s = s.replace(&v, "<DIR>");
    }
    s.into_bytes()
}

/// Reflog files carry wall-clock timestamps; normalize `<epoch> <tz>` runs.
fn norm_reflog(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // ` <10 digits> <+|-><4 digits>` timestamp shape, followed by
        // a tab (message) or a bare newline (no message).
        if bytes[i] == b' '
            && i + 17 < bytes.len()
            && bytes[i + 1..i + 11].iter().all(|b| b.is_ascii_digit())
            && bytes[i + 11] == b' '
            && (bytes[i + 12] == b'+' || bytes[i + 12] == b'-')
            && bytes[i + 13..i + 17].iter().all(|b| b.is_ascii_digit())
            && (bytes[i + 17] == b'\t' || bytes[i + 17] == b'\n')
        {
            out.extend_from_slice(b" <TS>");
            out.push(bytes[i + 17]);
            i += 18;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn read_opt(dir: &Path, rel: &str) -> Option<Vec<u8>> {
    std::fs::read(dir.join(rel)).ok()
}

/// Identical twins with identical setup (setup runs under the oracle).
fn pair(tag: &str, setup: &dyn Fn(&Path, &Path)) -> (PathBuf, PathBuf) {
    let (real, _) = oracle();
    let c = tempdir(&format!("{tag}-c"));
    let r = tempdir(&format!("{tag}-r"));
    setup(&real, &c);
    setup(&real, &r);
    (c, r)
}

/// Run `args` under both binaries in their twin dirs; assert byte-identical
/// exit/stdout/stderr (dir paths normalized).
fn both(desc: &str, c: &Path, r: &Path, args: &[&str], input: Option<&[u8]>) {
    both_cfg(desc, c, r, args, input, false)
}

fn both_cfg(
    desc: &str,
    c: &Path,
    r: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    with_global: bool,
) {
    let (real, _) = oracle();
    let ours = rust_git();
    let a = hermetic_cfg_input(&real, c, args, input, with_global);
    let b = hermetic_cfg_input(&ours, r, args, input, with_global);
    assert_eq!(a.status.code(), b.status.code(), "[{desc}] exit code");
    assert_eq!(norm(c, &a.stdout), norm(r, &b.stdout), "[{desc}] stdout");
    assert_eq!(norm(c, &a.stderr), norm(r, &b.stderr), "[{desc}] stderr");
}

/// Identical observable repo state: status, all refs, packed-refs bytes,
fn same_state(desc: &str, c: &Path, r: &Path) {
    let (real, _) = oracle();
    let sa = hermetic(&real, c, &["status", "--porcelain", "--untracked-files=all"]);
    let sb = hermetic(&real, r, &["status", "--porcelain", "--untracked-files=all"]);
    assert_eq!(sa.stdout, sb.stdout, "[{desc}] status");
    let ra = hermetic(&real, c, &["show-ref"]);
    let rb = hermetic(&real, r, &["show-ref"]);
    assert_eq!(
        (ra.status.code(), norm(c, &ra.stdout), norm(c, &ra.stderr)),
        (rb.status.code(), norm(r, &rb.stdout), norm(r, &rb.stderr)),
        "[{desc}] show-ref"
    );
    assert_eq!(
        read_opt(c, ".git/packed-refs"),
        read_opt(r, ".git/packed-refs"),
        "[{desc}] packed-refs"
    );
    for log in ["logs/HEAD", "logs/refs/heads/main", "logs/refs/heads/feat"] {
        let la = read_opt(c, &format!(".git/{log}")).map(|b| norm_reflog(&b));
        let lb = read_opt(r, &format!(".git/{log}")).map(|b| norm_reflog(&b));
        assert_eq!(la, lb, "[{desc}] {log}");
    }
    assert_eq!(
        read_opt(c, ".git/config"),
        read_opt(r, ".git/config"),
        "[{desc}] .git/config"
    );
    assert_eq!(
        read_opt(c, ".gitconfig"),
        read_opt(r, ".gitconfig"),
        "[{desc}] ~/.gitconfig"
    );
}

fn commit_fixture(real: &Path, dir: &Path, msgs: &[&str]) {
    shell_success(real, dir, &["init", "-q", "-b", "main"]);
    shell_success(real, dir, &["config", "user.name", "T"]);
    shell_success(real, dir, &["config", "user.email", "t@example.com"]);
    for (i, m) in msgs.iter().enumerate() {
        std::fs::write(dir.join("a.txt"), format!("content {i}\n")).unwrap();
        shell_success(real, dir, &["add", "-A"]);
        shell_success(real, dir, &["commit", "-qm", m]);
    }
}

fn sha(real: &Path, dir: &Path, rev: &str) -> String {
    let out = hermetic(real, dir, &["rev-parse", rev]);
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn reflog_show_after_mutations() {
    let (c, r) = pair("reflog-show", &|real, dir| {
        commit_fixture(real, dir, &["c1", "c2"]);
        shell_success(real, dir, &["branch", "feat"]);
        shell_success(real, dir, &["reset", "-q", "--hard", "HEAD~1"]);
        shell_success(real, dir, &["checkout", "-q", "-b", "side"]);
        shell_success(real, dir, &["checkout", "-q", "main"]);
    });
    for (desc, args) in [
        ("show-head", vec!["reflog", "show", "HEAD"]),
        ("show-default", vec!["reflog"]),
        ("show-main", vec!["reflog", "show", "refs/heads/main"]),
        ("show-feat", vec!["reflog", "show", "feat"]),
        ("show-side", vec!["reflog", "show", "side"]),
        ("show-bad-ref", vec!["reflog", "show", "nosuch"]),
    ] {
        both(desc, &c, &r, &args, None);
    }
    same_state("reflog-show", &c, &r);
}

#[test]
fn reflog_recovery_from_prior_entry() {
    let (c, r) = pair("reflog-recover", &|real, dir| {
        commit_fixture(real, dir, &["c1", "c2", "c3"]);
    });
    // `main@{1}` names the pre-c3 tip on both sides.
    both("at-one", &c, &r, &["rev-parse", "main@{1}"], None);
    let (real, _) = oracle();
    let back = sha(&real, &c, "main@{1}");
    assert_eq!(back, sha(&real, &r, "main@{1}"));
    // Restore the moved ref through update-ref, then prove the tip and its
    // log match.
    both("restore", &c, &r, &["update-ref", "refs/heads/main", &back], None);
    both("tip", &c, &r, &["rev-parse", "main"], None);
    both("log", &c, &r, &["reflog", "show", "main"], None);
    same_state("reflog-recover", &c, &r);
}

#[test]
fn update_ref_single_ops() {
    let (c, r) = pair("update-single", &|real, dir| {
        commit_fixture(real, dir, &["c1", "c2"]);
    });
    let (real, _) = oracle();
    let c1 = sha(&real, &c, "HEAD~1");
    let c2 = sha(&real, &c, "HEAD");
    assert_eq!(c1, sha(&real, &r, "HEAD~1"));
    // Create, conditional update, failed verify, delete, failed delete.
    both("create", &c, &r, &["update-ref", "refs/heads/new", &c1], None);
    both(
        "verify-update",
        &c,
        &r,
        &["update-ref", "refs/heads/new", &c2, &c1],
        None,
    );
    both(
        "verify-fail",
        &c,
        &r,
        &["update-ref", "refs/heads/new", &c1, &c1],
        None,
    );
    both("delete", &c, &r, &["update-ref", "-d", "refs/heads/new", &c2], None);
    both("delete-missing", &c, &r, &["update-ref", "-d", "refs/heads/new"], None);
    same_state("update-single", &c, &r);
}

#[test]
fn update_ref_stdin_batches() {
    let (c, r) = pair("update-stdin", &|real, dir| {
        commit_fixture(real, dir, &["c1", "c2"]);
    });
    let (real, _) = oracle();
    let c1 = sha(&real, &c, "HEAD~1");
    let c2 = sha(&real, &c, "HEAD");
    // Multi-op batch: creates plus a verified update.
    let batch = format!("create refs/heads/a {c1}\ncreate refs/heads/b {c2}\nupdate refs/heads/main {c2} {c1}\n");
    both("batch-ok", &c, &r, &["update-ref", "--stdin"], Some(batch.as_bytes()));
    both("tips", &c, &r, &["show-ref"], None);
    // Failing batch aborts all-or-nothing: b keeps its tip.
    let zeros = "0000000000000000000000000000000000000000";
    let bad = format!("update refs/heads/a {c2} {zeros}\ndelete refs/heads/b\n");
    both("batch-fail", &c, &r, &["update-ref", "--stdin"], Some(bad.as_bytes()));
    both("tips-unchanged", &c, &r, &["show-ref"], None);
    // NUL-separated batch applies identically.
    let nul = format!("create refs/heads/n {c1}\0update refs/heads/b {c1} {c2}\0");
    both("batch-nul", &c, &r, &["update-ref", "--stdin", "-z"], Some(nul.as_bytes()));
    both("tips-nul", &c, &r, &["show-ref"], None);
    same_state("update-stdin", &c, &r);
}

#[test]
fn update_ref_lock_contention() {
    let (real, is_tree) = oracle();
    assert!(real.is_file());
    let (c, r) = pair("update-lock", &|real, dir| {
        commit_fixture(real, dir, &["c1"]);
        std::fs::write(dir.join(".git/refs/heads/locked.lock"), "stale\n").unwrap();
    });
    let tip = sha(&real, &c, "HEAD");
    let ours = rust_git();
    let a = hermetic(&real, &c, &["update-ref", "refs/heads/locked", &tip]);
    let b = hermetic(&ours, &r, &["update-ref", "refs/heads/locked", &tip]);
    assert_eq!(a.status.code(), b.status.code(), "[lock] exit code");
    assert_eq!(a.status.code(), Some(128));
    assert_eq!(norm(&c, &a.stdout), norm(&r, &b.stdout), "[lock] stdout");
    if is_tree {
        assert_eq!(norm(&c, &a.stderr), norm(&r, &b.stderr), "[lock] stderr");
    } else {
        // Older system git carries a longer stale-lock advisory; both sides
        // must still name the lock file and report the collision.
        for (label, out, dir) in [("C", &a, &c), ("Rust", &b, &r)] {
            let normed = norm(dir, &out.stderr);
            let err = String::from_utf8_lossy(&normed);
            assert!(err.contains("Unable to create"), "[lock] {label} names the lock");
            assert!(err.contains("locked.lock': File exists"), "[lock] {label} reports collision");
        }
    }
}

#[test]
fn packed_refs_c_to_rust() {
    let (c, r) = pair("packed-read", &|real, dir| {
        commit_fixture(real, dir, &["c1"]);
        shell_success(real, dir, &["branch", "feat"]);
        shell_success(real, dir, &["tag", "v1"]);
        shell_success(real, dir, &["pack-refs", "--all"]);
    });
    for (desc, args) in [
        ("show-ref", vec!["show-ref"]),
        ("for-each-ref", vec!["for-each-ref"]),
        ("rev-parse-tag", vec!["rev-parse", "v1"]),
        ("rev-parse-feat", vec!["rev-parse", "feat"]),
        ("reflog-feat", vec!["reflog", "show", "feat"]),
        ("verify-main", vec!["update-ref", "refs/heads/main", "refs/heads/main"]),
    ] {
        both(desc, &c, &r, &args, None);
    }
    same_state("packed-read", &c, &r);
}

#[test]
fn packed_refs_rust_to_c() {
    let (real, _) = oracle();
    assert!(real.is_file());
    let (c, r) = pair("packed-write", &|real, dir| {
        commit_fixture(real, dir, &["c1"]);
        shell_success(real, dir, &["branch", "feat"]);
        shell_success(real, dir, &["pack-refs", "--all"]);
    });
    // The same delete through each binary must leave identical packed-refs
    // bytes, and the C binary reads the Rust-written file cleanly.
    both("delete-feat", &c, &r, &["update-ref", "-d", "refs/heads/feat"], None);
    assert_eq!(
        read_opt(&c, ".git/packed-refs"),
        read_opt(&r, ".git/packed-refs"),
        "[packed-write] packed-refs bytes"
    );
    let a = hermetic(&real, &c, &["show-ref"]);
    let b = hermetic(&real, &r, &["show-ref"]);
    assert_eq!((a.status.code(), a.stdout), (b.status.code(), b.stdout));
    same_state("packed-write", &c, &r);
}

#[test]
fn config_local_reads() {
    let (c, r) = pair("config-local", &|real, dir| {
        commit_fixture(real, dir, &["c1"]);
        shell_success(real, dir, &["config", "user.name", "Local"]);
        shell_success(real, dir, &["config", "user.email", "local@example.com"]);
    });
    for (desc, args) in [
        ("get", vec!["config", "user.name"]),
        ("get-flag", vec!["config", "--get", "user.email"]),
        ("get-missing", vec!["config", "nosuch.key"]),
        ("get-bad-key", vec!["config", "nosuch"]),
        ("list", vec!["config", "--list"]),
        ("list-origin", vec!["config", "--list", "--show-origin"]),
        ("list-scope", vec!["config", "--list", "--show-scope"]),
        ("list-nul", vec!["config", "--list", "-z"]),
        ("get-all", vec!["config", "--get-all", "user.name"]),
        ("get-regexp", vec!["config", "--get-regexp", "user\\..*"]),
        ("usage", vec!["config", "--get"]),
        ("bad-selector", vec!["config", "--global", "--system", "--list"]),
    ] {
        both(desc, &c, &r, &args, None);
    }
    same_state("config-local", &c, &r);
}

#[test]
fn config_scopes_and_special_reads() {
    let (c, r) = pair("config-scopes", &|real, dir| {
        commit_fixture(real, dir, &["c1"]);
        shell_success(real, dir, &["config", "user.name", "Local"]);
        std::fs::write(dir.join("other.cfg"), "[other]\n\tkey = fromfile\n").unwrap();
        std::fs::write(dir.join("blob.cfg"), "[blobsec]\n\tkey = fromblob\n").unwrap();
        shell_success(real, dir, &["add", "-A"]);
        shell_success(real, dir, &["commit", "-qm", "add cfg"]);
    });
    let (real, _) = oracle();
    let blob = sha(&real, &c, "HEAD:blob.cfg");
    assert_eq!(blob, sha(&real, &r, "HEAD:blob.cfg"));
    for (desc, args) in [
        ("global-missing", vec!["config", "--global", "user.name"]),
        ("file", vec!["config", "-f", "other.cfg", "--list"]),
        ("file-origin", vec!["config", "-f", "other.cfg", "--list", "--show-origin"]),
        ("file-missing", vec!["config", "-f", "nosuch.cfg", "--list"]),
        ("blob", vec!["config", "--blob", blob.as_str(), "--list"]),
    ] {
        both_cfg(desc, &c, &r, &args, None, true);
    }
    // Stdin read via `-f -`.
    both_cfg(
        "stdin",
        &c,
        &r,
        &["config", "-f", "-", "--list"],
        Some(b"[stdinsec]\n\tkey = fromstdin\n"),
        true,
    );
    same_state("config-scopes", &c, &r);
}

#[test]
fn config_mutations() {
    let (c, r) = pair("config-write", &|real, dir| {
        commit_fixture(real, dir, &["c1"]);
    });
    // Sequential writes stay in lockstep; each step is compared.
    let steps: Vec<(&str, Vec<&str>)> = vec![
        ("set", vec!["config", "sec.key", "one"]),
        ("get", vec!["config", "sec.key"]),
        ("implicit-set", vec!["config", "sec.key", "two"]),
        ("add", vec!["config", "--add", "sec.key", "three"]),
        ("get-all", vec!["config", "--get-all", "sec.key"]),
        ("replace-all", vec!["config", "--replace-all", "sec.key", "uno"]),
        ("add-back", vec!["config", "--add", "sec.key", "dos"]),
        ("unset-pattern", vec!["config", "--unset", "sec.key", "dos"]),
        ("unset-all", vec!["config", "--unset-all", "sec.key"]),
        ("unset-missing", vec!["config", "--unset", "sec.key"]),
        ("sub-set", vec!["config", "set", "sec.key", "sub"]),
        ("sub-get", vec!["config", "get", "sec.key"]),
        ("sub-unset", vec!["config", "unset", "sec.key"]),
        ("rename", vec!["config", "--rename-section", "sec", "renamed"]),
        ("list-renamed", vec!["config", "--list"]),
        ("remove", vec!["config", "--remove-section", "renamed"]),
        ("list-empty", vec!["config", "--list"]),
        ("global-set", vec!["config", "--global", "g.key", "gval"]),
        ("global-get", vec!["config", "--global", "g.key"]),
    ];
    for (desc, args) in &steps {
        both_cfg(desc, &c, &r, args, None, true);
    }
    same_state("config-write", &c, &r);
}

#[test]
fn config_includes_honored() {
    let (c, r) = pair("config-include", &|real, dir| {
        commit_fixture(real, dir, &["c1"]);
        shell_success(real, dir, &["config", "user.name", "Local"]);
        std::fs::write(dir.join(".git/extra.cfg"), "[inc]\n\tkey = frominclude\n").unwrap();
        shell_success(real, dir, &["config", "--add", "include.path", "extra.cfg"]);
        std::fs::write(dir.join(".gitconfig"), "[gsec]\n\tkey = fromglobal\n").unwrap();
    });
    for (desc, args) in [
        ("get-inc", vec!["config", "inc.key"]),
        ("get-global", vec!["config", "gsec.key"]),
        ("list", vec!["config", "--list"]),
        ("list-origin", vec!["config", "--list", "--show-origin"]),
        ("get-regexp", vec!["config", "--get-regexp", "inc\\..*"]),
        ("no-includes", vec!["config", "--no-includes", "--list"]),
    ] {
        both_cfg(desc, &c, &r, &args, None, true);
    }
    same_state("config-include", &c, &r);
}

#[test]
fn twin_inventory_sanity() {
    // The harness itself produces identical twins: same refs, same files.
    let (c, r) = pair("sanity", &|real, dir| {
        commit_fixture(real, dir, &["c1"]);
    });
    fn inventory(dir: &Path) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                let rel = p.strip_prefix(dir).unwrap().to_string_lossy().into_owned();
                if p.is_dir() && !p.is_symlink() {
                    if rel == ".git" {
                        continue;
                    }
                    out.insert(format!("{rel}/"));
                    stack.push(p);
                } else {
                    out.insert(rel);
                }
            }
        }
        out
    }
    assert_eq!(inventory(&c), inventory(&r));
    same_state("sanity", &c, &r);
}
