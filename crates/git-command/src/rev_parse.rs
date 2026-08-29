//! `git rev-parse`: resolve refs and query repository metadata.

use std::io::Write;

use crate::{Command, CommandError, RepoContext};
use git_core::Repository;
use git_refs::RefStore;

pub struct RevParse;

impl Command for RevParse {
    fn name(&self) -> &'static str {
        "rev-parse"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let repo = ctx.repository()?;
        let _algo = repo.hash_algo;

        let mut verify = false;
        let mut quiet = false;
        let mut short_len: Option<usize> = None;
        let mut abbrev_ref = false;
        let mut symbolic = false;
        let mut symbolic_full = false;
        let mut sq = false;
        let mut sq_quote = false;
        let mut default_arg: Option<String> = None;
        let mut rest: Vec<String> = Vec::new();
        let mut after_dashdash = false;
        for a in args {
            if after_dashdash {
                rest.push(a.clone());
                continue;
            }
            match a.as_str() {
                "--" => after_dashdash = true,
                "--verify" => verify = true,
                "--quiet" | "-q" => quiet = true,
                "--short" => short_len = Some(7),
                s if s.starts_with("--short=") => {
                    short_len = Some(s["--short=".len()..].parse().unwrap_or(7));
                }
                "--abbrev-ref" => abbrev_ref = true,
                "--symbolic" => symbolic = true,
                "--symbolic-full-name" => symbolic_full = true,
                "--sq" => sq = true,
                "--sq-quote" => sq_quote = true,
                "--default" => {
                    // The next argument supplies the default revision.
                    return Err(CommandError::usage("rev-parse: --default requires a value"));
                }
                s if s.starts_with("--default=") => {
                    default_arg = Some(s["--default=".len()..].to_string());
                }
                "--git-dir" => {
                    let shown =
                        repo.git_dir_specified.clone().unwrap_or_else(|| display_path(&repo.git_dir, false));
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
                    let inside = repo
                        .work_tree
                        .as_ref()
                        .is_some_and(|wt| repo.git_dir.starts_with(wt));
                    writeln!(out, "{}", if inside { "true" } else { "false" })
                        .map_err(|e| CommandError::fatal(e.to_string()))?;
                }
                "--is-bare-repository" => {
                    writeln!(out, "{}", repo.bare || repo.work_tree.is_none())
                        .map_err(|e| CommandError::fatal(e.to_string()))?;
                }
                "--is-shallow-repository" => {
                    let shallow = repo.git_dir.join("shallow").exists();
                    writeln!(out, "{}", shallow)
                        .map_err(|e| CommandError::fatal(e.to_string()))?;
                }
                "--show-prefix" => {
                    let prefix = relative_from(&std::env::current_dir().unwrap_or_default(), &repo.work_tree.clone().unwrap_or_default())
                        .map(|p| {
                            let s = p.display().to_string();
                            if s.is_empty() { s } else { format!("{s}/") }
                        })
                        .unwrap_or_default();
                    writeln!(out, "{prefix}").map_err(|e| CommandError::fatal(e.to_string()))?;
                }
                "--show-cdup" => {
                    let cdup = relative_from(&std::env::current_dir().unwrap_or_default(), &repo.work_tree.clone().unwrap_or_default())
                        .map(|p| {
                            let depth = p.components().count();
                            "../".repeat(depth)
                        })
                        .unwrap_or_default();
                    writeln!(out, "{cdup}").map_err(|e| CommandError::fatal(e.to_string()))?;
                }
                "--absolute-git-dir" => {
                    writeln!(out, "{}", repo.git_dir.display())
                        .map_err(|e| CommandError::fatal(e.to_string()))?;
                }
                "--shared-index-path" | "--show-superproject-working-tree" => {
                    // No shared index / superproject support yet: C git
                    // prints nothing for these in plain repositories.
                }
                "--local-env-vars" => {
                    for v in LOCAL_REPO_ENV {
                        writeln!(out, "{v}").map_err(|e| CommandError::fatal(e.to_string()))?;
                    }
                }
                // Unrecognized options are echoed verbatim and ignored,
                // matching C git's rev-parse passthrough behavior.
                s if s.starts_with('-') && s.len() > 1 => {
                    writeln!(out, "{s}").map_err(|e| CommandError::fatal(e.to_string()))?;
                }
                s => rest.push(s.to_string()),
            }
        }

