//! `git rev-parse`: resolve refs and query repository metadata.

use std::io::Write;

use crate::{Command, CommandError};
use git_core::Repository;
use git_hash::Oid;
use git_refs::RefStore;

pub struct RevParse;

impl Command for RevParse {
    fn name(&self) -> &'static str {
        "rev-parse"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let repo = Repository::discover()?;
        let store = RefStore::from_repo(&repo);
        let algo = repo.hash_algo;

        let mut verify = false;
        let mut rest: Vec<String> = Vec::new();
        for a in args {
            match a.as_str() {
                "--verify" => verify = true,
                "--git-dir" => {
                    println!("{}", repo.git_dir.display());
                }
                "--git-common-dir" => {
                    println!("{}", repo.common_dir.display());
                }
                "--show-toplevel" | "--is-inside-work-tree" => {}
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("rev-parse: option '{s}' not supported")));
                }
                s => rest.push(s.to_string()),
            }
        }

        for arg in &rest {
            if let Ok(oid) = Oid::from_hex(arg, algo) {
                writeln!(out, "{oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
                continue;
            }
            match store.resolve(arg) {
                Some(oid) => {
                    writeln!(out, "{oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
                }
                None => {
                    if verify {
                        return Err(CommandError::error("fatal: Needed a single revision"));
                    }
                    return Err(CommandError::error(format!("{arg}: unknown revision")));
                }
            }
        }
        Ok(())
    }
}