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
        let mut error_unmatch = false;
        let mut format: Option<String> = None;
        let mut operands: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < args.len() {
            let a = &args[i];
            match a.as_str() {
                "--stage" => stage = true,
                "--error-unmatch" => error_unmatch = true,
                "--cached" | "--debug" => {}
                "--format" => {
                    i += 1;
                    format = Some(
                        args.get(i)
                            .ok_or_else(|| {
                                CommandError::usage("error: option `format' requires a value")
                            })?
                            .clone(),
                    );
                }
                s if s.starts_with("--format=") => {
                    format = Some(s["--format=".len()..].to_string())
                }
                "--" => continue,
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("ls-files: option '{s}' not supported")));
                }
                s => operands.push(s.to_string()),
            }
            i += 1;
        }
        let repo = ctx.repository()?;
        let algo = repo.hash_algo;
        let index = match Index::read(&repo.index_file(), algo) {
            Ok(i) => i,
            Err(_) => Index { version: 2, entries: vec![], cache_tree: None },
        };

        // Pathspec filter (literal or directory prefix, like elsewhere).
        let specs: Vec<String> = operands
            .iter()
            .map(|o| crate::checkout_core::resolve_path_arg(ctx, &repo, o))
            .collect();
        if error_unmatch {
            for (orig, spec) in operands.iter().zip(specs.iter()) {
                let hit = index.entries.iter().any(|e| {
                    crate::checkout_core::spec_matches_glob(spec, &e.name)
                });
                if !hit {
                    eprintln!("error: pathspec '{orig}' did not match any file(s) known to git");
                    eprintln!("Did you forget to 'git add'?");
                    return Err(CommandError::silent(1));
                }
            }
        }
        let show = |name: &str| {
            specs.is_empty()
                || specs.iter().any(|s| crate::checkout_core::spec_matches_glob(s, name))
        };

        if let Some(fmt) = &format {
            // `--format` with %(objectmode)/%(objectname)/%(stage) atoms
            // (the subset t7001/t3600 need for entry comparisons).
            for e in &index.entries {
                if !show(&e.name) {
                    continue;
                }
                writeln!(out, "{}", expand_format(fmt, e)?)
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
            }
            return Ok(());
        }
        if stage {
            // List every entry with `mode oid stage` columns.
            for e in &index.entries {
                if !show(&e.name) {
                    continue;
                }
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
                if e.stage == 0 && show(&e.name) {
                    writeln!(out, "{}", e.name).map_err(|e| CommandError::fatal(e.to_string()))?;
                }
            }
        }
        Ok(())
    }
}

/// Expand `%(atom)` placeholders for one index entry. Unknown atoms are
/// fatal, like C (`fatal: unknown format element: ...`, exit 128).
fn expand_format(fmt: &str, e: &git_index::IndexEntry) -> Result<String, CommandError> {
    let mut out = String::new();
    let mut rest = fmt;
    while let Some(i) = rest.find("%(") {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 2..];
        match tail.find(')') {
            Some(j) => {
                let atom = &tail[..j];
                match atom {
                    "objectmode" => out.push_str(&format!("{:06o}", e.mode)),
                    "objectname" => out.push_str(&e.oid.to_string()),
                    "stage" => out.push_str(&e.stage.to_string()),
                    _ => {
                        return Err(CommandError::fatal(format!(
                            "fatal: unknown format element: {atom}"
                        )));
                    }
                }
                rest = &tail[j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                break;
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}