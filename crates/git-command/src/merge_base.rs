//! `git merge-base`: find common ancestors of two commits.

use std::io::Write;

use crate::{Command, CommandError};
use git_core::Repository;
use git_hash::Oid;
use git_merge::merge_bases;
use git_object::{parse_commit, ObjectKind};
use git_odb::Odb;

pub struct MergeBase;

impl Command for MergeBase {
    fn name(&self) -> &'static str {
        "merge-base"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut all = false;
        let mut rest: Vec<String> = Vec::new();
        for a in args {
            match a.as_str() {
                "--all" => all = true,
                "--octopus" | "--independent" | "--is-ancestor" => {
                    return Err(CommandError::usage(format!("merge-base: option '{a}' not supported")));
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("merge-base: option '{s}' not supported")));
                }
                s => rest.push(s.to_string()),
            }
        }
        if rest.len() != 2 {
            return Err(CommandError::usage("merge-base: requires two <commit> arguments"));
        }

        let repo = Repository::discover()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let algo = repo.hash_algo;
        let a = crate::resolve_arg(&repo, &rest[0])?;
        let b = crate::resolve_arg(&repo, &rest[1])?;

        let mut loader = |oid: &Oid| -> Vec<Oid> {
            odb.read(oid)
                .ok()
                .filter(|o| o.kind == ObjectKind::Commit)
                .and_then(|o| parse_commit(&o.data, algo).ok())
                .map(|c| c.parents)
                .unwrap_or_default()
        };
        let bases = merge_bases(&a, &b, &mut loader);
        if bases.is_empty() {
            return Err(CommandError::error("fatal: no merge base found"));
        }
        let _ = algo;
        if all {
            let mut sorted = bases;
            sorted.sort();
            for oid in sorted {
                writeln!(out, "{oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        } else {
            writeln!(out, "{}", bases[0]).map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}