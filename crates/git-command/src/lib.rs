//! Plumbing command implementations.
//!
//! This crate mirrors git's `builtin/` layout: each command is a unit struct
//! implementing [`Command`], and [`dispatch`] routes a subcommand name to its
//! implementation. Commands write their primary output to the caller-supplied
//! writer (so they are unit-testable without spawning processes) and report
//! failures through [`CommandError`].

pub mod cat_file;
pub mod commit_graph;
pub mod commit_tree;
pub mod count_objects;
pub mod diff;
pub mod diff_tree;
pub mod hash_object;
pub mod ident;
pub mod log;
pub mod ls_tree;
pub mod mktree;
pub mod multi_pack_index;
pub mod pack_objects;
pub mod patch;
pub mod rev_list;
pub mod unpack_objects;
pub mod verify_pack;

use std::error::Error;
use std::fmt;
use std::io::Write;

use git_core::RepoError;
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
        CommandError::fatal(e.to_string())
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

/// A git subcommand.
pub trait Command {
    /// The subcommand name used on the command line.
    fn name(&self) -> &'static str;

    /// Execute the command.
    ///
    /// `args` are the arguments following the subcommand name. Primary output
    /// is written to `out`.
    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError>;
}

/// Route a subcommand name to its implementation.
///
/// Returns `None` if `name` is not a known command.
pub fn dispatch(name: &str, args: &[String], out: &mut dyn Write) -> Option<Result<(), CommandError>> {
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
        _ => return None,
    };
    Some(cmd.run(args, out))
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
