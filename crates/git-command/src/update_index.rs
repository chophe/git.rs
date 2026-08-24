//! `git update-index`: add/remove worktree files in the index.

use std::io::Write;
use std::os::unix::fs::MetadataExt;

use crate::{Command, CommandError};
use git_core::Repository;
use git_index::{Index, IndexEntry};
use git_object::{Object, ObjectKind};
use git_odb::LooseStore;

pub struct UpdateIndex;

impl Command for UpdateIndex {
    fn name(&self) -> &'static str {
        "update-index"
    }

    fn run(&self, args: &[String], _out: &mut dyn Write) -> Result<(), CommandError> {
        let mut add = false;
        let mut remove = false;
        let mut paths: Vec<String> = Vec::new();
        let mut after_dashdash = false;
        for a in args {
            if after_dashdash {
                paths.push(a.clone());
                continue;
            }
            match a.as_str() {
                "--add" => add = true,
                "--remove" => remove = true,
                "--" => after_dashdash = true,
                "--refresh" | "-q" | "--again" => {}
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("update-index: option '{s}' not supported")));
                }
                s => paths.push(s.to_string()),
            }
        }
        if paths.is_empty() {
            return Err(CommandError::usage("update-index: no paths given"));
        }

        let repo = Repository::discover()?;
        let algo = repo.hash_algo;
        let work_tree = repo
            .work_tree
            .as_ref()
            .ok_or_else(|| CommandError::error("this operation must be run in a work tree"))?;
        let store = LooseStore::from_repo(&repo);

        let mut index = Index::read(&repo.index_file(), algo)
            .unwrap_or(Index { version: 2, entries: vec![] });

        for path in &paths {
            let full = work_tree.join(path);
            if full.is_file() {
                if !add && !index.entries.iter().any(|e| e.name == *path) {
                    return Err(CommandError::error(format!(
                        "fatal: Unable to add '{path}' to index (not in index and --add not given)"
                    )));
                }
                let data = std::fs::read(&full).map_err(|e| CommandError::error(e.to_string()))?;
                let obj = Object::from_data(ObjectKind::Blob, data);
                let oid = store.write(&obj).map_err(CommandError::from)?;
                let meta = std::fs::metadata(&full).map_err(|e| CommandError::error(e.to_string()))?;
                let entry = IndexEntry {
                    ctime_sec: meta.ctime() as u32,
                    ctime_nsec: meta.ctime_nsec() as u32,
                    mtime_sec: meta.mtime() as u32,
                    mtime_nsec: meta.mtime_nsec() as u32,
                    dev: meta.dev() as u32,
                    ino: meta.ino() as u32,
                    mode: meta.mode() as u32,
                    uid: meta.uid(),
                    gid: meta.gid(),
                    size: meta.size() as u32,
                    oid,
                    assume_valid: false,
                    stage: 0,
                    name: path.clone(),
                };
                index.entries.retain(|e| e.name != *path);
                index.entries.push(entry);
            } else if remove {
                index.entries.retain(|e| e.name != *path);
            } else {
                return Err(CommandError::error(format!("fatal: unable to stat '{path}'")));
            }
        }

        // Keep entries sorted by path (git requires a sorted index).
        index.entries.sort_by(|a, b| a.name.cmp(&b.name));
        index.write(&repo.index_file(), algo).map_err(|e| CommandError::fatal(e.to_string()))?;
        Ok(())
    }
}