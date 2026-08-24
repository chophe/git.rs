//! `git status --porcelain`: report index/worktree state.

use std::collections::HashMap;
use std::io::Write;

use crate::{Command, CommandError};
use git_core::Repository;
use git_hash::{HashAlgorithm, Oid};
use git_index::Index;
use git_object::{parse_commit, parse_tree, Object, ObjectKind};
use git_odb::Odb;

pub struct Status;

impl Command for Status {
    fn name(&self) -> &'static str {
        "status"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut porcelain = false;
        for a in args {
            match a.as_str() {
                "--porcelain" | "--porcelain=v1" => porcelain = true,
                "-z" => {}
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("status: option '{s}' not supported")));
                }
                _ => {}
            }
        }
        if !porcelain {
            return Err(CommandError::usage("status: only --porcelain is supported"));
        }

        let repo = Repository::discover()?;
        let algo = repo.hash_algo;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let work_tree = repo
            .work_tree
            .as_ref()
            .ok_or_else(|| CommandError::error("this operation must be run in a work tree"))?;
        let index = Index::read(&repo.index_file(), algo)
            .unwrap_or(Index { version: 2, entries: vec![] });

        // Base tree from HEAD (or empty when there is no commit yet).
        let mut base: HashMap<String, Oid> = HashMap::new();
        if let Some(head) = repo.resolve_head() {
            if let Ok(commit_obj) = odb.read(&head) {
                if commit_obj.kind == ObjectKind::Commit {
                    if let Ok(commit) = parse_commit(&commit_obj.data, algo) {
                        flatten_tree(&odb, commit.tree, "", &mut base);
                    }
                }
            }
        }
        let index_entries: HashMap<&str, &git_index::IndexEntry> =
            index.entries.iter().map(|e| (e.name.as_str(), e)).collect();

        // Column X: index vs HEAD; column Y: worktree vs index.
        for e in &index.entries {
            let x = match base.get(&e.name) {
                Some(b) if *b == e.oid => ' ',
                Some(_) => 'M',
                None => 'A',
            };
            let y = match std::fs::read(work_tree.join(&e.name)) {
                Err(_) => 'D',
                Ok(data) => {
                    let blob = Object::from_data(ObjectKind::Blob, data.clone());
                    let oid = blob.compute_id(algo);
                    if oid == e.oid {
                        ' '
                    } else {
                        'M'
                    }
                }
            };
            if x != ' ' || y != ' ' {
                writeln!(out, "{x}{y} {}", e.name).map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }

        // Untracked files.
        for path in untracked(work_tree, &index_entries, algo) {
            writeln!(out, "?? {path}").map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}

fn flatten_tree(odb: &Odb, tree: Oid, prefix: &str, map: &mut HashMap<String, Oid>) {
    if let Ok(obj) = odb.read(&tree) {
        if let Ok(entries) = parse_tree(&obj.data, tree.algorithm()) {
            for e in &entries {
                let path = if prefix.is_empty() {
                    String::from_utf8_lossy(&e.name).into_owned()
                } else {
                    format!("{prefix}/{}", String::from_utf8_lossy(&e.name))
                };
                if e.is_dir() {
                    flatten_tree(odb, e.oid, &path, map);
                } else {
                    map.insert(path, e.oid);
                }
            }
        }
    }
}

/// Walk the worktree for files not covered by the index.
fn untracked(
    work_tree: &std::path::Path,
    index: &HashMap<&str, &git_index::IndexEntry>,
    _algo: HashAlgorithm,
) -> Vec<String> {
    let mut out = Vec::new();
    walk(work_tree, work_tree, "", index, &mut out);
    out
}

fn walk(
    root: &std::path::Path,
    dir: &std::path::Path,
    prefix: &str,
    index: &HashMap<&str, &git_index::IndexEntry>,
    out: &mut Vec<String>,
) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if e.path().is_dir() {
            walk(root, &e.path(), &path, index, out);
        } else if !index.contains_key(path.as_str()) {
            out.push(path);
        }
    }
    let _ = root;
}