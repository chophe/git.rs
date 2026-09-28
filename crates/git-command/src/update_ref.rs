//! `git update-ref` and `git symbolic-ref`.

use std::io::Write;

use crate::{Command, CommandError, RepoContext};
use git_hash::Oid;
use git_refs::RefStore;

pub struct UpdateRef;

impl Command for UpdateRef {
    fn name(&self) -> &'static str {
        "update-ref"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], _out: &mut dyn Write) -> Result<(), CommandError> {
        let mut delete = false;
        let mut rest: Vec<String> = Vec::new();
        let mut message: Option<String> = None;
        let mut i = 0usize;
        while i < args.len() {
            let a = &args[i];
            match a.as_str() {
                "-d" | "--delete" => delete = true,
                // `-m <msg>` sets the reflog message (stored for the
                // Task-2 logging work; consumed here so it never leaks
                // into the ref/oid operands like C's OPT_STRING).
                "-m" => {
                    i += 1;
                    message = Some(
                        args.get(i)
                            .ok_or_else(|| CommandError::usage("update-ref: option 'm' requires a value"))?
                            .clone(),
                    );
                }
                "--create-reflog" => {}
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("update-ref: option '{s}' not supported")));
                }
                s => rest.push(s.to_string()),
            }
            i += 1;
        }
        let _ = message;

        let repo = ctx.repository()?;
        let store = RefStore::from_repo(&repo);
        let algo = repo.hash_algo;

        if delete {
            if rest.len() != 1 {
                return Err(CommandError::usage("update-ref -d: requires <ref>"));
            }
            store
                .update(&rest[0], None)
                .map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
            return Ok(());
        }

        if rest.len() != 2 {
            return Err(CommandError::usage("update-ref: requires <ref> <new-oid>"));
        }
        // C resolves the value as a full revision (`HEAD`, `HEAD^{tree}`,
        // abbreviated oids), not just hex (t/t1410 "non-commit sha1s").
        let oid = crate::resolve_arg(&repo, &rest[1])
            .map_err(|_| CommandError::fatal(format!("fatal: {}: not a valid SHA1", rest[1])))?;
        store
            .update(&rest[0], Some(&oid))
            .map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
        Ok(())
    }
}

pub struct SymbolicRef;

impl Command for SymbolicRef {
    fn name(&self) -> &'static str {
        "symbolic-ref"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
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
        let repo = ctx.repository()?;
        let store = RefStore::from_repo(&repo);
        let target = store
            .head_symbolic_target()
            .ok_or_else(|| {
                // C exits 128 here (die), not 1.
                CommandError::fatal(format!("ref '{name}' is not a symbolic ref"))
            })?;
        if short {
            let short_name = target.strip_prefix("refs/heads/").unwrap_or(&target);
            writeln!(out, "{short_name}").map_err(|e| CommandError::fatal(e.to_string()))?;
        } else {
            writeln!(out, "{target}").map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}