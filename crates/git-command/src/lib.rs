//! Plumbing command implementations.
//!
//! This crate mirrors git's `builtin/` layout: each command is a unit struct
//! implementing [`Command`], and [`dispatch`] routes a subcommand name to its
//! implementation. Commands write their primary output to the caller-supplied
//! writer (so they are unit-testable without spawning processes) and report
//! failures through [`CommandError`].

pub mod apply;
pub mod cat_file;
pub mod commit_graph;
pub mod commit_tree;
pub mod count_objects;
pub mod diff;
pub mod diff_tree;
pub mod fsck;
pub mod hash_object;
pub mod index_pack;
pub mod ident;
pub mod log;
pub mod merge_base;
pub mod merge_file;
pub mod ls_files;
pub mod ls_tree;
pub mod mktree;
pub mod multi_pack_index;
pub mod pack_objects;
pub mod patch;
pub mod rev_list;
pub mod rev_parse;
pub mod show_ref;
pub mod status;
pub mod unpack_objects;
pub mod update_index;
pub mod update_ref;
pub mod verify_pack;

use std::error::Error;
use std::fmt;
use std::io::Write;
use std::path::PathBuf;

use git_core::{RepoEnv, RepoError, Repository};
use git_commitgraph::GraphError;
use git_odb::pack::{MidxError, PackError};
use git_odb::OdbError;

/// An error returned by a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    pub message: String,
    /// Process exit code to use.
    pub code: i32,
}

impl CommandError {
    /// A usage error (git exits 129).
    pub fn usage(message: impl Into<String>) -> CommandError {
        CommandError { message: message.into(), code: 129 }
    }

    /// A fatal error (git exits 128).
    pub fn fatal(message: impl Into<String>) -> CommandError {
        CommandError { message: message.into(), code: 128 }
    }

    /// A general error (git exits 1).
    pub fn error(message: impl Into<String>) -> CommandError {
        CommandError { message: message.into(), code: 1 }
    }

