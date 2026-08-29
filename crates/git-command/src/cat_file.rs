//! `git cat-file`: show object types, sizes, and contents.

use std::io::Write;

use crate::{Command, CommandError, RepoContext};
use git_core::Repository;
use git_hash::Oid;
use git_object::{parse_tree, Object, ObjectKind};
use git_odb::Odb;

pub struct CatFile;

impl Command for CatFile {
    fn name(&self) -> &'static str {
        "cat-file"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut action: Option<char> = None;
        let mut batch: Option<bool> = None; // Some(true) = --batch, Some(false) = --batch-check
        let mut batch_format: Option<String> = None;
        let mut batch_all_objects = false;
        let mut nul_terminated = false;
        let mut _buffered = false;
        let mut rest: Vec<String> = Vec::new();
        for a in args {
            match a.as_str() {
                "-t" | "-s" | "-p" | "-e" => action = Some(a.as_bytes()[1] as char),
                "--batch" => batch = Some(true),
                "--batch-check" => batch = Some(false),
                s if s.starts_with("--batch=") => {
                    batch = Some(true);
                    batch_format = Some(s["--batch=".len()..].to_string());
                }
                s if s.starts_with("--batch-check=") => {
                    batch = Some(false);
                    batch_format = Some(s["--batch-check=".len()..].to_string());
                }
                "--batch-all-objects" => batch_all_objects = true,
                "-z" => nul_terminated = true,
                "--buffer" | "--allow-unknown-type" => _buffered = true,
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("cat-file: unknown option '{s}'")));
                }
                s => rest.push(s.to_string()),
            }
        }
        let repo = ctx.repository()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let algo = repo.hash_algo;

        // Batch modes read object names from stdin (or iterate all objects).
        if let Some(with_contents) = batch {
            let format = batch_format.unwrap_or_else(|| {
                "%(objectname) %(objecttype) %(objectsize)".to_string()
            });
            if !batch_all_objects {
                use std::io::Read;
                let mut input = Vec::new();
                std::io::stdin()
                    .read_to_end(&mut input)
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
                let sep = if nul_terminated { b'\0' } else { b'\n' };
                let mut records: Vec<&[u8]> = input.split(|&b| b == sep).collect();
                if let Some(last) = records.last() {
                    if last.is_empty() {
                        records.pop();
                    }
                }
                for record in records {
                    let (name, rest_str) = split_record(record, &format);
                    emit_batch(out, &odb, &repo, &name, rest_str, with_contents, &format, algo)?;
                }
            } else {
                let mut oids = odb.loose.iter_oids();
                for (_pf, idx) in &odb.packs {
                    oids.extend(idx.oids().iter().cloned());
                }
                oids.sort();
                oids.dedup();
                for oid in &oids {
                    emit_batch(out, &odb, &repo, &oid.to_string(), None, with_contents, &format, algo)?;
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


/// Split a batch input record into the object name and the `%(rest)` part.
/// The rest is only split off when the format actually uses `%(rest)`.
fn split_record<'a>(record: &'a [u8], format: &str) -> (String, Option<String>) {
    let record = if record.ends_with(b"\r") {
        &record[..record.len() - 1]
    } else {
        record
    };
    if !format.contains("%(rest)") {
        return (String::from_utf8_lossy(record).into_owned(), None);
    }
    match record.iter().position(|&b| b == b' ') {
        Some(i) => (
            String::from_utf8_lossy(&record[..i]).into_owned(),
            Some(String::from_utf8_lossy(&record[i + 1..]).into_owned()),
        ),
        None => (String::from_utf8_lossy(record).into_owned(), None),
    }
}

/// Emit one batch record in the given format.
fn emit_batch(
    out: &mut dyn Write,
    odb: &Odb,
    repo: &Repository,
    name: &str,
    rest_str: Option<String>,
    with_contents: bool,
    format: &str,
    algo: git_hash::HashAlgorithm,
) -> Result<(), CommandError> {
    let resolved = Oid::from_hex(name, algo)
        .ok()
        .or_else(|| crate::resolve_arg(repo, name).ok());
    match resolved.and_then(|oid| odb.read(&oid).ok().map(|obj| (oid, obj))) {
        Some((oid, obj)) => {
            let mut line = String::new();
            expand_format(format, name, rest_str, Some((oid, &obj, odb, algo)), &mut line);
            write!(out, "{line}
").map_err(|e| CommandError::fatal(e.to_string()))?;
            if with_contents {
                out.write_all(&obj.data).map_err(|e| CommandError::fatal(e.to_string()))?;
                writeln!(out).map_err(|e| CommandError::fatal(e.to_string()))?;
            }
            Ok(())
        }
        None => {
            writeln!(out, "{name} missing").map_err(|e| CommandError::fatal(e.to_string()))
        }
    }
}

/// Expand the `%(atom)` format used by batch modes.
fn expand_format(
    format: &str,
    name: &str,
    rest_str: Option<String>,
    obj_info: Option<(Oid, &Object, &Odb, git_hash::HashAlgorithm)>,
    out: &mut String,
) {
    let _ = name;
    let mut rest_fmt = format;
    while let Some(i) = rest_fmt.find("%(") {
        out.push_str(&rest_fmt[..i]);
        let tail = &rest_fmt[i + 2..];
        let Some(end) = tail.find(')') else {
            out.push_str(&rest_fmt[i..]);
            return;
        };
        let atom = &tail[..end];
        match atom {
            "rest" => match &rest_str {
                Some(r) => out.push_str(r),
                None => {}
            },
            _ => match obj_info {
                Some((oid, obj, odb, algo)) => expand_object_atom(atom, oid, obj, odb, algo, out),
                None => out.push_str(&format!("%({atom})")),
            },
        }
        rest_fmt = &tail[end + 1..];
    }
    out.push_str(rest_fmt);
}

fn expand_object_atom(
    atom: &str,
    oid: Oid,
    obj: &Object,
    odb: &Odb,
    algo: git_hash::HashAlgorithm,
    out: &mut String,
) {
    match atom {
        "objectname" => out.push_str(&oid.to_string()),
        "objecttype" => out.push_str(obj.kind.as_str()),
        "objectsize" => out.push_str(&obj.data.len().to_string()),
        "objectsize:disk" => {
            let (disk, _) = odb.disk_info(&oid).unwrap_or((0, None));
            out.push_str(&disk.to_string());
        }
        "deltabase" => {
            let zero = Oid::new(algo, &vec![0u8; algo.hex_len() / 2]);
            let base = odb
                .disk_info(&oid)
                .and_then(|(_, b)| b)
                .unwrap_or(zero);
            out.push_str(&base.to_string());
        }
        other => {
            // Unknown atoms are a fatal error in C git; report via the
            // out-of-band error string convention (prefixed message).
            panic_prelude(other);
        }
    }
}

fn panic_prelude(atom: &str) -> ! {
    eprintln!("fatal: unknown format element: {atom}");
    std::process::exit(128);
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
