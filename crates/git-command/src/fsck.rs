//! `git fsck`: verify the object database.

use std::collections::HashSet;
use std::io::Write;

use crate::{Command, CommandError};
use git_core::Repository;
use git_hash::Oid;
use git_object::{parse_commit, parse_tag, parse_tree};
use git_odb::Odb;
use git_refs::RefStore;

pub struct Fsck;

impl Command for Fsck {
    fn name(&self) -> &'static str {
        "fsck"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        for a in args {
            if !a.starts_with('-') {
                return Err(CommandError::usage(format!("fsck: unexpected argument '{a}'")));
            }
        }

        let repo = Repository::discover()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let store = RefStore::from_repo(&repo);
        let algo = repo.hash_algo;

        let mut errors = false;
        let mut reachable: HashSet<Oid> = HashSet::new();
        let mut queue: Vec<(Oid, &'static str)> = Vec::new();

        // Seed from all refs and HEAD.
        for (_name, oid) in store.list() {
            queue.push((oid, "commit"));
        }
        if let Some(h) = store.resolve("HEAD") {
            if !reachable.contains(&h) {
                queue.push((h, "commit"));
            }
        }

        while let Some((oid, typ)) = queue.pop() {
            if reachable.contains(&oid) {
                continue;
            }
            if !odb.contains(&oid) {
                writeln!(out, "missing {typ} {oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
                errors = true;
                continue;
            }
            let obj = match odb.read(&oid) {
                Ok(o) => o,
                Err(_) => {
                    writeln!(out, "error: bad object {oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
                    errors = true;
                    continue;
                }
            };
            reachable.insert(oid);
            match obj.kind {
                git_object::ObjectKind::Commit => match parse_commit(&obj.data, algo) {
                    Ok(c) => {
                        queue.push((c.tree, "tree"));
                        for p in c.parents {
                            queue.push((p, "commit"));
                        }
                    }
                    Err(_) => {
                        writeln!(out, "error: corrupt commit {oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
                        errors = true;
                    }
                },
                git_object::ObjectKind::Tree => match parse_tree(&obj.data, algo) {
                    Ok(entries) => {
                        for e in entries {
                            if e.mode == "160000" {
                                continue; // gitlink
                            }
                            let t = if e.is_dir() { "tree" } else { "blob" };
                            queue.push((e.oid, t));
                        }
                    }
                    Err(_) => {
                        writeln!(out, "error: corrupt tree {oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
                        errors = true;
                    }
                },
                git_object::ObjectKind::Tag => match parse_tag(&obj.data, algo) {
                    Ok(t) => queue.push((t.object, t.kind.as_str())),
                    Err(_) => {
                        writeln!(out, "error: corrupt tag {oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
                        errors = true;
                    }
                },
                git_object::ObjectKind::Blob => {}
            }
        }

        // Dangling: present but unreachable objects (sorted by type, then oid).
        let mut all: Vec<Oid> = odb.loose.iter_oids();
        for (_pf, idx) in &odb.packs {
            all.extend(idx.oids().iter().cloned());
        }
        let mut dangling: Vec<(String, Oid)> = Vec::new();
        for oid in all {
            if reachable.contains(&oid) {
                continue;
            }
            if let Ok(obj) = odb.read(&oid) {
                dangling.push((obj.kind.as_str().to_string(), oid));
            } else {
                dangling.push(("unknown".to_string(), oid));
            }
        }
        dangling.sort();
        for (t, oid) in &dangling {
            writeln!(out, "dangling {t} {oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
        }

        if errors {
            // git fsck exits 2 when it finds missing/corrupt objects.
            Err(CommandError::silent(2))
        } else {
            Ok(())
        }
    }
}