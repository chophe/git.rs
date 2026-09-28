//! Wave-0 integration scaffold for the refs foundation (plan 01-01).
//!
//! Covers the tracer slice end to end at the storage layer: lock
//! contention on one ref, transaction abort leaving the set unchanged,
//! reflog append plus gating for HEAD versus branch under
//! `logallrefupdates` true/false/always, and packed-refs write/read
//! round-trip including caret peeled lines (validated by C git).
//!
//! Gate-script mapping (research O2, recorded for the gate plan in 01-04):
//! the `t/t3210-pack-refs.sh` name from 01-CONTEXT.md does not exist in
//! this tree; the real packed-refs oracles are
//! `t/t0601-reffiles-pack-refs.sh` + `t/pack-refs-tests.sh`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use git_core::{RepoEnv, Repository};
use git_hash::{HashAlgorithm, Oid};
use git_refs::lock::LockFile;
use git_refs::transaction::{Transaction, TxnOp};
use git_refs::{packed, reflog, RefStore};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn system_git() -> Option<&'static str> {
    for cand in ["/usr/bin/git", "/usr/local/bin/git", "/opt/homebrew/bin/git"] {
        if Path::new(cand).exists() {
            return Some(cand);
        }
    }
    None
}

/// Synthetic repo (inline-test helper pattern from `git-refs/src/lib.rs`):
/// `.git` with HEAD, empty config, no objects.
fn synthetic_repo(tag: &str) -> (Repository, PathBuf) {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("git-ref-tx-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let git = dir.join(".git");
    std::fs::create_dir_all(git.join("refs/heads")).unwrap();
    std::fs::create_dir_all(git.join("refs/tags")).unwrap();
    std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    let repo = Repository::discover_from(&dir, &RepoEnv::default()).unwrap();
    (repo, dir)
}

/// Real repo with one commit via system C git (needed whenever C must
/// validate objects, e.g. `show-ref` over a Rust-written pack).
fn real_repo(tag: &str) -> Option<(PathBuf, Oid)> {
    let git = system_git()?;
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("git-ref-tx-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| Command::new(git).args(args).current_dir(&dir).output().unwrap();
    assert!(run(&["init", "-q"]).status.success());
    assert!(run(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "probe"])
        .status
        .success());
    let out = run(&["rev-parse", "HEAD"]);
    let oid = Oid::from_hex(String::from_utf8(out.stdout).unwrap().trim(), HashAlgorithm::Sha1).unwrap();
    Some((dir, oid))
}

fn file_bytes(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.join(".git/refs"), dir.join(".git/packed-refs")];
    // packed-refs itself is a file; refs is a dir.
    let mut files: Vec<PathBuf> = Vec::new();
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                for e in rd.flatten() {
                    stack.push(e.path());
                }
            }
        } else if p.is_file() {
            files.push(p);
        }
    }
    for p in files {
        if let Ok(data) = std::fs::read(&p) {
            out.push((p.strip_prefix(dir).unwrap().to_string_lossy().into_owned(), data));
        }
    }
    out.sort();
    out
}

