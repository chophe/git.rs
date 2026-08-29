//! Revision name resolution: a subset of C git's `object-name.c`
//! (`get_oid`) covering full oids, ref names, abbreviated oids with
//! ambiguity detection, and the `~<n>` / `^<n>` peel operators.

use std::fmt;

use git_core::Repository;
use git_hash::Oid;
use git_object::{parse_commit, parse_tree, ObjectKind};
use git_odb::Odb;
use git_refs::RefStore;

/// Errors carrying C git's exact stderr text (always exit 128, from `die`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// A short object id matched multiple objects.
    Ambiguous { arg: String, prefix: String },
    /// The argument could not be resolved at all.
    Unknown { arg: String },
}

impl ResolveError {
    /// The full stderr text C git would print for this error.
    pub fn render(&self) -> String {
        let generic = |arg: &str| {
            format!(
                "fatal: ambiguous argument '{arg}': unknown revision or path not in the working tree.\nUse '--' to separate paths from revisions, like this:\n'git <command> [<revision>...] -- [<file>...]'"
            )
        };
        match self {
            ResolveError::Ambiguous { arg, prefix } => format!(
                "error: short object ID {prefix} is ambiguous\n{}",
                generic(arg)
            ),
            ResolveError::Unknown { arg } => generic(arg),
        }
    }
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render())
    }
}

impl std::error::Error for ResolveError {}

/// A resolver over a repository's object database and refs.
pub struct Resolver {
    odb: Odb,
    store: RefStore,
    hex_len: usize,
}

impl Resolver {
    pub fn new(repo: &Repository) -> Result<Resolver, git_odb::PackError> {
        Ok(Resolver {
            odb: Odb::from_repo(repo)?,
            store: RefStore::from_repo(repo),
            hex_len: repo.hash_algo.hex_len(),
        })
    }

    /// Resolve an argument to an object id (C git's `get_oid`).
    pub fn resolve(&self, arg: &str) -> Result<Oid, ResolveError> {
        self.resolve_inner(arg)
    }

    fn resolve_inner(&self, arg: &str) -> Result<Oid, ResolveError> {
        let unknown = || ResolveError::Unknown { arg: arg.to_string() };

        // `<rev>:<path>`: object at `path` inside the tree of `rev`.
        if let Some((rev, path)) = arg.split_once(':') {
            if !path.is_empty() && !rev.is_empty() {
                return self.rev_path(rev, path).map_err(|_| unknown());
            }
        }

        // Peel operators: split off the last `~`/`^` group and recurse.
        if let Some((base, op, count)) = split_peel(arg) {
            let base_oid = self.resolve_inner(base)?;
            return self.peel(base_oid, op, count).map_err(|_| unknown());
        }

        // Full-length hex oid.
        if arg.len() == self.hex_len && arg.bytes().all(|b| b.is_ascii_hexdigit()) {
            if let Ok(oid) = Oid::from_hex(arg, self.odb.algorithm()) {
                return Ok(oid);
            }
        }

        // Refname.
        let candidates = [
            arg.to_string(),
            "refs/".to_string() + arg,
            "refs/tags/".to_string() + arg,
            "refs/heads/".to_string() + arg,
            "refs/remotes/".to_string() + arg,
            "refs/remotes/".to_string() + arg + "/HEAD",
        ];
        for c in &candidates {
            if let Some(oid) = self.store.resolve(c) {
                return Ok(oid);
            }
        }

        // Abbreviated hex oid.
        if arg.len() >= 4 && arg.len() < self.hex_len && arg.bytes().all(|b| b.is_ascii_hexdigit()) {
            let matches = self.find_short_oid(arg);
            return match matches.len() {
                0 => Err(unknown()),
                1 => Ok(matches[0]),
                _ => Err(ResolveError::Ambiguous {
                    arg: arg.to_string(),
                    prefix: arg.to_string(),
                }),
            };
        }

        Err(unknown())
    }

    /// The shortest unambiguous abbreviation length for `oid` that is at
    /// least `min` (C git's `find_unique_abbrev`).
    pub fn unique_abbrev_len(&self, oid: &Oid, min: usize) -> usize {
        let hex = format!("{oid}");
        for len in min..=hex.len() {
            if self.find_short_oid(&hex[..len]).len() <= 1 {
                return len;
            }
        }
        hex.len()
    }