    /// An exit code with no message (e.g. `git diff` returning 1 when files
    /// differ).
    pub fn silent(code: i32) -> CommandError {
        CommandError { message: String::new(), code }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for CommandError {}

impl From<RepoError> for CommandError {
    fn from(e: RepoError) -> CommandError {
        CommandError::fatal(format!("fatal: {e}"))
    }
}

impl From<OdbError> for CommandError {
    fn from(e: OdbError) -> CommandError {
        CommandError::fatal(e.to_string())
    }
}

impl From<PackError> for CommandError {
    fn from(e: PackError) -> CommandError {
        CommandError::fatal(e.to_string())
    }
}

impl From<MidxError> for CommandError {
    fn from(e: MidxError) -> CommandError {
        CommandError::fatal(e.to_string())
    }
}

impl From<GraphError> for CommandError {
    fn from(e: GraphError) -> CommandError {
        CommandError::fatal(e.to_string())
    }
}

/// The per-invocation repository context.
///
/// Every command receives this explicitly instead of calling
/// `Repository::discover()` itself. It carries the resolved working directory
/// (after `-C`), the `GIT_DIR`/`GIT_WORK_TREE`/`GIT_COMMON_DIR` overrides
/// (CLI flags take precedence over env vars, which take precedence over
/// discovery), and `git -c` config overrides.
#[derive(Debug, Clone)]
pub struct RepoContext {
    /// Effective working directory (`-C` applied; never mutates process cwd).
    pub cwd: PathBuf,
    pub git_dir: Option<PathBuf>,
    pub work_tree: Option<PathBuf>,
    pub common_dir: Option<PathBuf>,
    /// `--bare`: operate in bare-repository mode.
    pub bare: bool,
    /// `git -c name=value` overlays applied on top of the repo config.
    pub config_overrides: Vec<(String, Option<String>)>,
}

impl RepoContext {
    /// Build a context from the process environment and current directory.
    pub fn new() -> RepoContext {
        let cwd = std::env::current_dir()
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        RepoContext {
            cwd,
            git_dir: std::env::var_os("GIT_DIR").map(PathBuf::from),
            work_tree: std::env::var_os("GIT_WORK_TREE").map(PathBuf::from),
            common_dir: std::env::var_os("GIT_COMMON_DIR").map(PathBuf::from),
            bare: false,
            config_overrides: Vec::new(),
        }
    }

    /// Parse C-git global options appearing before the subcommand.
    ///
    /// Recognized: `-C <dir>`, `-c <name>[=<value>]`, `--git-dir[=]<path>`,
    /// `--work-tree[=]<path>`, `--common-dir[=]<path>`, `--bare`,
    /// `--no-pager`, `--paginate`, `--literal-pathspecs`. Returns the context
    /// and the remaining arguments (starting with the subcommand name).
    pub fn from_global_args(args: &[String]) -> Result<(RepoContext, Vec<String>), CommandError> {
        let mut ctx = RepoContext::new();
        let mut rest: Vec<String> = Vec::new();
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            if a == "-C" {
                i += 1;
                match args.get(i) {
                    Some(v) => ctx.cwd = ctx.cwd.join(v),
                    None => {
                        return Err(CommandError::usage("option `-C' requires a value"));
                    }
                }
            } else if let Some(v) = a.strip_prefix("-C") {
                if !v.is_empty() {
                    ctx.cwd = ctx.cwd.join(v);
                }
            } else if a == "-c" {
                i += 1;
                match args.get(i) {
                    Some(v) => ctx.config_overrides.push(split_config_pair(v)),
                    None => {
                        return Err(CommandError::usage("option `-c' requires a value"));
                    }
                }
            } else if let Some(v) = a.strip_prefix("--git-dir=") {
                ctx.git_dir = Some(PathBuf::from(v));
            } else if a == "--git-dir" {
                i += 1;
                match args.get(i) {
                    Some(v) => ctx.git_dir = Some(PathBuf::from(v)),
                    None => {
                        return Err(CommandError::usage(
                            "option `--git-dir' requires a value",
                        ));
                    }
                }
            } else if let Some(v) = a.strip_prefix("--work-tree=") {
                ctx.work_tree = Some(PathBuf::from(v));
            } else if a == "--work-tree" {
                i += 1;
                match args.get(i) {
                    Some(v) => ctx.work_tree = Some(PathBuf::from(v)),
                    None => {
                        return Err(CommandError::usage(
                            "option `--work-tree' requires a value",
                        ));
                    }
                }
            } else if let Some(v) = a.strip_prefix("--common-dir=") {
                ctx.common_dir = Some(PathBuf::from(v));
            } else if a == "--common-dir" {
                i += 1;
                match args.get(i) {
                    Some(v) => ctx.common_dir = Some(PathBuf::from(v)),
                    None => {
                        return Err(CommandError::usage(
                            "option `--common-dir' requires a value",
                        ));
                    }
                }
            } else if a == "--bare" {
                ctx.bare = true;
                ctx.work_tree = None;
            } else if a == "--no-pager" || a == "--paginate" || a == "--literal-pathspecs" {
                // Accepted for compatibility; paging is not implemented.
            } else if a.starts_with('-') && a.len() > 1 {
                return Err(CommandError::usage(format!(
                    "unknown option: {a}"
                )));
            } else {
                rest = args[i..].to_vec();
                break;
            }
            i += 1;
        }
        Ok((ctx, rest))
    }

    /// A context rooted at `dir`, with no overrides (useful for tests).
    pub fn at(dir: &std::path::Path) -> RepoContext {
        RepoContext {
            cwd: dir.to_path_buf(),
            git_dir: None,
            work_tree: None,
            common_dir: None,
            bare: false,
            config_overrides: Vec::new(),
        }
    }

    /// Discover the repository for this context, applying config overrides.
    pub fn repository(&self) -> Result<Repository, CommandError> {
        let env = RepoEnv {
            git_dir: self.git_dir.clone(),
            work_tree: self.work_tree.clone(),
            common_dir: self.common_dir.clone(),
            index_file: std::env::var_os("GIT_INDEX_FILE").map(PathBuf::from),
            object_dir: std::env::var_os("GIT_OBJECT_DIRECTORY").map(PathBuf::from),
            alternates: std::env::var_os("GIT_ALTERNATE_OBJECT_DIRECTORIES")
                .map(|v| {
                    std::env::split_paths(&v)
                        .filter(|p| !p.as_os_str().is_empty())
                        .collect()
                })
                .unwrap_or_default(),
        };
        let mut repo = Repository::discover_from(&self.cwd, &env).map_err(CommandError::from)?;
        if self.bare {
            repo.bare = true;
            repo.work_tree = None;
        }
        for (name, value) in &self.config_overrides {
            repo.config.set_cli(name, value.as_deref());
        }
        Ok(repo)
    }
}

/// Split `name=value` (missing `=` means boolean true, matching `git -c`).
fn split_config_pair(s: &str) -> (String, Option<String>) {
    match s.split_once('=') {
        Some((name, value)) => (name.to_string(), Some(value.to_string())),
        None => (s.to_string(), None),
    }
}

/// A git subcommand.
pub trait Command {
    /// The subcommand name used on the command line.
    fn name(&self) -> &'static str;

