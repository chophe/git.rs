//! `git diff-tree`: compare the trees of two objects.

use std::io::Write;

use crate::patch;
use crate::{Command, CommandError, RepoContext};
use git_hash::Oid;
use git_odb::Odb;

pub struct DiffTree;

impl Command for DiffTree {
    fn name(&self) -> &'static str {
        "diff-tree"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut patch_mode = false;
        let mut recursive = false;
        let mut name_status = false;
        let mut name_only = false;
        let mut exit_code = false;
        let mut quiet = false;
        let mut trees: Vec<String> = Vec::new();

        for a in args {
            match a.as_str() {
                "-p" | "--patch" => patch_mode = true,
                "-r" => recursive = true,
                "--name-status" => name_status = true,
                "--name-only" => name_only = true,
                "--exit-code" => exit_code = true,
                "--quiet" => {
                    quiet = true;
                    exit_code = true;
                }
                "--root" | "-t" => {}
                "--no-commit-id" => {}
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("diff-tree: option '{s}' not supported")));
                }
                s => trees.push(s.to_string()),
            }
        }
        if trees.len() != 2 {
            return Err(CommandError::usage("diff-tree: requires two <tree-ish> arguments"));
        }

        let repo = ctx.repository()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;

        let t1 = crate::diff::resolve_tree(&repo, &odb, &trees[0])?;
        let t2 = crate::diff::resolve_tree(&repo, &odb, &trees[1])?;
        let obj1 = odb.read(&t1).map_err(|e| CommandError::error(e.to_string()))?;
        let obj2 = odb.read(&t2).map_err(|e| CommandError::error(e.to_string()))?;
        let e1 = git_object::parse_tree(&obj1.data, repo.hash_algo)
            .map_err(|e| CommandError::error(e.to_string()))?;
        let e2 = git_object::parse_tree(&obj2.data, repo.hash_algo)
            .map_err(|e| CommandError::error(e.to_string()))?;

        let recurse = patch_mode || recursive;
        let mut loader = |oid: &Oid| odb.read(oid).ok();
        let changes = git_diff::compare_trees(&e1, &e2, "", recurse, &mut loader);
        let has_changes = !changes.is_empty();

        if !quiet {
            for c in &changes {
                if patch_mode {
                    let p = patch::render_change_patch(c, &odb)?;
                    out.write_all(&p).map_err(|e| CommandError::fatal(e.to_string()))?;
                } else if name_only {
                    writeln!(out, "{}", c.path).map_err(|e| CommandError::fatal(e.to_string()))?;
                } else if name_status {
                    writeln!(out, "{}\t{}", c.status, c.path)
                        .map_err(|e| CommandError::fatal(e.to_string()))?;
                } else {
                    writeln!(
                        out,
                        ":{} {} {} {} {}\t{}",
                        patch::mode6(&c.old_mode),
                        patch::mode6(&c.new_mode),
                        patch::full_hex(&c.old_oid),
                        patch::full_hex(&c.new_oid),
                        c.status,
                        c.path
                    )
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
                }
            }
        }
        if exit_code && has_changes {
            return Err(CommandError::silent(1));
        }
        Ok(())
    }
}
