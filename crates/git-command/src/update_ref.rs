//! `git update-ref` and `git symbolic-ref`.

use std::io::Write;

use crate::{Command, CommandError};
use git_core::Repository;
use git_hash::Oid;
use git_refs::RefStore;

pub struct UpdateRef;

impl Command for UpdateRef {
    fn name(&self) -> &'static str {
        "update-ref"
    }

    fn run(&self, args: &[String], _out: &mut dyn Write) -> Result<(), CommandError> {
        let mut delete = false;
        let mut rest: Vec<String> = Vec::new();
        for a in args {
            match a.as_str() {
                "-d" | "--delete" => delete = true,
                "-m" | "--create-reflog" => {}
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("update-ref: option '{s}' not supported")));
                }
                s => rest.push(s.to_string()),
            }
        }

        let repo = Repository::discover()?;
        let store = RefStore::from_repo(&repo);
        let algo = repo.hash_algo;

        if delete {
            if rest.len() != 1 {
                return Err(CommandError::usage("update-ref -d: requires <ref>"));
            }
            store
                .update(&rest[0], None)
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            return Ok(());
        }

        if rest.len() != 2 {
            return Err(CommandError::usage("update-ref: requires <ref> <new-oid>"));
        }
        let oid = Oid::from_hex(&rest[1], algo)
            .map_err(|_| CommandError::error(format!("invalid object name '{}'", rest[1])))?;
        store
            .update(&rest[0], Some(&oid))
            .map_err(|e| CommandError::fatal(e.to_string()))?;
        Ok(())
    }
}

pub struct SymbolicRef;

impl Command for SymbolicRef {
    fn name(&self) -> &'static str {
        "symbolic-ref"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut short = false;
        let mut name: Option<String> = None;
        for a in args {
            match a.as_str() {
                "--short" => short = true,
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("symbolic-ref: option '{s}' not supported")));
                }
                s => name = Some(s.to_string()),
            }
        }
        let name = name.ok_or_else(|| CommandError::usage("symbolic-ref: missing <name>"))?;
        let repo = Repository::discover()?;
        let store = RefStore::from_repo(&repo);
        let target = store
            .head_symbolic_target()
            .ok_or_else(|| CommandError::error(format!("ref '{name}' is not a symbolic ref")))?;
        if short {
            let short_name = target.strip_prefix("refs/heads/").unwrap_or(&target);
            writeln!(out, "{short_name}").map_err(|e| CommandError::fatal(e.to_string()))?;
        } else {
            writeln!(out, "{target}").map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}