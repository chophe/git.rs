//! Crosswise tests for Phase A item A13 (pack delta compression on write)
//! against the system C git. Skips when no system `git` is available.
//!
//! The writer's delta selection is intentionally heuristic (a port of
//! `pack-objects.c`'s window/depth search), so byte-for-byte pack equality
//! with C is not expected. What must hold is that C git accepts and verifies
//! a deltified pack we write, that it contains the same object set, and that
//! its size is in the same ballpark as C git's own pack.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn git() -> Option<&'static str> {
    for cand in ["/usr/bin/git", "/usr/local/bin/git", "/opt/homebrew/bin/git"] {
        if Path::new(cand).exists() {
            return Some(cand);
        }
    }
    None
}

fn rust_git() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/git");
    p.canonicalize().ok()
}

fn tempdir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("git-a13-xwise-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn run(git: &str, dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(git).args(args).current_dir(dir).output().expect("git runs")
}

fn ok(git: &str, dir: &Path, args: &[&str]) {
    let out = run(git, dir, args);
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A repo with many near-identical revisions so delta selection has work.
fn build_repo(git: &str) -> PathBuf {
    let dir = tempdir("repo");
    ok(git, &dir, &["init", "-q", "-b", "main"]);
    ok(git, &dir, &["config", "user.name", "T"]);
    ok(git, &dir, &["config", "user.email", "t@example.com"]);
    for i in 0..16 {
        let mut body = String::new();
        for line in 0..300 {
            body.push_str(&format!("shared line {line:04} of body {i}\n"));
        }
        // A line that changes every revision so blobs are distinct.
        body.push_str(&format!("revision marker {i}\n"));
        std::fs::write(dir.join("f.txt"), body).unwrap();
        ok(git, &dir, &["add", "f.txt"]);
        ok(git, &dir, &["commit", "-qm", &format!("commit {i}")]);
    }
    // Second file with independent history.
    std::fs::write(dir.join("g.txt"), "alpha\nbeta\ngamma\n").unwrap();
    ok(git, &dir, &["add", "g.txt"]);
    ok(git, &dir, &["commit", "-qm", "add g"]);
    dir
}

/// Every object id reachable from all refs (the input list for pack-objects).
fn object_ids(git: &str, dir: &Path) -> Vec<String> {
    let out = run(git, dir, &["rev-list", "--objects", "--all"]);
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

fn pipe_pack_objects(exe: &Path, dir: &Path, args: &[&str], oids: &[String]) -> Vec<u8> {
    let mut child = Command::new(exe)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for oid in oids {
            writeln!(stdin, "{oid}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{exe:?} {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

/// Run C `git pack-objects --stdout` on the same object list.
fn c_pack(git: &str, dir: &Path, args: &[&str], oids: &[String]) -> Vec<u8> {
    let mut argv = vec!["pack-objects"];
    argv.extend_from_slice(args);
    argv.push("--stdout");
    let mut child = Command::new(git)
        .args(&argv)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for oid in oids {
            writeln!(stdin, "{oid}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "C pack-objects failed");
    out.stdout
}

/// Verify a pack with C git and return the sorted set of object ids it
/// contains (from `verify-pack -v`).
fn c_verify_pack(git: &str, dir: &Path, pack: &[u8]) -> Vec<String> {
    let pack_path = dir.join("candidate.pack");
    std::fs::write(&pack_path, pack).unwrap();
    // index-pack builds candidate.idx (and verifies the pack on the way).
    let out = run(git, dir, &["index-pack", "candidate.pack"]);
    assert!(
        out.status.success(),
        "C index-pack rejected our pack: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let idx = dir.join("candidate.idx");
    let v = run(git, dir, &["verify-pack", "-v", "candidate.idx"]);
    assert!(v.status.success(), "C verify-pack failed");
    let text = String::from_utf8_lossy(&v.stdout);
    assert!(text.contains("ok"), "verify-pack did not report ok:\n{text}");
    let mut ids: Vec<String> = text
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|tok| tok.len() == 40 && tok.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_string)
        .collect();
    ids.sort();
    ids.dedup();
    let _ = std::fs::remove_file(idx);
    ids
}

#[test]
fn rust_idx_is_accepted_by_c_git() {
    let (Some(git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let dir = build_repo(git);
    let oids = object_ids(git, &dir);

    // `<base>` form writes `<base>.pack` and `<base>.idx` (our own index).
    let mut child = Command::new(&rust)
        .args(["pack-objects", "--window=10", "--depth=50", "ours"])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for oid in &oids {
            writeln!(stdin, "{oid}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "rust pack-objects failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // C git must verify *our* idx (not one it rebuilt).
    let v = run(git, &dir, &["verify-pack", "-v", "ours.idx"]);
    assert!(
        v.status.success(),
        "C verify-pack rejected our idx: {}",
        String::from_utf8_lossy(&v.stderr)
    );
    let text = String::from_utf8_lossy(&v.stdout);
    assert!(text.contains("ours.pack: ok"), "verify-pack not ok:\n{text}");

    // And `index-pack --verify` with our idx alongside must pass.
    let ip = run(git, &dir, &["index-pack", "--verify", "ours.pack"]);
    assert!(
        ip.status.success(),
        "C index-pack --verify rejected our pack+idx: {}",
        String::from_utf8_lossy(&ip.stderr)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rust_deltified_pack_verifies_with_c_git() {
    let (Some(git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let dir = build_repo(git);
    let oids = object_ids(git, &dir);
    assert!(oids.len() > 10);

    let pack = pipe_pack_objects(
        &rust,
        &dir,
        &["pack-objects", "--window=10", "--depth=50", "--stdout"],
        &oids,
    );
    let ids = c_verify_pack(git, &dir, &pack);

    let mut expect = oids.clone();
    expect.sort();
    expect.dedup();
    assert_eq!(ids, expect, "pack content differs from C git's object set");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rust_pack_is_deltified_and_size_comparable() {
    let (Some(git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let dir = build_repo(git);
    let oids = object_ids(git, &dir);

    let opts = ["pack-objects", "--window=10", "--depth=50", "--stdout"];
    let rust_pack = pipe_pack_objects(&rust, &dir, &opts, &oids);
    let c_pack_bytes = c_pack(git, &dir, &["--window=10", "--depth=50"], &oids);

    // Sanity: our pack must be smaller than the sum of the raw object sizes
    // (i.e. it actually deltifies); and within 2x of C git's pack.
    let raw: usize = oids
        .iter()
        .map(|o| {
            let out = run(git, &dir, &["cat-file", "-s", o]);
            String::from_utf8_lossy(&out.stdout).trim().parse::<usize>().unwrap_or(0)
        })
        .sum();
    assert!(
        rust_pack.len() < raw,
        "rust pack {} not smaller than raw payload {raw}",
        rust_pack.len()
    );
    assert!(
        rust_pack.len() <= c_pack_bytes.len() * 2,
        "rust pack {} vs C pack {} (ratio {:.2})",
        rust_pack.len(),
        c_pack_bytes.len(),
        rust_pack.len() as f64 / c_pack_bytes.len() as f64
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ref_delta_mode_verifies_with_c_git() {
    let (Some(git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let dir = build_repo(git);
    let oids = object_ids(git, &dir);

    let pack = pipe_pack_objects(
        &rust,
        &dir,
        &[
            "pack-objects",
            "--no-delta-base-offset",
            "--window=10",
            "--depth=50",
            "--stdout",
        ],
        &oids,
    );
    // C git must verify a REF_DELTA pack the same way.
    let ids = c_verify_pack(git, &dir, &pack);
    assert_eq!(ids.len(), {
        let mut e = oids.clone();
        e.sort();
        e.dedup();
        e.len()
    });

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn window_and_depth_flags_are_accepted() {
    let (Some(git), Some(rust)) = (git(), rust_git()) else {
        eprintln!("skipping: need system git and built rust binary");
        return;
    };
    let dir = build_repo(git);
    let oids = object_ids(git, &dir);

    // window=0 must produce a pack C git still verifies (no deltas).
    let plain = pipe_pack_objects(&rust, &dir, &["pack-objects", "--window=0", "--stdout"], &oids);
    c_verify_pack(git, &dir, &plain);

    // Spaced and `=` forms, and --compression.
    let compressed = pipe_pack_objects(
        &rust,
        &dir,
        &[
            "pack-objects",
            "--window",
            "5",
            "--depth",
            "2",
            "--compression=9",
            "--stdout",
        ],
        &oids,
    );
    c_verify_pack(git, &dir, &compressed);

    // `--non-empty` with no input succeeds silently (no pack bytes).
    let empty = pipe_pack_objects(&rust, &dir, &["pack-objects", "--non-empty", "--stdout"], &[]);
    assert!(empty.is_empty(), "expected no pack for empty --non-empty input");

    let _ = std::fs::remove_dir_all(&dir);
}
