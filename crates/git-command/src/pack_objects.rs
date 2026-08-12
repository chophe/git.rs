//! `git pack-objects`: write a pack (and index) from object ids read on stdin.

use std::io::{BufRead, Write};

use crate::{Command, CommandError};
use git_core::Repository;
use git_hash::Oid;
use git_odb::pack::{write_pack, PackObject};
use git_odb::Odb;

pub struct PackObjects;

impl Command for PackObjects {
    fn name(&self) -> &'static str {
        "pack-objects"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut to_stdout = false;
        let mut base_name: Option<String> = None;
        for a in args {
            match a.as_str() {
                "--stdout" => to_stdout = true,
                "-q" => {}
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("pack-objects: unknown option '{s}'")));
                }
                b => base_name = Some(b.to_string()),
            }
        }
        if to_stdout && base_name.is_some() {
            return Err(CommandError::usage("pack-objects: --stdout and <base-name> are mutually exclusive"));
        }

        let repo = Repository::discover()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let algo = repo.hash_algo;

        let stdin = std::io::stdin();
        let mut handle = stdin.lock();
        let mut line = String::new();
        let mut oids: Vec<Oid> = Vec::new();
        loop {
            line.clear();
            if handle
                .read_line(&mut line)
                .map_err(|e| CommandError::fatal(e.to_string()))?
                == 0
            {
                break;
            }
            let oid_s = line.trim();
            if oid_s.is_empty() {
                continue;
            }
            let oid = Oid::from_hex(oid_s, algo)
                .map_err(|_| CommandError::error(format!("not a valid object name: '{oid_s}'")))?;
            oids.push(oid);
        }

        let mut pack_objs = Vec::with_capacity(oids.len());
        for oid in &oids {
            let obj = odb
                .read(oid)
                .map_err(|e| CommandError::error(format!("{oid}: {e}")))?;
            pack_objs.push(PackObject {
                oid: *oid,
                kind: obj.kind,
                data: obj.data,
            });
        }

        let (pack, idx) = write_pack(&pack_objs, algo).map_err(CommandError::from)?;

        if to_stdout {
            out.write_all(&pack).map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        if let Some(base) = base_name {
            std::fs::write(format!("{base}.pack"), &pack)
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            std::fs::write(format!("{base}.idx"), &idx)
                .map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}
