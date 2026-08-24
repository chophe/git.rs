//! Reference storage: the files backend and packed-refs reading.
//!
//! A port of the loose-refs + packed-refs reader from `refs.c` /
//! `refs/files-backend.c`. Loose refs are files containing a hex oid or a
//! `ref: <target>` symref; `packed-refs` holds the packed ones. Reftable
//! support is deferred.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use git_core::Repository;
use git_hash::{HashAlgorithm, Oid};

const MAX_SYMREF_DEPTH: usize = 10;

/// Errors from ref operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefError {
    Io(String),
    InvalidName(String),
    NotFound,
}

impl fmt::Display for RefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RefError::Io(e) => write!(f, "ref I/O error: {e}"),
            RefError::InvalidName(n) => write!(f, "invalid ref name '{n}'"),
            RefError::NotFound => write!(f, "ref not found"),
        }
    }
}

impl Error for RefError {}

/// What a loose ref file points at.
#[derive(Debug, Clone)]
enum RefTarget {
    Oid(Oid),
    SymRef(String),
}

/// A ref store rooted at a repository.
#[derive(Debug, Clone)]
pub struct RefStore {
    git_dir: PathBuf,
    common_dir: PathBuf,
    algo: HashAlgorithm,
}

impl RefStore {
    pub fn from_repo(repo: &Repository) -> RefStore {
        RefStore {
            git_dir: repo.git_dir.clone(),
            common_dir: repo.common_dir.clone(),
            algo: repo.hash_algo,
        }
    }

    /// Resolve a ref name (following symrefs) to an object id.
    pub fn resolve(&self, name: &str) -> Option<Oid> {
        let mut cur = name.to_string();
        for _ in 0..MAX_SYMREF_DEPTH {
            let target = self.read_loose(&cur).or_else(|| {
                self.packed().and_then(|p| p.get(&cur).copied().map(RefTarget::Oid))
            })?;
            match target {
                RefTarget::Oid(oid) => return Some(oid),
                RefTarget::SymRef(next) => cur = next,
            }
        }
        None
    }

    /// The refname `HEAD` points at, if it is a symbolic ref.
    pub fn head_symbolic_target(&self) -> Option<String> {
        match self.read_loose("HEAD")? {
            RefTarget::SymRef(t) => Some(t),
            _ => None,
        }
    }

    /// All refs (loose overrides packed), sorted by refname.
    pub fn list(&self) -> Vec<(String, Oid)> {
        let mut map: HashMap<String, Oid> = HashMap::new();
        if let Some(packed) = self.packed() {
            for (k, v) in packed {
                map.insert(k, v);
            }
        }
        for (name, oid) in self.list_loose() {
            map.insert(name, oid);
        }
        let mut refs: Vec<(String, Oid)> = map.into_iter().collect();
        refs.sort_by(|a, b| a.0.cmp(&b.0));
        refs
    }

    /// Create or update a ref (atomic: temp file + rename).
    pub fn update(&self, name: &str, oid: Option<&Oid>) -> Result<(), RefError> {
        validate_refname(name)?;
        let path = self.common_dir.join(name);
        match oid {
            Some(oid) => {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| RefError::Io(e.to_string()))?;
                }
                let tmp = path.with_extension(format!("lock.{}", std::process::id()));
                std::fs::write(&tmp, format!("{oid}\n"))
                    .map_err(|e| RefError::Io(e.to_string()))?;
                std::fs::rename(&tmp, &path).map_err(|e| RefError::Io(e.to_string()))?;
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
        Ok(())
    }

    /// Read a loose ref file (oid or symref).
    fn read_loose(&self, name: &str) -> Option<RefTarget> {
        for dir in [&self.git_dir, &self.common_dir] {
            let p = dir.join(name);
            let content = std::fs::read_to_string(&p).ok()?;
            let t = content.trim();
            if let Some(target) = t.strip_prefix("ref:") {
                return Some(RefTarget::SymRef(target.trim().to_string()));
            }
            if let Ok(oid) = Oid::from_hex(t, self.algo) {
                return Some(RefTarget::Oid(oid));
            }
        }
        None
    }

