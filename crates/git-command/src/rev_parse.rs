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
                    let shown = repo.git_dir_specified.clone().unwrap_or_else(|| display_path(&repo.git_dir, false));
                    writeln!(out, "{}", shown.display())
                        .map_err(|e| CommandError::fatal(e.to_string()))?;
                }
                "--git-common-dir" => {
                    writeln!(out, "{}", display_path(&repo.common_dir, true).display())
                        .map_err(|e| CommandError::fatal(e.to_string()))?;
                }
                "--show-toplevel" => {
                    match &repo.work_tree {
                        Some(wt) => {
                            writeln!(out, "{}", wt.display())
                                .map_err(|e| CommandError::fatal(e.to_string()))?;
                        }
                        None => {
                            return Err(CommandError::fatal(
                                "this operation must be run in a work tree",
                            ));
                        }
                    }
                }
                "--is-inside-work-tree" => {
                    let inside = repo.work_tree.as_ref().is_some_and(|wt| repo.git_dir.starts_with(wt));
                    writeln!(out, "{}", if inside { "true" } else { "false" })
                        .map_err(|e| CommandError::fatal(e.to_string()))?;
                }
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

/// How C git renders `--git-dir`/`--git-common-dir`: relative to the current
/// directory when the path sits under the cwd, otherwise absolute.
/// `allow_dotdot` permits a relative result that escapes the cwd (matching
/// C git's `--git-common-dir` behavior from a subdirectory).
fn display_path(p: &std::path::Path, allow_dotdot: bool) -> std::path::PathBuf {
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(_) => return p.to_path_buf(),
    };
    if let Ok(rel) = p.strip_prefix(&cwd) {
        return rel.to_path_buf();
    }
    if allow_dotdot {
        if let Some(rel) = relative_from(p, &cwd) {
            return rel;
        }
    }
    p.to_path_buf()
}

/// A relative path from `cwd` to `p`, or `None` when they share no prefix.
fn relative_from(p: &std::path::Path, cwd: &std::path::Path) -> Option<std::path::PathBuf> {
    let pc: Vec<_> = p.components().collect();
    let cc: Vec<_> = cwd.components().collect();
    let mut i = 0;
    while i < pc.len() && i < cc.len() && pc[i] == cc[i] {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    let mut out = std::path::PathBuf::new();
    for _ in i..cc.len() {
        out.push("..");
    }
    for c in &pc[i..] {
        out.push(c);
    }
    Some(out)
}