    /// Look up a hex prefix among loose and packed objects.
    fn find_short_oid(&self, prefix: &str) -> Vec<Oid> {
        let mut out = Vec::new();
        // Loose: walk the fanout directory matching the first two chars.
        if prefix.len() >= 2 {
            let objects_dir = self.odb.loose.objects_dir();
            let fan = &prefix[..2];
            let rest = &prefix[2..];
            if let Ok(rd) = std::fs::read_dir(objects_dir.join(fan)) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.len() == self.hex_len - 2 && name.starts_with(rest) {
                        if let Ok(oid) = Oid::from_hex(&format!("{fan}{name}"), self.odb.algorithm()) {
                            out.push(oid);
                        }
                    }
                }
            }
        }
        // Packed: the idx oid lists are sorted by oid, so a prefix range is
        // found with a binary search over the hex rendering.
        for idx in self.odb.packs.iter().map(|(_, i)| i) {
            let lo = idx
                .oids()
                .partition_point(|o| oid_hex(o).as_str() < prefix);
            for o in &idx.oids()[lo..] {
                if oid_hex(o).starts_with(prefix) {
                    out.push(o.clone());
                } else {
                    break;
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// Resolve `<rev>:<path>`: dereference commits to trees, then walk the
    /// path components.
    fn rev_path(&self, rev: &str, path: &str) -> Result<Oid, ()> {
        let rev_oid = self.resolve(rev).map_err(|_| ())?;
        let mut cur = self.odb.read(&rev_oid).map_err(|_| ())?;
        if cur.kind == ObjectKind::Commit {
            let tree_oid = commit_tree_oid(&cur.data, self.odb.algorithm()).ok_or(())?;
            cur = self.odb.read(&tree_oid).map_err(|_| ())?;
        }
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        for (i, part) in parts.iter().enumerate() {
            if cur.kind != ObjectKind::Tree {
                return Err(());
            }
            let entries = parse_tree(&cur.data, self.odb.algorithm()).map_err(|_| ())?;
            let entry = entries
                .iter()
                .find(|e| String::from_utf8_lossy(&e.name) == *part)
                .ok_or(())?;
            let oid = entry.oid.clone();
            if i + 1 == parts.len() {
                return Ok(oid);
            }
            cur = self.odb.read(&oid).map_err(|_| ())?;
        }
        Err(())
    }

    /// Apply `~<n>` / `^<n>` peeling starting from `oid`.
    fn peel(&self, mut oid: Oid, op: char, count: u64) -> Result<Oid, ()> {
        match op {
            '~' => {
                for _ in 0..count {
                    oid = self.first_parent(&oid).ok_or(())?;
                }
                Ok(oid)
            }
            '^' => {
                if count == 0 {
                    // `<rev>^{}` peels to the object itself; `<rev>^0` is the
                    // commit itself for commit-ish arguments.
                    return Ok(oid);
                }
                self.nth_parent(&oid, count as usize).ok_or(())
            }
            _ => Err(()),
        }
    }

    fn commit_parents(&self, oid: &Oid) -> Option<Vec<Oid>> {
        let obj = self.odb.read(oid).ok()?;
        let commit = parse_commit(&obj.data, self.odb.algorithm()).ok()?;
        Some(commit.parents)
    }

    fn first_parent(&self, oid: &Oid) -> Option<Oid> {
        self.commit_parents(oid).and_then(|p| p.into_iter().next())
    }

    fn nth_parent(&self, oid: &Oid, n: usize) -> Option<Oid> {
        self.commit_parents(oid).and_then(|p| p.into_iter().nth(n - 1))
    }
}

fn oid_hex(o: &Oid) -> String {
    format!("{o}")
}

/// Split the trailing `~<n>` / `^<n>` group off a revision string.
/// Returns `(base, op, count)`.
fn split_peel(arg: &str) -> Option<(&str, char, u64)> {
    let idx = arg.rfind(['~', '^'])?;
    // A `:` rev:path expression never mixes with peel operators in the
    // argument; peel before the colon only when the operator comes last.
    let (base, suffix) = arg.split_at(idx);
    if base.is_empty() {
        return None;
    }
    let op = arg.as_bytes()[idx] as char;
    let count = if suffix.len() > 1 {
        suffix[1..].parse::<u64>().ok()?
    } else {
        1
    };
    if op == '~' && suffix.len() > 1 && suffix[1..].is_empty() {
        return None;
    }
    Some((base, op, count))
}

/// Extract the tree id from a commit object's raw bytes (`tree <hex>` line).
fn commit_tree_oid(data: &[u8], algo: git_hash::HashAlgorithm) -> Option<Oid> {
    let line_end = data.iter().position(|&b| b == b'\n')?;
    let head = std::str::from_utf8(&data[..line_end]).ok()?;
    let hex = head.strip_prefix("tree ")?;
    Oid::from_hex(hex, algo).ok()
}
