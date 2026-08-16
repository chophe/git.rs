//! `git ls-tree`: list the entries of a tree object.

use std::io::Write;

use crate::{Command, CommandError};
use git_core::Repository;
use git_hash::Oid;
use git_object::{parse_tree, TreeEntry};
use git_odb::Odb;

pub struct LsTree;

impl Command for LsTree {
    fn name(&self) -> &'static str {
        "ls-tree"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut recursive = false;
        let mut include_trees = false;
        let mut name_only = false;
        let mut tree_arg: Option<String> = None;

        for a in args {
            match a.as_str() {
                "-r" => recursive = true,
                "-t" => include_trees = true,
                "--name-only" => name_only = true,
                "-d" => {}
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("ls-tree: unknown option '{s}'")));
                }
                s => tree_arg = Some(s.to_string()),
            }
        }
        let tree_arg = tree_arg.ok_or_else(|| CommandError::usage("ls-tree: missing <tree-ish>"))?;

        let repo = Repository::discover()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let algo = repo.hash_algo;
        let oid = Oid::from_hex(&tree_arg, algo)
            .map_err(|_| CommandError::error(format!("Not a valid object name '{tree_arg}'")))?;
        let obj = odb.read(&oid).map_err(|e| CommandError::error(e.to_string()))?;
        let entries = parse_tree(&obj.data, algo).map_err(|e| CommandError::error(e.to_string()))?;

        print_entries(&odb, &entries, "", out, recursive, include_trees, name_only)?;
        Ok(())
    }
}

fn print_entries(
    odb: &Odb,
    entries: &[TreeEntry],
    prefix: &str,
    out: &mut dyn Write,
    recursive: bool,
    include_trees: bool,
    name_only: bool,
) -> Result<(), CommandError> {
    for e in entries {
        let path = if prefix.is_empty() {
            String::from_utf8_lossy(&e.name).into_owned()
        } else {
            format!("{prefix}{}", String::from_utf8_lossy(&e.name))
        };
        if name_only {
            writeln!(out, "{path}").map_err(|e| CommandError::fatal(e.to_string()))?;
        } else {
            writeln!(
                out,
                "{:06} {} {}\t{path}",
                e.mode,
                e.type_name(),
                e.oid
            )
            .map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        if recursive && e.is_dir() {
            if let Ok(sub) = odb.read(&e.oid) {
                if let Ok(sub_entries) = parse_tree(&sub.data, e.oid.algorithm()) {
                    let sub_prefix = format!("{path}/");
                    print_entries(
                        odb,
                        &sub_entries,
                        &sub_prefix,
                        out,
                        recursive,
                        include_trees,
                        name_only,
                    )?;
                }
            }
        }
        let _ = include_trees;
    }
    Ok(())
}
