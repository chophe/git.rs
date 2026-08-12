//! `git count-objects`: report loose and packed object counts.

use std::io::Write;

use crate::{Command, CommandError};
use git_core::Repository;
use git_odb::Odb;

pub struct CountObjects;

impl Command for CountObjects {
    fn name(&self) -> &'static str {
        "count-objects"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut verbose = false;
        for a in args {
            match a.as_str() {
                "-v" => verbose = true,
                _ => return Err(CommandError::usage(format!("count-objects: unknown option '{a}'"))),
            }
        }
        let repo = Repository::discover()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let (loose, in_pack) = odb.object_counts();

        if verbose {
            writeln!(out, "count: {loose}").map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "size: 0").map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "in-pack: {in_pack}").map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "packs: {}", odb.packs.len()).map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "prune-packable: 0").map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "garbage: 0").map_err(|e| CommandError::fatal(e.to_string()))?;
        } else {
            writeln!(out, "{loose}").map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}
