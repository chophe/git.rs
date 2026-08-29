//! `git log`: walk commits and show them via the pretty engine.

use std::io::Write;

use crate::{Command, CommandError, RepoContext};
use git_hash::Oid;
use git_object::{parse_commit, ObjectKind};
use git_odb::Odb;
use git_pretty::{CommitInfo, Format, Options};
use git_revision::RevWalk;

pub struct Log;

impl Command for Log {
    fn name(&self) -> &'static str {
        "log"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut format = Format::Medium;
        let mut date_mode = git_pretty::date::DateMode::Default;
        let mut tip: Option<String> = None;
        let mut max_count: Option<usize> = None;
        let mut skip = 0usize;
        let mut reverse = false;
        let mut no_merges = false;
        let mut only_first_parent = false;
        for a in args {
            match a.as_str() {
                "--oneline" => format = Format::UserTerminated("%h %s".to_string()),
                "--reverse" => reverse = true,
                "--no-merges" => no_merges = true,
                "--first-parent" => only_first_parent = true,
                s if s.starts_with("--pretty=") => {
                    let spec = &s["--pretty=".len()..];
                    format = Format::parse(spec).ok_or_else(|| {
                        CommandError::fatal(format!("fatal: invalid --pretty format: {spec}"))
                    })?;
                }
                s if s.starts_with("--format=") => {
                    // `--format=<str>` is `tformat:<str>` semantics.
                    format = Format::UserTerminated(s["--format=".len()..].to_string());
                }
                "--pretty" | "--format" => format = Format::Medium,
                s if s.starts_with("--date=") => {
                    let spec = &s["--date=".len()..];
                    date_mode = git_pretty::date::DateMode::parse(spec).ok_or_else(|| {
                        CommandError::error(format!("fatal: invalid date format: {spec}"))
                    })?;
                }
                s if s.starts_with("-n") && s.len() > 2 => {
                    max_count = Some(s[2..].parse().unwrap_or(0));
                }
                s if s.starts_with("--max-count=") => {
                    max_count = Some(s["--max-count=".len()..].parse().unwrap_or(0));
                }
                s if s.starts_with("--skip=") => {
                    skip = s["--skip=".len()..].parse().unwrap_or(0);
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("log: option '{s}' not supported")));
                }
                s => tip = Some(s.to_string()),
            }
        }
        let tip = tip.unwrap_or_else(|| "HEAD".to_string());

        let repo = ctx.repository()?;
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
        let mut walk_opts = git_revision::WalkOptions::default();
        walk_opts.follow_all_parents = !only_first_parent;
        let mut walk = RevWalk::new(&mut loader, walk_opts);
        let mut ids = walk.walk(&[tip_oid]);
        if no_merges {
            ids.retain(|oid| loader(oid).map(|c| c.parents.len() <= 1).unwrap_or(true));
        }
        if reverse {
            ids.reverse();
        }
        if skip > 0 {
            ids = ids.into_iter().skip(skip).collect();
        }
        if let Some(n) = max_count {
            ids.truncate(n);
        }

        // Abbreviation length: extend `%h` until unambiguous (C git's
        // default_abbrev + unique extension).
        let resolver = git_revision::Resolver::new(&repo).ok();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut opts = Options {
            date: date_mode,
            abbrev: 7,
            color: false,
            now,
        };

        let mut first = true;
        for oid in &ids {
            let obj = odb.read(oid).map_err(|_| CommandError::error("bad commit"))?;
            let info = CommitInfo::parse(*oid, &obj.data, algo)
                .ok_or_else(|| CommandError::error("bad commit"))?;
            if let Some(resolver) = &resolver {
                opts.abbrev = resolver.unique_abbrev_len(oid, 7);
            }
            if !first && !format.is_oneline() {
                writeln!(out).map_err(|e| CommandError::fatal(e.to_string()))?;
            }
            first = false;
            git_pretty::format_commit(&format, &info, &opts, out)
                .map_err(|e| CommandError::error(e.to_string()))?;
        }
        Ok(())
    }
}
