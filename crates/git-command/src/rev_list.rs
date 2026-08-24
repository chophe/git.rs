//! `git rev-list`: list commit object ids.

use std::io::Write;

use crate::{Command, CommandError};
use git_core::Repository;
use git_hash::Oid;
use git_object::{parse_commit, ObjectKind};
use git_odb::Odb;
use git_revision::RevWalk;

pub struct RevList;

impl Command for RevList {
    fn name(&self) -> &'static str {
        "rev-list"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut with_parents = false;
        let mut tips: Vec<String> = Vec::new();
        for a in args {
            match a.as_str() {
                "--parents" => with_parents = true,
                "--topo-order" | "--date-order" | "--reverse" | "--objects" | "--all" => {}
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("rev-list: option '{s}' not supported")));
                }
                s => tips.push(s.to_string()),
            }
        }
        if tips.is_empty() {
            return Err(CommandError::usage("rev-list: missing commit"));
        }

        let repo = Repository::discover()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let algo = repo.hash_algo;

        let mut tip_oids = Vec::with_capacity(tips.len());
        for t in &tips {
            tip_oids.push(crate::resolve_arg(&repo, t)?);
        }

        let mut loader = |oid: &Oid| -> Option<git_object::Commit> {
            let obj = odb.read(oid).ok()?;
            if obj.kind != ObjectKind::Commit {
                return None;
            }
            parse_commit(&obj.data, algo).ok()
        };
        let mut walk = RevWalk::new(&mut loader, git_revision::WalkOptions { follow_all_parents: true });
        let ids = walk.walk(&tip_oids);

        for oid in &ids {
            if with_parents {
                let parents = loader(oid)
                    .map(|c| c.parents)
                    .unwrap_or_default();
                let line = std::iter::once(oid.to_string())
                    .chain(parents.iter().map(|p| p.to_string()))
                    .collect::<Vec<_>>()
                    .join(" ");
                writeln!(out, "{line}").map_err(|e| CommandError::fatal(e.to_string()))?;
            } else {
                writeln!(out, "{oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }
        Ok(())
    }
}
