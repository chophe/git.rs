//! `git write-tree`: create a tree object from the current index.
//!
//! Port of `builtin/write-tree.c` plus the non-cache-tree path of
//! `cache-tree.c`. On success the index's `TREE` extension is refreshed and
//! the index written back, like C. `--missing-ok` is accepted; it only matters
//! with a promisor remote, which the port does not model. `--prefix=<prefix>/`
//! writes the subtree at `<prefix>` (and, like a subtree extraction, does not
//! rewrite the whole-index cache-tree).

use std::collections::BTreeMap;
use std::io::Write;

use crate::treeobj::{build, insert_path, TreeNode};
use crate::{Command, CommandError, RepoContext};
use git_index::Index;
use git_odb::LooseStore;

pub struct WriteTree;

impl Command for WriteTree {
    fn name(&self) -> &'static str {
        "write-tree"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut prefix: Option<String> = None;
        let mut i = 0usize;
        while i < args.len() {
            let a = &args[i];
            match a.as_str() {
                "--missing-ok" | "--ignore-cache-tree" => {}
                s if s.starts_with("--prefix=") => {
                    prefix = Some(s["--prefix=".len()..].to_string());
                }
                "--prefix" => {
                    i += 1;
                    let v = args
                        .get(i)
                        .ok_or_else(|| CommandError::usage("option `prefix' requires a value"))?;
                    prefix = Some(v.clone());
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("usage: git write-tree [--missing-ok] [--prefix=<prefix>/]\nerror: unknown option `{s}'")));
                }
                s => {
                    return Err(CommandError::usage(format!(
                        "usage: git write-tree [--missing-ok] [--prefix=<prefix>/]\nerror: unknown option `{s}'"
                    )));
                }
            }
            i += 1;
        }

        let repo = ctx.repository()?;
        let algo = repo.hash_algo;
        let index_path = repo.index_file();
        let index = match Index::read(&index_path, algo) {
            Ok(ix) => ix,
            Err(git_index::IndexError::Io(_)) => Index { version: 2, entries: Vec::new(), cache_tree: None },
            Err(e) => {
                return Err(CommandError::fatal(format!("fatal: git-write-tree: error reading the index ({e})")));
            }
        };

        // Unmerged (stage > 0) entries make the index unwritable, like C.
        let mut unmerged = false;
        for e in &index.entries {
            if e.stage != 0 {
                unmerged = true;
                let _ = writeln!(std::io::stderr(), "{}: unmerged ({})", e.name, e.oid);
            }
        }
        if unmerged {
            return Err(CommandError::fatal("fatal: git-write-tree: error building trees"));
        }

        // Build the hierarchy (optionally restricted to a prefix).
        let mut root: BTreeMap<String, TreeNode> = BTreeMap::new();
        let mut matched_prefix = false;
        for e in &index.entries {
            let rel = match &prefix {
                Some(p) => {
                    let p = p.trim_end_matches('/');
                    match e.name.strip_prefix(&format!("{p}/")) {
                        Some(rest) if !rest.is_empty() => {
                            matched_prefix = true;
                            rest.to_string()
                        }
                        _ => continue,
                    }
                }
                None => e.name.clone(),
            };
            let comps: Vec<&str> = rel.split('/').collect();
            insert_path(&mut root, &comps, e.mode, e.oid);
        }
        if prefix.is_some() && !matched_prefix {
            return Err(CommandError::fatal(format!(
                "fatal: git-write-tree: prefix {} not found",
                prefix.as_deref().unwrap_or("")
            )));
        }

        let store = LooseStore::from_repo(&repo);
        let (oid, cache_tree) =
            build(&root, Vec::new(), algo, Some(&store)).map_err(CommandError::from)?;
        writeln!(out, "{oid}").map_err(|e| CommandError::fatal(e.to_string()))?;

        // Refresh the index's cache-tree and write it back (skip the prefix
        // case, where the cache-tree we built only covers the subtree).
        if prefix.is_none() {
            let mut index = index;
            index.cache_tree = Some(cache_tree);
            index
                .write(&index_path, algo)
                .map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}
