//! `git rev-parse`: resolve refs and query repository metadata.

use std::io::Write;

use crate::{Command, CommandError, RepoContext};
use git_hash::Oid;
use git_refs::RefStore;

pub struct RevParse;

impl Command for RevParse {
    fn name(&self) -> &'static str {
        "rev-parse"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let repo = ctx.repository()?;
        let store = RefStore::from_repo(&repo);
        let algo = repo.hash_algo;

        let mut verify = false;
        let mut short = false;
        let mut abbrev_ref = false;
        let mut rest: Vec<String> = Vec::new();
        for a in args {
            match a.as_str() {
                "--verify" => verify = true,
                "--short" => short = true,
                "--abbrev-ref" => abbrev_ref = true,
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
            // --abbrev-ref: print the ref name the argument resolves through.
            if abbrev_ref {
                if arg == "HEAD" {
                    match store.head_symbolic_target() {
                        Some(t) => {
                            let name = t.strip_prefix("refs/heads/").unwrap_or(&t);
                            writeln!(out, "{name}").map_err(|e| CommandError::fatal(e.to_string()))?;
                        }
                        None => {
                            return Err(CommandError::error("fatal: HEAD is not a symbolic ref"));
                        }
                    }
                } else {
                    writeln!(out, "{arg}").map_err(|e| CommandError::fatal(e.to_string()))?;
                }
                continue;
            }

            let resolved = Oid::from_hex(arg, algo).ok().or_else(|| store.resolve(arg));
            let oid = match resolved {
                Some(oid) => oid,
                None => {
                    if verify {
                        return Err(CommandError::error("fatal: Needed a single revision"));
                    }
                    return Err(CommandError::error(format!("{arg}: unknown revision")));
                }
            };
            if short {
                // Abbreviate to 7 hex chars (git's default length).
                writeln!(out, "{}", &oid.to_string()[..7])
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
            } else {
                writeln!(out, "{oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }
        Ok(())
    }
}