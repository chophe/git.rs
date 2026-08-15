//! Repository discovery and layout.
//!
//! A git-compatible subset of `setup.c` / `environment.c`: locate the `.git`
//! directory (including via `gitdir:` files and the `GIT_DIR` environment
//! variable), resolve the common directory, read the repository config, and
//! determine the work tree.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use git_config::ConfigSet;
use git_hash::HashAlgorithm;

pub mod strbuf;
pub use strbuf::StringBuf;

/// Errors returned while discovering a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoError {
    NotFound,
    Config(git_config::ConfigError),
    Io(String),
}

impl fmt::Display for RepoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RepoError::NotFound => {
                write!(f, "not a git repository (or any of the parent directories)")
            }
            RepoError::Config(e) => write!(f, "{e}"),
            RepoError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl Error for RepoError {}

impl From<git_config::ConfigError> for RepoError {
    fn from(e: git_config::ConfigError) -> RepoError {
        RepoError::Config(e)
    }
}

/// Environment / command-line overrides for repository layout.
#[derive(Debug, Clone, Default)]
pub struct RepoEnv {
    pub git_dir: Option<PathBuf>,
    pub work_tree: Option<PathBuf>,
    pub common_dir: Option<PathBuf>,
}

impl RepoEnv {
    /// Read the standard `GIT_DIR`, `GIT_WORK_TREE`, and `GIT_COMMON_DIR`
    /// environment variables.
    pub fn from_env() -> RepoEnv {
        RepoEnv {
            git_dir: std::env::var_os("GIT_DIR").map(PathBuf::from),
            work_tree: std::env::var_os("GIT_WORK_TREE").map(PathBuf::from),
            common_dir: std::env::var_os("GIT_COMMON_DIR").map(PathBuf::from),
        }
    }
}

/// A discovered repository.
#[derive(Debug, Clone)]
pub struct Repository {
    /// The `.git` directory.
    pub git_dir: PathBuf,
    /// The shared/common directory (same as `git_dir` unless a `commondir`
    /// file redirects it).
    pub common_dir: PathBuf,
    /// The work tree, if any.
    pub work_tree: Option<PathBuf>,
    /// Whether this is a bare repository.
    pub bare: bool,
    /// The repository's hash algorithm.
    pub hash_algo: HashAlgorithm,
    /// The merged repository configuration.
    pub config: ConfigSet,
}

impl Repository {
    /// Discover a repository starting from `start`, applying `env` overrides.
    pub fn discover_from(start: &Path, env: &RepoEnv) -> Result<Repository, RepoError> {
        let git_dir = match &env.git_dir {
            Some(g) => make_absolute(start, g),
            None => find_git_dir(start)?.ok_or(RepoError::NotFound)?,
        };
        let git_dir = canonicalize_preserve(&git_dir);

        let common_dir = match &env.common_dir {
            Some(c) => make_absolute(&git_dir, c),
            None => match read_commondir(&git_dir)? {
                Some(c) => make_absolute(&git_dir, &c),
                None => git_dir.clone(),
            },
        };
        let common_dir = canonicalize_preserve(&common_dir);

        let config = match std::fs::read(common_dir.join("config")) {
            Ok(data) => ConfigSet::parse(&data)?,
            Err(_) => ConfigSet::new(),
        };

        let bare = config.get_bool("core", "bare").unwrap_or(false);
        let hash_algo = match config.get("extensions", "objectformat") {
            Some("sha256") => HashAlgorithm::Sha256,
            _ => HashAlgorithm::Sha1,
        };

        let work_tree = match &env.work_tree {
            Some(w) => Some(canonicalize_preserve(&make_absolute(start, w))),
            None if bare => None,
            None => git_dir.parent().map(|p| p.to_path_buf()),
        };

        Ok(Repository {
            git_dir,
            common_dir,
            work_tree,
            bare,
            hash_algo,
            config,
        })
    }

    /// Discover a repository starting at the current directory.
    pub fn discover() -> Result<Repository, RepoError> {
        let start = std::env::current_dir().map_err(|e| RepoError::Io(e.to_string()))?;
        Repository::discover_from(&start, &RepoEnv::from_env())
    }
}

/// Walk up from `start` looking for a `.git` directory or `gitdir:` file.
fn find_git_dir(start: &Path) -> Result<Option<PathBuf>, RepoError> {
    let start = if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| RepoError::Io(e.to_string()))?
            .join(start)
    };
    let mut dir = Some(start.as_path());
    while let Some(d) = dir {
        let candidate = d.join(".git");
        if candidate.is_dir() {
            return Ok(Some(candidate));
        }
        if candidate.is_file() {
            if let Ok(content) = std::fs::read_to_string(&candidate) {
                if let Some(target) = content.strip_prefix("gitdir:") {
                    let target = target.trim();
                    if !target.is_empty() {
                        return Ok(Some(make_absolute(d, Path::new(target))));
                    }
                }
            }
            return Ok(Some(candidate));
        }
        dir = d.parent();
    }
    Ok(None)
}

