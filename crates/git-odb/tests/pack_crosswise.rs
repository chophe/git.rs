//! Crosswise compatibility tests against the system C git.
//!
//! These verify that packs written by real git are read correctly by this
//! port, and that packs written by this port pass C git's own verification.
//! They skip (pass trivially) when no system `git` binary is available.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use git_hash::HashAlgorithm;
use git_object::{Object, ObjectKind};
use git_odb::pack::{write_pack, PackFile, PackObject};
use git_odb::{LooseStore, Odb};

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
    let dir = std::env::temp_dir().join(format!("git-odb-xwise-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn run(git: &str, dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(git)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs")
}

fn cmd_ok(git: &str, dir: &Path, args: &[&str]) -> bool {
    run(git, dir, args).status.success()
}

/// Build a small repository with a few commits and repack it, returning the
/// pack file bytes and every object id reachable from HEAD.
fn build_real_repo() -> (Vec<u8>, Vec<String>) {
    let git = git().unwrap();
    let dir = tempdir();
    run(git, &dir, &["init", "-q"]);
    run(git, &dir, &["config", "user.name", "Tester"]);
    run(git, &dir, &["config", "user.email", "t@e.com"]);

    std::fs::write(dir.join("a.txt"), "alpha\n").unwrap();
    std::fs::write(dir.join("b.txt"), "beta\n").unwrap();
    run(git, &dir, &["add", "."]);
    assert!(cmd_ok(git, &dir, &["commit", "-qm", "one"]));
    std::fs::write(dir.join("a.txt"), "alpha2\n").unwrap();
    assert!(cmd_ok(git, &dir, &["commit", "-qam", "two"]));
    std::fs::write(dir.join("c.txt"), "gamma2\n").unwrap();
    assert!(cmd_ok(git, &dir, &["add", "-A"]));
    assert!(cmd_ok(git, &dir, &["commit", "-qm", "three"]));
    assert!(cmd_ok(git, &dir, &["repack", "-ad"]));

    let idx_path = std::fs::read_dir(dir.join(".git/objects/pack"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|x| x.to_str()) == Some("idx"))
        .unwrap();
    let pack_path = idx_path.with_extension("pack");
    let pack = std::fs::read(&pack_path).unwrap();

    let out = run(git, &dir, &["rev-list", "--objects", "--all"]);
    let stdout = String::from_utf8(out.stdout).unwrap();
    let oids: Vec<String> = stdout
        .lines()
        .map(|l| l.split_whitespace().next().unwrap().to_string())
        .collect();

    let _ = std::fs::remove_dir_all(&dir);
    (pack, oids)
}

#[test]
fn reads_and_verifies_real_git_pack() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let (pack, oids) = build_real_repo();
    assert!(!oids.is_empty());

    // We need the idx too; rebuild the repo path mapping by parsing the pack
    // directly and verifying the trailer, then check each object.
    let algo = HashAlgorithm::Sha1;
    let pf = PackFile::from_bytes(pack, algo).unwrap();

    // The pack from `git repack -ad` is complete; verify the trailer.
    pf.verify_trailer().unwrap();

    // Walk all entries and ensure each resolves and hashes back to a unique id.
    let mut resolver = |_: &git_hash::Oid| -> Option<Object> { None };
    let end = pf.data_end();
    let mut pos = pf.first_entry_offset();
    let mut found = 0usize;
    while pos < end {
        let resolved = pf.resolve_entry(pos, None, &mut resolver).unwrap();
        let oid = resolved.object.compute_id(algo).to_string();
        assert!(oids.contains(&oid), "object {oid} should be reachable");
        found += 1;
        pos += resolved.entry_len;
    }
    assert_eq!(found, oids.len(), "pack should contain exactly the reachable objects");
    let _ = git;
}

