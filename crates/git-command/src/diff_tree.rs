//! `git diff-tree`: compare the trees of two objects.

use std::io::Write;

use crate::patch;
use crate::{Command, CommandError};
use git_core::Repository;
use git_hash::Oid;
use git_object::parse_tree;
use git_odb::Odb;

pub struct DiffTree;

impl Command for DiffTree {
    fn name(&self) -> &'static str {
        "diff-tree"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut patch_mode = false;
        let mut recursive = false;
        let mut name_status = false;
        let mut name_only = false;
        let mut trees: Vec<String> = Vec::new();

        for a in args {
            match a.as_str() {
                "-p" | "--patch" => patch_mode = true,
                "-r" => recursive = true,
                "--name-status" => name_status = true,
                "--name-only" => name_only = true,
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

        let repo = Repository::discover()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let algo = repo.hash_algo;

        let t1 = Oid::from_hex(&trees[0], algo)
            .map_err(|_| CommandError::error(format!("Not a valid object name '{}'", trees[0])))?;
        let t2 = Oid::from_hex(&trees[1], algo)
            .map_err(|_| CommandError::error(format!("Not a valid object name '{}'", trees[1])))?;
        let obj1 = odb.read(&t1).map_err(|e| CommandError::error(e.to_string()))?;
        let obj2 = odb.read(&t2).map_err(|e| CommandError::error(e.to_string()))?;
        let e1 = parse_tree(&obj1.data, algo).map_err(|e| CommandError::error(e.to_string()))?;
        let e2 = parse_tree(&obj2.data, algo).map_err(|e| CommandError::error(e.to_string()))?;

        let recurse = patch_mode || recursive;
        let mut loader = |oid: &Oid| odb.read(oid).ok();
        let changes = git_diff::compare_trees(&e1, &e2, "", recurse, &mut loader);

        for c in &changes {
            if patch_mode {
                let p = patch::render_change_patch(c, &odb)?;
                out.write_all(&p).map_err(|e| CommandError::fatal(e.to_string()))?;
            } else if name_only {
                writeln!(out, "{}", c.path).map_err(|e| CommandError::fatal(e.to_string()))?;
            } else if name_status {
                writeln!(out, "{}\t{}", c.status, c.path).map_err(|e| CommandError::fatal(e.to_string()))?;
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
        Ok(())
    }
}