//! Crosswise compatibility tests for the commit-graph and multi-pack-index
//! against the system C git. Skips when no system `git` is available.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use git_commitgraph::CommitGraph;
use git_hash::HashAlgorithm;
use git_odb::pack::midx::{write_from_indexes, Midx};
use git_odb::pack::PackIndex;

static COUNTER: AtomicU32 = AtomicU32::new(0);

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
    let dir = std::env::temp_dir().join(format!("git-graph-xwise-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn run(git: &str, dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(git).args(args).current_dir(dir).output().expect("git runs")
}

fn cmd_ok(git: &str, dir: &Path, args: &[&str]) -> bool {
    run(git, dir, args).status.success()
}

/// Build a repo with several commits and repack it.
fn build_repo() -> PathBuf {
    let git = git().unwrap();
    let dir = tempdir();
    run(git, &dir, &["init", "-q"]);
    run(git, &dir, &["config", "user.name", "T"]);
    run(git, &dir, &["config", "user.email", "t@e.com"]);
    for i in 0..5 {
        std::fs::write(dir.join("f"), format!("line {i}\n")).unwrap();
        assert!(cmd_ok(git, &dir, &["add", "-A"]));
        assert!(cmd_ok(git, &dir, &["commit", "-qm", &format!("c{i}")]));
    }
    assert!(cmd_ok(git, &dir, &["repack", "-ad"]));
    dir
}

#[test]
fn verifies_real_git_commit_graph() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    assert!(cmd_ok(git, &dir, &["commit-graph", "write", "--reachable"]));
    let path = dir.join(".git/objects/info/commit-graph");
    let data = std::fs::read(&path).expect("commit-graph exists");
    let graph = CommitGraph::parse(data, HashAlgorithm::Sha1).unwrap();
    assert!(graph.num_commits() >= 5);
    assert!(graph.verify().is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verifies_real_git_midx() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    assert!(cmd_ok(git, &dir, &["multi-pack-index", "write"]));
    let path = dir.join(".git/objects/pack/multi-pack-index");
    let data = std::fs::read(&path).expect("midx exists");
    let midx = Midx::parse(data, HashAlgorithm::Sha1).unwrap();
    assert!(midx.num_packs() >= 1);
    assert!(midx.verify().is_ok());
    // Every object in the pack must be findable through the midx.
    let pack_dir = dir.join(".git/objects/pack");
    for e in std::fs::read_dir(&pack_dir).unwrap().flatten() {
        if e.path().extension().and_then(|x| x.to_str()) == Some("idx") {
            let idx_data = std::fs::read(e.path()).unwrap();
            let idx = PackIndex::parse(&idx_data, HashAlgorithm::Sha1).unwrap();
            for oid in idx.oids().iter().take(5) {
                assert!(midx.find(oid).is_some(), "midx should find {oid}");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn real_git_verifies_our_midx() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = build_repo();
    let pack_dir = dir.join(".git/objects/pack");
    let mut indexes: Vec<(String, PackIndex)> = Vec::new();
    for e in std::fs::read_dir(&pack_dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) == Some("idx") {
            let data = std::fs::read(&p).unwrap();
            let idx = PackIndex::parse(&data, HashAlgorithm::Sha1).unwrap();
            let base = p.file_stem().unwrap().to_str().unwrap().to_string();
            indexes.push((base, idx));
        }
    }
    assert!(!indexes.is_empty());
    let midx = write_from_indexes(&indexes, HashAlgorithm::Sha1).unwrap();
    std::fs::write(pack_dir.join("multi-pack-index"), &midx).unwrap();
    assert!(
        cmd_ok(git, &dir, &["multi-pack-index", "verify"]),
        "real git should verify our midx"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