#[test]
fn lock_contention_on_one_ref() {
    let (_repo, dir) = synthetic_repo("lock");
    let target = dir.join(".git/refs/heads/main");
    let _held = LockFile::acquire(&target).unwrap();
    let err = match LockFile::acquire(&target) {
        Ok(_) => panic!("second holder must fail"),
        Err(e) => e,
    };
    let text = err.to_string();
    assert!(text.contains("refs/heads/main.lock"), "names the lock file: {text}");
    // The held lock is intact; nothing was published.
    assert!(!target.exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn transaction_abort_leaves_set_unchanged() {
    let (repo, dir) = synthetic_repo("abort");
    let store = RefStore::from_repo(&repo);
    let algo = HashAlgorithm::Sha1;
    let oid = *algo.empty_blob();
    let other = Oid::from_hex("2222222222222222222222222222222222222222", algo).unwrap();
    let wrong = Oid::from_hex("1111111111111111111111111111111111111111", algo).unwrap();
    store.update("refs/heads/keep", Some(&oid)).unwrap();
    let before = file_bytes(&dir);

    let mut tx = Transaction::begin(&store);
    tx.queue(TxnOp::Set { name: "refs/heads/keep".to_string(), new: other, old: None, deref: false });
    tx.queue(TxnOp::Set { name: "refs/heads/new".to_string(), new: other, old: None, deref: false });
    tx.queue(TxnOp::Set { name: "refs/heads/keep".to_string(), new: other, old: None, deref: false });
    // Duplicate op must abort the batch before any rename...
    assert!(tx.prepare().is_err());
    drop(tx);

    let mut tx = Transaction::begin(&store);
    tx.queue(TxnOp::Set { name: "refs/heads/keep".to_string(), new: other, old: None, deref: false });
    tx.queue(TxnOp::Set { name: "refs/heads/keep2".to_string(), new: other, old: Some(wrong), deref: false });
    // ...as must a bad old-oid on a missing ref.
    let err = tx.prepare().unwrap_err();
    assert!(err.to_string().contains("unable to resolve reference"), "{err}");
    drop(tx);

    assert_eq!(file_bytes(&dir), before, "aborted batches changed the ref set");
    assert!(store.resolve("refs/heads/new").is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn reflog_append_and_gating_head_vs_branch() {
    let (mut repo, dir) = synthetic_repo("gating");
    let algo = HashAlgorithm::Sha1;
    let old = *algo.null_oid();
    let new = *algo.empty_blob();
    let ident = "T Est <t@example.com> 1752327337 +0000";
    let git = dir.join(".git");

    // Default (unset, non-bare): HEAD and branch log, tags do not.
    assert!(reflog::should_log_repo(&repo, "HEAD"));
    assert!(reflog::should_log_repo(&repo, "refs/heads/main"));
    assert!(!reflog::should_log_repo(&repo, "refs/tags/v1"));

    // Explicit false: nothing logs, not even HEAD.
    repo.config.set("core", "logallrefupdates", "false");
    assert!(!reflog::should_log_repo(&repo, "HEAD"));
    assert!(!reflog::should_log_repo(&repo, "refs/heads/main"));

    // Explicit always: everything logs, including tags.
    repo.config.set("core", "logallrefupdates", "always");
    assert!(reflog::should_log_repo(&repo, "HEAD"));
    assert!(reflog::should_log_repo(&repo, "refs/tags/v1"));

    // Append + read round-trip in file order.
    repo.config.set("core", "logallrefupdates", "true");
    reflog::append(&git, "HEAD", &old, &new, ident, "commit (initial): probe").unwrap();
    reflog::append(&git, "HEAD", &new, &new, ident, "").unwrap();
    let entries = reflog::read_all(&git, "HEAD", algo);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].message, "commit (initial): probe");
    assert_eq!(entries[0].ident, ident);
    assert_eq!(entries[1].message, "");
    // Raw bytes honor the no-tab-on-empty-message contract.
    let raw = std::fs::read_to_string(git.join("logs/HEAD")).unwrap();
    assert_eq!(raw.lines().count(), 2);
    assert!(!raw.lines().nth(1).unwrap().contains('\t'));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn packed_write_read_round_trip_with_peeled() {
    let Some((dir, oid)) = real_repo("packed") else {
        eprintln!("skipping: no system git");
        return;
    };
    let git = dir.join(".git");
    let repo = Repository::discover_from(&dir, &RepoEnv::default()).unwrap();
    let store = RefStore::from_repo(&repo);

    let entries = vec![
        packed::PackedEntry { name: "refs/heads/zeta".to_string(), oid, peeled: None },
        packed::PackedEntry { name: "refs/heads/master".to_string(), oid, peeled: None },
        packed::PackedEntry { name: "refs/tags/annotated".to_string(), oid, peeled: Some(oid) },
        packed::PackedEntry { name: "refs/tags/light".to_string(), oid, peeled: None },
    ];
    packed::write_packed_refs(&git, &entries).unwrap();
    packed::collapse_loose(&git, &["refs/heads/master"]).unwrap();

    // Raw file: C header first, sorted entries, caret peeled continuation.
    let raw = std::fs::read_to_string(git.join("packed-refs")).unwrap();
    let mut lines = raw.lines();
    assert_eq!(lines.next().unwrap(), "# pack-refs with: peeled fully-peeled sorted ");
    let body: Vec<&str> = lines.collect();
    let names: Vec<&str> = body
        .iter()
        .filter(|l| !l.starts_with('^'))
        .map(|l| l.split_once(' ').unwrap().1)
        .collect();
    assert_eq!(names, vec![
        "refs/heads/master",
        "refs/heads/zeta",
        "refs/tags/annotated",
        "refs/tags/light"
    ]);
    let tag_idx = body.iter().position(|l| l.ends_with("refs/tags/annotated")).unwrap();
    assert!(body[tag_idx + 1].starts_with('^'));

    // Rust reads the pack; C git reads it with zero errors.
    assert_eq!(store.resolve("refs/tags/annotated"), Some(oid));
    assert!(store.list().iter().any(|(n, _)| n == "refs/heads/zeta"));
    let out = Command::new(system_git().unwrap())
        .args(["show-ref"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "C show-ref failed: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    for name in ["refs/heads/master", "refs/heads/zeta", "refs/tags/annotated", "refs/tags/light"] {
        assert!(text.contains(name), "{text}");
    }
    std::fs::remove_dir_all(&dir).ok();
}