/// Read the `commondir` file if present.
fn read_commondir(git_dir: &Path) -> Result<Option<PathBuf>, RepoError> {
    match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(content) => {
            let p = content.trim();
            if p.is_empty() {
                Ok(None)
            } else {
                Ok(Some(PathBuf::from(p)))
            }
        }
        Err(_) => Ok(None),
    }
}

/// Join `p` onto `base` unless `p` is already absolute.
fn make_absolute(base: &Path, p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

/// Canonicalize without failing on nonexistent paths.
fn canonicalize_preserve(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Create a temporary directory unique per test (canonicalized so
    /// comparisons against `Repository` paths are stable on macOS).
    fn tempdir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("git-core-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn init_repo(base: &Path) -> PathBuf {
        let git_dir = base.join(".git");
        std::fs::create_dir_all(git_dir.join("objects")).unwrap();
        std::fs::create_dir_all(git_dir.join("refs")).unwrap();
        git_dir
    }

    #[test]
    fn discovers_git_dir_from_subdir() {
        let base = tempdir();
        let git_dir = init_repo(&base);
        let sub = base.join("a/b/c");
        std::fs::create_dir_all(&sub).unwrap();

        let repo = Repository::discover_from(&sub, &RepoEnv::default()).unwrap();
        assert_eq!(repo.git_dir, git_dir);
        assert_eq!(repo.common_dir, git_dir);
        assert_eq!(repo.work_tree.as_deref(), Some(base.as_path()));
        assert!(!repo.bare);
        assert_eq!(repo.hash_algo, HashAlgorithm::Sha1);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn respects_git_dir_env() {
        let base = tempdir();
        let git_dir = init_repo(&base);
        let env = RepoEnv {
            git_dir: Some(git_dir.clone()),
            work_tree: None,
            common_dir: None,
        };
        let repo = Repository::discover_from(&base, &env).unwrap();
        assert_eq!(repo.git_dir, git_dir);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn reads_config_and_object_format() {
        let base = tempdir();
        let git_dir = init_repo(&base);
        std::fs::write(
            git_dir.join("config"),
            "[core]\n\tbare = false\n[extensions]\n\tobjectformat = sha256\n",
        )
        .unwrap();

        let repo = Repository::discover_from(&base, &RepoEnv::default()).unwrap();
        assert_eq!(repo.hash_algo, HashAlgorithm::Sha256);
        assert_eq!(repo.get_bool("core", "bare"), Some(false));

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn gitdir_file_indirection() {
        let base = tempdir();
        let real_git = base.join("real-git");
        std::fs::create_dir_all(real_git.join("objects")).unwrap();
        std::fs::create_dir_all(real_git.join("refs")).unwrap();
        // .git is a file pointing at a sibling directory relative to `base`.
        std::fs::write(base.join(".git"), "gitdir: real-git\n").unwrap();

        let sub = base.join("src");
        std::fs::create_dir_all(&sub).unwrap();
        let repo = Repository::discover_from(&sub, &RepoEnv::default()).unwrap();
        assert_eq!(repo.git_dir, real_git);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn commondir_redirect() {
        let base = tempdir();
        let git_dir = init_repo(&base);
        let common = base.join("shared");
        std::fs::create_dir_all(&common).unwrap();
        std::fs::write(git_dir.join("commondir"), "../shared\n").unwrap();
        std::fs::write(common.join("config"), "[core]\n\tbare = true\n").unwrap();

        let repo = Repository::discover_from(&base, &RepoEnv::default()).unwrap();
        assert_eq!(repo.common_dir, common);
        assert!(repo.bare);
        assert_eq!(repo.work_tree, None);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn not_a_repository() {
        let base = tempdir();
        let err = Repository::discover_from(&base, &RepoEnv::default()).unwrap_err();
        assert_eq!(err, RepoError::NotFound);
        std::fs::remove_dir_all(&base).ok();
    }
}

impl Repository {
    /// Convenience typed accessor delegating to the repository config.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.config.get(section, key)
    }

    /// Convenience typed bool accessor delegating to the repository config.
    pub fn get_bool(&self, section: &str, key: &str) -> Option<bool> {
        self.config.get_bool(section, key)
    }
}
