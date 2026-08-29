//! `git ls-files`: list files in the index.

use std::io::Write;

use crate::{Command, CommandError, RepoContext};
use git_index::Index;

pub struct LsFiles;

impl Command for LsFiles {
    fn name(&self) -> &'static str {
        "ls-files"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut stage = false;
        for a in args {
            match a.as_str() {
                "--stage" => stage = true,
                "--cached" | "--debug" => {}
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("ls-files: option '{s}' not supported")));
                }
                _ => {}
            }
        }
        let repo = ctx.repository()?;
        let algo = repo.hash_algo;
        let index = match Index::read(&repo.index_file(), algo) {
            Ok(i) => i,
            Err(_) => Index { version: 2, entries: vec![] },
        };

        if stage {
            // List every entry with `mode oid stage` columns.
            for e in &index.entries {
                writeln!(
                    out,
                    "{:06o} {} {}\t{}",
                    e.mode,
                    e.oid,
                    e.stage,
                    e.name
                )
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        } else {
            // Default: stage-0 paths.
            for e in &index.entries {
                if e.stage == 0 {
                    writeln!(out, "{}", e.name).map_err(|e| CommandError::fatal(e.to_string()))?;
                }
            }
        }
        Ok(())
    }
}