    /// The packed-refs map, if the file exists.
    fn packed(&self) -> Option<HashMap<String, Oid>> {
        let path = self.common_dir.join("packed-refs");
        let content = std::fs::read_to_string(&path).ok()?;
        let mut map = HashMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            let mut it = line.splitn(2, ' ');
            let oid_s = it.next()?;
            let name = it.next()?;
            if let Ok(oid) = Oid::from_hex(oid_s, self.algo) {
                map.insert(name.to_string(), oid);
            }
        }
        Some(map)
    }

    /// Walk the loose refs under `refs/`.
    fn list_loose(&self) -> Vec<(String, Oid)> {
        let mut out = Vec::new();
        self.walk_refs(&self.common_dir.join("refs"), "", &mut out);
        out
    }

    fn walk_refs(&self, dir: &Path, prefix: &str, out: &mut Vec<(String, Oid)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let full = if prefix.is_empty() {
                format!("refs/{name}")
            } else {
                format!("{prefix}/{name}")
            };
            if e.path().is_dir() {
                self.walk_refs(&e.path(), &full, out);
            } else if let Some(RefTarget::Oid(oid)) = self.read_loose(&full) {
                out.push((full, oid));
            }
        }
    }
}

/// Validate a ref name against git's rules (subset).
pub fn validate_refname(name: &str) -> Result<(), RefError> {
    if !name.starts_with("refs/") {
        return Err(RefError::InvalidName(name.to_string()));
    }
    if name.contains("..")
        || name.contains("@{")
        || name.contains(".lock")
        || name.ends_with('/')
        || name.contains("//")
        || name.contains('~')
        || name.contains('^')
        || name.contains(':')
        || name.contains('?')
        || name.contains('*')
        || name.contains('[')
        || name.contains('\\')
    {
        return Err(RefError::InvalidName(name.to_string()));
    }
    if name.bytes().any(|b| b.is_ascii_control() || b == b' ') {
        return Err(RefError::InvalidName(name.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_core::RepoEnv;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn repo() -> (Repository, PathBuf) {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("git-refs-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let git = dir.join(".git");
        std::fs::create_dir_all(git.join("refs/heads")).unwrap();
        std::fs::create_dir_all(git.join("refs/tags")).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let repo = Repository::discover_from(&dir, &RepoEnv::default()).unwrap();
        (repo, dir)
    }

    #[test]
    fn resolves_and_updates_refs() {
        let (repo, dir) = repo();
        let store = RefStore::from_repo(&repo);
        let oid = *HashAlgorithm::Sha1.empty_blob();
        store.update("refs/heads/main", Some(&oid)).unwrap();
        assert_eq!(store.resolve("refs/heads/main"), Some(oid));
        // Symref: HEAD -> refs/heads/main.
        assert_eq!(store.resolve("HEAD"), Some(oid));
        assert_eq!(store.head_symbolic_target().as_deref(), Some("refs/heads/main"));
        // Delete.
        store.update("refs/heads/main", None).unwrap();
        assert_eq!(store.resolve("refs/heads/main"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lists_refs() {
        let (repo, dir) = repo();
        let store = RefStore::from_repo(&repo);
        let oid = *HashAlgorithm::Sha1.empty_blob();
        store.update("refs/heads/a", Some(&oid)).unwrap();
        store.update("refs/heads/b", Some(&oid)).unwrap();
        store.update("refs/tags/t", Some(&oid)).unwrap();
        let refs = store.list();
        assert_eq!(
            refs,
            vec![
                ("refs/heads/a".to_string(), oid),
                ("refs/heads/b".to_string(), oid),
                ("refs/tags/t".to_string(), oid),
            ]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reads_packed_refs() {
        let (repo, dir) = repo();
        let oid = *HashAlgorithm::Sha1.empty_blob();
        std::fs::write(
            dir.join(".git/packed-refs"),
            format!("# pack-refs with: peeled fully-peeled sorted\n{oid} refs/heads/packed\n"),
        )
        .unwrap();
        let store = RefStore::from_repo(&repo);
        assert_eq!(store.resolve("refs/heads/packed"), Some(oid));
        assert!(store.list().iter().any(|(n, _)| n == "refs/heads/packed"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn validates_names() {
        assert!(validate_refname("refs/heads/main").is_ok());
        assert!(validate_refname("refs/heads/feature/x").is_ok());
        assert!(validate_refname("refs/heads/main..evil").is_err());
        assert!(validate_refname("refs/heads/").is_err());
        assert!(validate_refname("main").is_err());
    }
}