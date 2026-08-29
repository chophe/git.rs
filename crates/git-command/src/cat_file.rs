//! `git cat-file`: show object types, sizes, and contents.

use std::io::Write;

use crate::{Command, CommandError};
use git_core::Repository;
use git_hash::Oid;
use git_object::{parse_tree, Object, ObjectKind};
use git_odb::Odb;

pub struct CatFile;

impl Command for CatFile {
    fn name(&self) -> &'static str {
        "cat-file"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut action: Option<char> = None;
        let mut batch: Option<bool> = None; // Some(true) = --batch, Some(false) = --batch-check
        let mut rest: Vec<String> = Vec::new();
        for a in args {
            match a.as_str() {
                "-t" | "-s" | "-p" | "-e" => action = Some(a.as_bytes()[1] as char),
                "--batch" => batch = Some(true),
                "--batch-check" => batch = Some(false),
                "--batch-all-objects" => {
                    return Err(CommandError::usage("cat-file: --batch-all-objects not supported"));
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("cat-file: unknown option '{s}'")));
                }
                s => rest.push(s.to_string()),
            }
        }
        let repo = Repository::discover()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let algo = repo.hash_algo;

        // Batch modes read object names from stdin, one per line.
        if let Some(with_contents) = batch {
            use std::io::BufRead;
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                let name = line.map_err(|e| CommandError::fatal(e.to_string()))?;
                let name = name.trim().to_string();
                if name.is_empty() {
                    continue;
                }
                let resolved = git_hash::Oid::from_hex(&name, algo).ok().or_else(|| crate::resolve_arg(&repo, &name).ok());
                match resolved.and_then(|oid| odb.read(&oid).ok().map(|obj| (oid, obj))) {
                    Some((oid, obj)) => {
                        // git echoes the resolved object name, not the input.
                        if with_contents {
                            writeln!(
                                out,
                                "{} {} {}",
                                oid,
                                obj.kind.as_str(),
                                obj.data.len()
                            )
                            .map_err(|e| CommandError::fatal(e.to_string()))?;
                            out.write_all(&obj.data).map_err(|e| CommandError::fatal(e.to_string()))?;
                            writeln!(out).map_err(|e| CommandError::fatal(e.to_string()))?;
                        } else {
                            writeln!(out, "{} {} {}", oid, obj.kind.as_str(), obj.data.len())
                                .map_err(|e| CommandError::fatal(e.to_string()))?;
                        }
                    }
                    None => {
                        writeln!(out, "{name} missing").map_err(|e| CommandError::fatal(e.to_string()))?;
                    }
                }
            }
            return Ok(());
        }

        let (action, oid_s, type_arg) = match action {
            Some(a) => {
                let oid_s = rest
                    .first()
                    .ok_or_else(|| CommandError::usage("cat-file: missing <object>"))?;
                (Some(a), oid_s.clone(), None)
            }
            None => {
                // `git cat-file <type> <object>`
                if rest.len() < 2 {
                    return Err(CommandError::usage("cat-file: usage: cat-file (-t|-s|-p|-e) <object> | <type> <object>"));
                }
                (None, rest[1].clone(), Some(rest[0].clone()))
            }
        };

        let oid = Oid::from_hex(&oid_s, algo)
            .map_err(|_| CommandError::error(format!("Not a valid object name '{oid_s}'")))?;
        let obj = odb
            .read(&oid)
            .map_err(|e| CommandError::error(format!("{oid_s}: {e}")))?;

        match action {
            Some('e') => Ok(()), // existence check
            Some('t') => {
                writeln!(out, "{}", obj.kind.as_str()).map_err(|e| CommandError::fatal(e.to_string()))
            }
            Some('s') => {
                writeln!(out, "{}", obj.data.len()).map_err(|e| CommandError::fatal(e.to_string()))
            }
            Some('p') => pretty_print(&obj, out, algo),
            None => {
                // Validate the requested type matches, then print raw content.
                if let Some(t) = &type_arg {
                    let kind = ObjectKind::from_str(t)
                        .ok_or_else(|| CommandError::error(format!("invalid object type '{t}'")))?;
                    if kind != obj.kind {
                        return Err(CommandError::error(format!(
                            "object '{oid_s}' is a {}, not a {}",
                            obj.kind.as_str(),
                            kind.as_str()
                        )));
                    }
                }
                out.write_all(&obj.data).map_err(|e| CommandError::fatal(e.to_string()))?;
                Ok(())
            }
            _ => unreachable!("cat-file action is always e/t/s/p"),
        }
    }
}

/// Print an object the way `git cat-file -p` does.
pub fn pretty_print(obj: &Object, out: &mut dyn Write, algo: git_hash::HashAlgorithm) -> Result<(), CommandError> {
    match obj.kind {
        ObjectKind::Blob | ObjectKind::Commit | ObjectKind::Tag => {
            out.write_all(&obj.data).map_err(|e| CommandError::fatal(e.to_string()))
        }
        ObjectKind::Tree => {
            let entries = parse_tree(&obj.data, algo)
                .map_err(|e| CommandError::error(e.to_string()))?;
            for e in &entries {
                writeln!(
                    out,
                    "{:0>6} {} {}\t{}",
                    e.mode,
                    e.type_name(),
                    e.oid,
                    String::from_utf8_lossy(&e.name)
                )
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            }
            Ok(())
        }
    }
}