    /// Execute the command.
    ///
    /// `args` are the arguments following the subcommand name. Primary output
    /// is written to `out`.
    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError>;
}

/// Resolve a revision argument: a full hex oid, or a ref name (e.g. `HEAD`,
/// `refs/heads/main`, `main`).
pub fn resolve_arg(repo: &Repository, s: &str) -> Result<git_hash::Oid, CommandError> {
    if let Ok(oid) = git_hash::Oid::from_hex(s, repo.hash_algo) {
        return Ok(oid);
    }
    // `main` may abbreviate `refs/heads/main`.
    let candidates = [
        s.to_string(),
        format!("refs/heads/{s}"),
        format!("refs/tags/{s}"),
    ];
    let store = git_refs::RefStore::from_repo(repo);
    for c in &candidates {
        if let Some(oid) = store.resolve(c) {
            return Ok(oid);
        }
    }
    Err(CommandError::error(format!("Not a valid object name '{s}'")))
}

/// Route a subcommand name to its implementation.
///
/// Returns `None` if `name` is not a known command. The context is built from
/// the process environment (no global CLI options).
pub fn dispatch(name: &str, args: &[String], out: &mut dyn Write) -> Option<Result<(), CommandError>> {
    let ctx = RepoContext::new();
    dispatch_with(&ctx, name, args, out)
}

/// Route a subcommand name to its implementation using a caller-supplied
/// context (i.e. one built by parsing global CLI options).
pub fn dispatch_with(
    ctx: &RepoContext,
    name: &str,
    args: &[String],
    out: &mut dyn Write,
) -> Option<Result<(), CommandError>> {
    let cmd: &dyn Command = match name {
        "hash-object" => &hash_object::HashObject,
        "commit-tree" => &commit_tree::CommitTree,
        "verify-pack" => &verify_pack::VerifyPack,
        "unpack-objects" => &unpack_objects::UnpackObjects,
        "pack-objects" => &pack_objects::PackObjects,
        "count-objects" => &count_objects::CountObjects,
        "multi-pack-index" => &multi_pack_index::MultiPackIndex,
        "commit-graph" => &commit_graph::CommitGraphCmd,
        "cat-file" => &cat_file::CatFile,
        "ls-tree" => &ls_tree::LsTree,
        "mktree" => &mktree::MkTree,
        "rev-list" => &rev_list::RevList,
        "log" => &log::Log,
        "diff-tree" => &diff_tree::DiffTree,
        "diff" => &diff::Diff,
        "ls-files" => &ls_files::LsFiles,
        "update-index" => &update_index::UpdateIndex,
        "status" => &status::Status,
        "rev-parse" => &rev_parse::RevParse,
        "show-ref" => &show_ref::ShowRef,
        "for-each-ref" => &show_ref::ForEachRef,
        "update-ref" => &update_ref::UpdateRef,
        "symbolic-ref" => &update_ref::SymbolicRef,
        "branch" => &show_ref::Branch,
        "merge-base" => &merge_base::MergeBase,
        "merge-file" => &merge_file::MergeFile,
        "fsck" => &fsck::Fsck,
        "apply" => &apply::Apply,
        "index-pack" => &index_pack::IndexPack,
        "tag" => &show_ref::Tag,
        _ => return None,
    };
    Some(cmd.run(ctx, args, out))
}

#[cfg(test)]
pub mod tests {
    use std::path::Path;
    use std::sync::Mutex;

    /// Serializes tests that change the process working directory (the test
    /// binary runs all tests in one process, so a global lock avoids races).
    static CWD_LOCK: Mutex<()> = Mutex::new(());

    pub fn with_cwd<R>(dir: &Path, f: impl FnOnce() -> R) -> R {
        let _guard = CWD_LOCK.lock().unwrap();
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        std::env::set_current_dir(prev).unwrap();
        match result {
            Ok(r) => r,
            Err(p) => std::panic::resume_unwind(p),
        }
    }

    /// Serialize a test that mutates process-global state such as environment
    /// variables, so parallel tests cannot interfere.
    pub fn serialized<R>(f: impl FnOnce() -> R) -> R {
        let _guard = CWD_LOCK.lock().unwrap();
        f()
    }
}