        if sq_quote {
            let mut line = String::new();
            for a in &rest {
                line.push(' ');
                sq_quote_arg(&mut line, a);
            }
            writeln!(out, "{line}").map_err(|e| CommandError::fatal(e.to_string()))?;
            return Ok(());
        }

        for arg in &rest {
            // Ranges: `A..B` prints B then ^A; `A...B` prints B, A, then
            // ^merge-base(A, B). An empty side defaults to HEAD.
            if !after_dashdash && !verify {
                if let Some((a, b, sym)) = split_range(arg) {
                    emit_range(out, &repo, a, b, sym, arg)?;
                    continue;
                }
            }

            // `--abbrev-ref` / `--symbolic-full-name`: report the ref path.
            if abbrev_ref || symbolic_full {
                match symbolic_ref_name(&repo, arg) {
                    Some(name) if symbolic_full || arg.as_str() != name => {
                        let shown = if abbrev_ref {
                            name.strip_prefix("refs/heads/").unwrap_or(&name).to_string()
                        } else {
                            name
                        };
                        writeln!(out, "{shown}")
                            .map_err(|e| CommandError::fatal(e.to_string()))?;
                    }
                    Some(name) => {
                        writeln!(out, "{name}").map_err(|e| CommandError::fatal(e.to_string()))?;
                    }
                    None if symbolic_full => {
                        if crate::resolve_arg(&repo, arg).is_err() {
                            if verify {
                                return Err(verify_error(quiet));
                            }
                            writeln!(out, "{arg}")
                                .map_err(|e| CommandError::fatal(e.to_string()))?;
                            return Err(CommandError::fatal(crate::revision_error_text(arg)));
                        }
                        // C git prints nothing for non-ref arguments here.
                    }
                    None => {
                        let oid = crate::resolve_arg(&repo, arg)?;
                        writeln!(out, "{oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
                    }
                }
                continue;
            }

            let mut resolved = crate::resolve_arg(&repo, arg);
            if resolved.is_err() {
                if let Some(def) = &default_arg {
                    resolved = crate::resolve_arg(&repo, def);
                }
            }
            let oid = match resolved {
                Ok(oid) => oid,
                Err(_) => {
                    if verify {
                        return Err(verify_error(quiet));
                    }
                    // C git echoes the unresolved argument to stdout before
                    // dying with the ambiguous-argument message.
                    writeln!(out, "{arg}").map_err(|e| CommandError::fatal(e.to_string()))?;
                    return Err(CommandError::fatal(crate::revision_error_text(arg)));
                }
            };

            if sq {
                let mut line = String::new();
                line.push(0x27 as char);
                line.push_str(&oid.to_string());
                line.push(0x27 as char);
                line.push(' ');
                write!(out, "{line}").map_err(|e| CommandError::fatal(e.to_string()))?;
            } else if symbolic {
                writeln!(out, "{arg}").map_err(|e| CommandError::fatal(e.to_string()))?;
            } else if abbrev_ref {
                let name = symbolic_ref_name(&repo, arg)
                    .unwrap_or_else(|| oid.to_string());
                let short_name = name.strip_prefix("refs/heads/").unwrap_or(&name);
                writeln!(out, "{short_name}").map_err(|e| CommandError::fatal(e.to_string()))?;
            } else {
                let len = short_len;
                let text = match len {
                    Some(n) if n >= 4 => oid.to_string()[..n.min(oid.to_string().len())].to_string(),
                    _ => oid.to_string(),
                };
                writeln!(out, "{text}").map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }
        Ok(())
    }
}


/// The env var names C git reports for `--local-env-vars`.
const LOCAL_REPO_ENV: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

fn verify_error(quiet: bool) -> CommandError {
    if quiet {
        CommandError::silent(1)
    } else {
        CommandError::fatal("fatal: Needed a single revision")
    }
}

/// Split `A..B` / `A...B`; returns `(a, b, symmetric)`.
fn split_range(arg: &str) -> Option<(&str, &str, bool)> {
    if let Some((a, b)) = arg.split_once("...") {
        return Some((a, b, true));
    }
    if let Some((a, b)) = arg.split_once("..") {
        if !arg.contains("...") {
            return Some((a, b, false));
        }
    }
    None
}

fn emit_range(
    out: &mut dyn Write,
    repo: &Repository,
    a: &str,
    b: &str,
    symmetric: bool,
    original: &str,
) -> Result<(), CommandError> {
    let fail = |arg: &str| CommandError::fatal(crate::revision_error_text(arg));
    let a = if a.is_empty() { "HEAD" } else { a };
    let b = if b.is_empty() { "HEAD" } else { b };
    let a_oid = crate::resolve_arg(repo, a).map_err(|_| fail(original))?;
    let b_oid = crate::resolve_arg(repo, b).map_err(|_| fail(original))?;
    writeln!(out, "{b_oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
    if symmetric {
        // `A...B` prints B, A, then ^merge-base(A, B).
        let base = merge_base(repo, &a_oid, &b_oid);
        writeln!(out, "{a_oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
        if let Some(base) = base {
            writeln!(out, "^{base}").map_err(|e| CommandError::fatal(e.to_string()))?;
        }
    } else {
        writeln!(out, "^{a_oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
    }
    Ok(())
}

/// Merge base by walking first parents (sufficient for the common case).
fn merge_base(repo: &Repository, a: &git_hash::Oid, b: &git_hash::Oid) -> Option<git_hash::Oid> {
    let odb = git_odb::Odb::from_repo(repo).ok()?;
    let ancestry = |start: &git_hash::Oid| -> Option<Vec<git_hash::Oid>> {
        let mut chain = Vec::new();
        let mut cur = start.clone();
        loop {
            let obj = odb.read(&cur).ok()?;
            if obj.kind != git_object::ObjectKind::Commit {
                return None;
            }
            chain.push(cur.clone());
            let commit = git_object::parse_commit(&obj.data, repo.hash_algo).ok()?;
            match commit.parents.first() {
                Some(p) => cur = p.clone(),
                None => break,
            }
        }
        Some(chain)
    };
    let a_chain = ancestry(a)?;
    let b_set: std::collections::HashSet<git_hash::Oid> = ancestry(b)?.into_iter().collect();
    a_chain.into_iter().find(|o| b_set.contains(o))
}

/// The full ref path an argument refers to, when it names a ref.
fn symbolic_ref_name(repo: &Repository, arg: &str) -> Option<String> {
    let store = RefStore::from_repo(repo);
    if arg == "HEAD" {
        return store.head_symbolic_target();
    }
    let candidates = [
        arg.to_string(),
        "refs/".to_string() + arg,
        "refs/tags/".to_string() + arg,
        "refs/heads/".to_string() + arg,
        "refs/remotes/".to_string() + arg,
    ];
    for c in &candidates {
        if store.resolve(c).is_some() {
            return Some(c.clone());
        }
    }
    None
}

/// Single-quote an argument the way `--sq-quote` does.
fn sq_quote_arg(out: &mut String, arg: &str) {
    out.push(0x27 as char);
    for c in arg.chars() {
        if c == (0x27 as char) {
            out.push_str("'\''");
        } else {
            out.push(c);
        }
    }
    out.push(0x27 as char);
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
