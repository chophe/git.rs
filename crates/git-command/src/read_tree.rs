//! `git read-tree`: read tree objects into the index.
//!
//! Port of `builtin/read-tree.c` for the one-way case (replace the index with
//! the contents of a tree, `--empty`, `--reset`, `-n`/`--dry-run`,
//! `--index-output=<file>`, `-v`). The merge (`-m`) and worktree update
//! (`-u`) paths depend on `unpack-trees` and are not implemented yet (they
//! report a clear "not supported" error rather than silently misbehaving).

use std::io::Write;
use std::path::PathBuf;

use crate::{Command, CommandError, RepoContext};
use git_hash::{HashAlgorithm, Oid};
use git_index::{Index, IndexEntry};
use git_object::{parse_tree, ObjectKind};
use git_odb::Odb;

pub struct ReadTree;

impl Command for ReadTree {
    fn name(&self) -> &'static str {
        "read-tree"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], _out: &mut dyn Write) -> Result<(), CommandError> {
        let mut empty = false;
        let mut dry_run = false;
        let mut index_output: Option<String> = None;
        let mut trees: Vec<String> = Vec::new();
        let mut after_dashdash = false;

        let mut i = 0usize;
        while i < args.len() {
            let a = &args[i];
            if after_dashdash {
                trees.push(a.clone());
                i += 1;
                continue;
            }
            match a.as_str() {
                "--empty" => empty = true,
                "-n" | "--dry-run" => dry_run = true,
                "-v" | "--verbose" => {}
                "-i" => {}
                "--reset" | "--reset=" => {}
                "--trivial-merge" | "--aggressive" | "--no-sparse-checkout" | "--exclude-per-directory" => {}
                "--" => after_dashdash = true,
                "-m" | "--reset-u" | "-u" | "--update" => {
                    return Err(CommandError::usage(
                        "read-tree: option not supported yet (unpack-trees)".to_string(),
                    ));
                }
                s if s.starts_with("--index-output=") => {
                    index_output = Some(s["--index-output=".len()..].to_string());
                }
                "--index-output" => {
                    i += 1;
                    let v = args.get(i).ok_or_else(|| {
                        CommandError::usage("option `index-output' requires a value")
                    })?;
                    index_output = Some(v.clone());
                }
                s if s.starts_with("--prefix=") => {
                    return Err(CommandError::usage(
                        "read-tree: --prefix is not supported yet".to_string(),
                    ));
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!(
                        "error: unknown option `{s}'\nusage: git read-tree [(-m [--trivial] [--aggressive] | --reset | --prefix=<prefix>)] [-u | -i] [--index-output=<file>] [-n] <tree-ish1> [<tree-ish2> [<tree-ish3>]]"
                    )));
                }
                s => trees.push(s.to_string()),
            }
            i += 1;
        }

        let repo = ctx.repository()?;
        let algo = repo.hash_algo;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;

        if trees.is_empty() && !empty {
            eprintln!("warning: read-tree: emptying the index with no arguments is deprecated; use --empty");
        }

        let mut index = if empty || trees.is_empty() {
            Index { version: 2, entries: Vec::new(), cache_tree: None }
        } else {
            let mut entries = Vec::new();
            for t in &trees {
                let oid = crate::resolve_arg(&repo, t)?;
                // A missing object or a non-tree is "failed to unpack", like C.
                match odb.read(&oid) {
                    Ok(o) if o.kind == ObjectKind::Tree => {}
                    _ => {
                        return Err(CommandError::fatal(format!(
                            "fatal: failed to unpack tree object {oid}"
                        )));
                    }
                }
                flatten_tree(&odb, &oid, "", &mut entries, algo, &mut Vec::new());
            }
            Index { version: 2, entries, cache_tree: None }
        };
        // The index requires plain-byte path order.
        index.entries.sort_by(|a, b| a.name.cmp(&b.name));

        // Prime the cache-tree from the resulting index (C reuses the input
        // tree oids; recomputing from the canonical trees is equivalent).
        let triples: Vec<(u32, Oid, String)> = index
            .entries
            .iter()
            .map(|e| (e.mode, e.oid, e.name.clone()))
            .collect();
        index.cache_tree = Some(
            crate::treeobj::cache_tree_from_entries(&triples, algo).map_err(CommandError::from)?,
        );

        if dry_run {
            return Ok(());
        }

        let target = match index_output {
            Some(p) => PathBuf::from(p),
            None => repo.index_file(),
        };
        index.write(&target, algo).map_err(|e| CommandError::fatal(e.to_string()))?;
        Ok(())
    }
}

/// Recursively flatten a tree into stage-0 index entries with zeroed stat data.
fn flatten_tree(
    odb: &Odb,
    oid: &Oid,
    prefix: &str,
    out: &mut Vec<IndexEntry>,
    algo: HashAlgorithm,
    seen: &mut Vec<Oid>,
) {
    // Cycle guard (defensive; trees cannot actually cycle).
    if seen.contains(oid) {
        return;
    }
    let obj = match odb.read(oid) {
        Ok(o) => o,
        Err(_) => return,
    };
    if obj.kind != ObjectKind::Tree {
        return;
    }
    let entries = match parse_tree(&obj.data, algo) {
        Ok(e) => e,
        Err(_) => return,
    };
    for e in entries {
        let name = format!("{prefix}{}", String::from_utf8_lossy(&e.name));
        if e.is_dir() {
            seen.push(*oid);
            flatten_tree(odb, &e.oid, &format!("{name}/"), out, algo, seen);
            seen.pop();
        } else {
            let mode = u32::from_str_radix(&e.mode, 8).unwrap_or(0o100644);
            out.push(IndexEntry::bare(e.oid, mode, name));
        }
    }
}
