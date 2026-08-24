//! `git log`: walk commits and show them.

use std::io::Write;

use crate::{Command, CommandError};
use git_core::Repository;
use git_hash::Oid;
use git_object::{parse_commit, ObjectKind};
use git_odb::Odb;
use git_revision::RevWalk;

pub struct Log;

impl Command for Log {
    fn name(&self) -> &'static str {
        "log"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut oneline = false;
        let mut tip: Option<String> = None;
        for a in args {
            match a.as_str() {
                "--oneline" => oneline = true,
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("log: option '{s}' not supported")));
                }
                s => tip = Some(s.to_string()),
            }
        }
        let tip = tip.ok_or_else(|| CommandError::usage("log: missing commit"))?;

        let repo = Repository::discover()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let algo = repo.hash_algo;
        let tip_oid = crate::resolve_arg(&repo, &tip)?;

        let mut loader = |oid: &Oid| -> Option<git_object::Commit> {
            let obj = odb.read(oid).ok()?;
            if obj.kind != ObjectKind::Commit {
                return None;
            }
            parse_commit(&obj.data, algo).ok()
        };
        let mut walk = RevWalk::new(&mut loader, git_revision::WalkOptions::default());
        let ids = walk.walk(&[tip_oid]);

        for oid in &ids {
            let commit = loader(oid).ok_or_else(|| CommandError::error("bad commit"))?;
            let subject = String::from_utf8_lossy(&commit.message)
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            if oneline {
                writeln!(out, "{} {}", &oid.to_string()[..7.min(oid.to_string().len())], subject)
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
            } else {
                writeln!(out, "commit {oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
                if let Some(author) = &commit.author {
                    writeln!(out, "Author: {author}").map_err(|e| CommandError::fatal(e.to_string()))?;
                }
                writeln!(out).map_err(|e| CommandError::fatal(e.to_string()))?;
                for line in String::from_utf8_lossy(&commit.message).lines() {
                    writeln!(out, "    {line}").map_err(|e| CommandError::fatal(e.to_string()))?;
                }
                writeln!(out).map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }
        Ok(())
    }
}
