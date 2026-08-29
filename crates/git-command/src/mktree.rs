//! `git mktree`: build a tree object from stdin entries.

use std::io::{BufRead, Write};

use crate::{Command, CommandError, RepoContext};
use git_hash::Oid;
use git_object::{serialize_tree, Object, ObjectKind, TreeEntry};
use git_odb::LooseStore;

pub struct MkTree;

impl Command for MkTree {
    fn name(&self) -> &'static str {
        "mktree"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        for a in args {
            if a != "--missing" && !a.is_empty() {
                return Err(CommandError::usage(format!("mktree: unknown option '{a}'")));
            }
        }

        let repo = ctx.repository()?;
        let store = LooseStore::from_repo(&repo);
        let algo = repo.hash_algo;

        let stdin = std::io::stdin();
        let mut entries: Vec<TreeEntry> = Vec::new();
        for line in stdin.lock().lines() {
            let line = line.map_err(|e| CommandError::fatal(e.to_string()))?;
            if line.is_empty() {
                continue;
            }
            // Format: <mode> <type> <oid>\t<name>
            let mut parts = line.splitn(3, ' ');
            let mode = parts.next().unwrap_or("").to_string();
            let type_s = parts.next().unwrap_or("");
            let rest = parts.next().unwrap_or("");
            let (oid_s, name) = rest
                .split_once('\t')
                .ok_or_else(|| CommandError::error(format!("malformed mktree line '{line}'")))?;
            if mode.is_empty() || !mode.bytes().all(|b| b.is_ascii_digit()) {
                return Err(CommandError::error(format!("invalid mode in '{line}'")));
            }
            // Validate the type matches the mode.
            let kind = ObjectKind::from_str(type_s)
                .ok_or_else(|| CommandError::error(format!("invalid type '{type_s}'")))?;
            if mode == "40000" && kind != ObjectKind::Tree {
                return Err(CommandError::error("tree entry mode does not match type"));
            }
            let oid = Oid::from_hex(oid_s, algo)
                .map_err(|_| CommandError::error(format!("invalid oid '{oid_s}'")))?;
            entries.push(TreeEntry {
                mode,
                name: name.as_bytes().to_vec(),
                oid,
            });
        }

        let data = serialize_tree(&entries, algo).map_err(|e| CommandError::error(e.to_string()))?;
        let obj = Object::from_data(ObjectKind::Tree, data);
        let oid = store.write(&obj).map_err(CommandError::from)?;
        writeln!(out, "{oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
        Ok(())
    }
}