#[test]
fn real_git_reads_our_pack() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let dir = tempdir();
    let algo = HashAlgorithm::Sha1;

    let objects = [
        Object::from_data(ObjectKind::Blob, b"crosswise blob".to_vec()),
        Object::from_data(ObjectKind::Tree, b"100644 f\0abc".as_slice().to_vec()),
        Object::from_data(ObjectKind::Commit, b"tree 0000\nauthor A <a@b> 0 +0000\n\nm\n".to_vec()),
        Object::from_data(ObjectKind::Blob, b"another blob".to_vec()),
    ];
    let pos: Vec<PackObject> = objects
        .iter()
        .map(|o| PackObject {
            oid: o.compute_id(algo),
            kind: o.kind,
            data: o.data.clone(),
        })
        .collect();
    let (pack, idx) = write_pack(&pos, algo).unwrap();

    let pack_path = dir.join("t.pack");
    let idx_path = dir.join("t.idx");
    std::fs::write(&pack_path, &pack).unwrap();
    std::fs::write(&idx_path, &idx).unwrap();

    assert!(cmd_ok(git, &dir, &["index-pack", "--verify", "t.pack"]), "git index-pack --verify");
    assert!(cmd_ok(git, &dir, &["verify-pack", "t.idx"]), "git verify-pack");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unpack_real_git_pack_into_loose_store() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let (pack, oids) = build_real_repo();
    let algo = HashAlgorithm::Sha1;

    // Unpack into a fresh repo's loose store.
    let dir = tempdir();
    let gitdir = dir.join(".git");
    std::fs::create_dir_all(&gitdir).unwrap();
    std::fs::write(gitdir.join("HEAD"), "ref: refs/heads/master\n").unwrap();
    std::fs::create_dir_all(gitdir.join("refs")).unwrap();
    let store = LooseStore::new(gitdir.join("objects"), algo);
    let pf = PackFile::from_bytes(pack, algo).unwrap();
    let end = pf.data_end();
    let mut pos = pf.first_entry_offset();
    while pos < end {
        let mut resolver = |boid: &git_hash::Oid| store.read(boid).ok();
        let resolved = pf.resolve_entry(pos, None, &mut resolver).unwrap();
        store.write(&resolved.object).unwrap();
        pos += resolved.entry_len;
    }
    assert_eq!(store.object_count(), oids.len());
    assert_eq!(pf.object_count() as usize, oids.len());

    // Let real git fsck the unpacked objects.
    let out = run(git, &dir, &["fsck"]);
    let text = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "git fsck failed: {text}");
    assert!(!text.contains("missing"), "fsck reported missing objects: {text}");
    assert!(!text.contains("corrupt"), "fsck reported corruption: {text}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn odb_reads_from_real_pack() {
    let Some(git) = git() else {
        eprintln!("skipping: no system git");
        return;
    };
    let (pack, oids) = build_real_repo();
    let algo = HashAlgorithm::Sha1;

    // Lay the pack out as a real repo (under .git) and read through Odb by id.
    let dir = tempdir();
    let gitdir = dir.join(".git");
    std::fs::create_dir_all(gitdir.join("objects/pack")).unwrap();
    std::fs::create_dir_all(gitdir.join("refs")).unwrap();
    std::fs::write(gitdir.join("HEAD"), "ref: refs/heads/master\n").unwrap();
    let idx = pack_index_for(&pack, algo);
    std::fs::write(gitdir.join("objects/pack/t.pack"), &pack).unwrap();
    std::fs::write(gitdir.join("objects/pack/t.idx"), &idx).unwrap();

    let repo = git_core::Repository {
        git_dir: gitdir.clone(),
        common_dir: gitdir,
        work_tree: None,
        bare: false,
        hash_algo: algo,
        config: git_config::ConfigSet::new(),
    };
    let odb = Odb::from_repo(&repo).unwrap();
    for oid_s in &oids {
        let oid = git_hash::Oid::from_hex(oid_s, algo).unwrap();
        let obj = odb.read(&oid).expect("odb reads reachable object");
        // Compare content with git cat-file.
        let out = run(git, &dir, &["cat-file", obj.kind.as_str(), oid_s]);
        assert!(out.status.success(), "git cat-file failed for {oid_s}");
        assert_eq!(out.stdout, obj.data, "content mismatch for {oid_s}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// Build a v2 index for a pack by walking its entries (used to give `Odb`
/// something to find oids in).
fn pack_index_for(pack: &[u8], algo: HashAlgorithm) -> Vec<u8> {
    let pf = PackFile::from_bytes(pack.to_vec(), algo).unwrap();
    let mut resolver = |_: &git_hash::Oid| -> Option<Object> { None };
    let end = pf.data_end();
    let mut pos = pf.first_entry_offset();
    let mut entries = Vec::new();
    while pos < end {
        let resolved = pf.resolve_entry(pos, None, &mut resolver).unwrap();
        let oid = resolved.object.compute_id(algo);
        let crc = git_odb::pack::crc32::crc32(&pack[pos..pos + resolved.entry_len]);
        entries.push((oid, pos as u64, crc));
        pos += resolved.entry_len;
    }
    entries.sort_by_key(|e| e.0);
    let trailer = &pack[pack.len() - algo.raw_len()..];
    git_odb::pack::write_idx(&entries, trailer, algo)
}
