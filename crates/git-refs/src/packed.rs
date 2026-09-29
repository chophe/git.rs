//! Atomic packed-refs writer.
//!
//! A port of the `packed-refs` rewrite side of `refs/files-backend.c`
//! (the `pack_refs` transaction): entries sorted by refname in byte order,
//! the fixed C header line, caret-prefixed peeled continuations after
//! annotated tags, all under the `packed-refs` lock with fsync before the
//! atomic rename. Loose refs that were collapsed into the pack are unlinked
//! only after the rename commits (never before).

use std::path::Path;

use git_hash::{HashAlgorithm, Oid};

use super::lock::LockFile;
use super::RefError;

/// The fixed header C always emits (C `refs/packed-backend.c`, od-verified
/// including the trailing space).
pub const PACKED_REFS_HEADER: &str = "# pack-refs with: peeled fully-peeled sorted \n";

/// One packed-refs entry. `peeled` is `Some` for annotated tags (the object
/// the tag peels to, rendered as a `^<oid>` continuation line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedEntry {
    pub name: String,
    pub oid: Oid,
    pub peeled: Option<Oid>,
}

/// Render the full file content: header, entries sorted by refname (byte
/// order, like C's `strcmp` sort), `^`-peeled continuations after tags.
pub fn render(entries: &[PackedEntry]) -> String {
    let mut sorted: Vec<&PackedEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
    let mut out = String::from(PACKED_REFS_HEADER);
    for e in sorted {
        out.push_str(&format!("{} {}\n", e.oid, e.name));
        if let Some(p) = &e.peeled {
            out.push_str(&format!("^{p}\n"));
        }
    }
    out
}

/// Atomically rewrite `packed-refs`: hold `packed-refs.lock` across the
/// whole rewrite, fsync before rename. A concurrent writer gets
/// [`RefError::LockContention`] naming the lock file.
pub fn write_packed_refs(common_dir: &Path, entries: &[PackedEntry]) -> Result<(), RefError> {
    let path = common_dir.join("packed-refs");
    let mut lock = LockFile::acquire(&path)?;
    lock.write_and_fsync(render(entries).as_bytes())?;
    lock.commit()
}

/// Read the current packed entries (with `^`-peeled continuations
/// attached to their tags); missing file reads as empty.
pub fn read_entries(common_dir: &Path, algo: HashAlgorithm) -> Vec<PackedEntry> {
    let content = std::fs::read_to_string(common_dir.join("packed-refs")).unwrap_or_default();
    let mut out: Vec<PackedEntry> = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(peeled) = line.strip_prefix('^') {
            if let (Some(last), Ok(oid)) = (out.last_mut(), Oid::from_hex(peeled, algo)) {
                last.peeled = Some(oid);
            }
            continue;
        }
        let mut it = line.splitn(2, ' ');
        if let (Some(oid_s), Some(name)) = (it.next(), it.next()) {
            if let Ok(oid) = Oid::from_hex(oid_s, algo) {
                out.push(PackedEntry { name: name.to_string(), oid, peeled: None });
            }
        }
    }
    out
}

/// Remove `names` from packed-refs, rewriting atomically (a no-op when
/// the file is missing or holds none of them). Used by ref deletes: C
/// drops packed entries on delete.
pub fn prune_entries(common_dir: &Path, algo: HashAlgorithm, names: &[&str]) -> Result<(), RefError> {
    let path = common_dir.join("packed-refs");
    if !path.is_file() {
        return Ok(());
    }
    let entries: Vec<PackedEntry> = read_entries(common_dir, algo)
        .into_iter()
        .filter(|e| !names.contains(&e.name.as_str()))
        .collect();
    write_packed_refs(common_dir, &entries)
}

/// Unlink loose refs that were collapsed into a committed pack. Must run
/// only after [`write_packed_refs`] commits; missing files are fine (a ref
/// may already be packed-only).
pub fn collapse_loose(common_dir: &Path, names: &[&str]) -> Result<(), RefError> {
    for name in names {
        let _ = std::fs::remove_file(common_dir.join(name));
    }
    // Prune directories left empty by the collapse (best-effort; a
    // non-empty dir simply stays).
    let mut dirs: Vec<std::path::PathBuf> = names
        .iter()
        .filter_map(|n| common_dir.join(n).parent().map(Path::to_path_buf))
        .collect();
    dirs.sort();
    dirs.dedup();
    for d in dirs {
        let _ = std::fs::remove_dir(d);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_hash::HashAlgorithm;

    fn entry(name: &str, peeled: bool) -> PackedEntry {
        let algo = HashAlgorithm::Sha1;
        PackedEntry {
            name: name.to_string(),
            oid: *algo.empty_blob(),
            peeled: peeled.then(|| *algo.null_oid()),
        }
    }

    #[test]
    fn render_matches_c_byte_contract() {
        let entries = vec![
            entry("refs/tags/light", false),
            entry("refs/heads/zeta", false),
            entry("refs/tags/annotated", true),
            entry("refs/heads/master", false),
        ];
        let text = render(&entries);
        let mut lines = text.lines();
        // First line is the C header (trailing space included).
        assert_eq!(lines.next().unwrap(), "# pack-refs with: peeled fully-peeled sorted ");
        let rest: Vec<&str> = lines.collect();
        let names: Vec<&str> = rest
            .iter()
            .filter(|l| !l.starts_with('^'))
            .map(|l| l.split_once(' ').unwrap().1)
            .collect();
        assert_eq!(
            names,
            vec!["refs/heads/master", "refs/heads/zeta", "refs/tags/annotated", "refs/tags/light"]
        );
        // The peeled continuation follows its tag.
        let tag_idx = rest.iter().position(|l| l.ends_with("refs/tags/annotated")).unwrap();
        assert!(rest[tag_idx + 1].starts_with('^'));
    }

    #[test]
    fn write_then_collapse_round_trip() {
        let dir = std::env::temp_dir().join(format!("git-packed-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let git = dir.join(".git");
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        let algo = HashAlgorithm::Sha1;
        std::fs::write(git.join("refs/heads/a"), format!("{}\n", algo.empty_blob())).unwrap();
        write_packed_refs(&git, &[entry("refs/heads/a", false)]).unwrap();
        assert!(git.join("packed-refs").exists());
        assert!(!crate::lock::lock_path_for(&git.join("packed-refs")).exists());
        collapse_loose(&git, &["refs/heads/a"]).unwrap();
        assert!(!git.join("refs/heads/a").exists());
        let content = std::fs::read_to_string(git.join("packed-refs")).unwrap();
        assert!(content.starts_with(PACKED_REFS_HEADER));
        assert!(content.contains("refs/heads/a"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn concurrent_pack_writer_gets_contention() {
        let dir = std::env::temp_dir().join(format!("git-packed-test-{}.lock", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let git = dir.join(".git");
        std::fs::create_dir_all(&git).unwrap();
        std::fs::write(git.join("packed-refs.lock"), b"").unwrap();
        let err = write_packed_refs(&git, &[]).unwrap_err();
        assert!(err.to_string().contains("packed-refs.lock"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
