//! `git unpack-objects`: unpack a pack read from stdin into loose objects.

use std::io::{Read, Write};

use crate::{Command, CommandError};
use git_core::Repository;
use git_hash::Oid;
use git_odb::pack::PackFile;
use git_odb::LooseStore;

pub struct UnpackObjects;

impl Command for UnpackObjects {
    fn name(&self) -> &'static str {
        "unpack-objects"
    }

    fn run(&self, args: &[String], _out: &mut dyn Write) -> Result<(), CommandError> {
        for a in args {
            if a != "-q" {
                return Err(CommandError::usage(format!("unpack-objects: unknown option '{a}'")));
            }
        }

        let repo = Repository::discover()?;
        let store = LooseStore::from_repo(&repo);
        let algo = repo.hash_algo;

        let mut pack_bytes = Vec::new();
        std::io::stdin()
            .read_to_end(&mut pack_bytes)
            .map_err(|e| CommandError::fatal(e.to_string()))?;
        let pf = PackFile::from_bytes(pack_bytes, algo).map_err(CommandError::from)?;

        let end = pf.data_end();
        let mut pos = pf.first_entry_offset();
        while pos < end {
            let mut resolver = |boid: &Oid| store.read(boid).ok();
            let resolved = pf
                .resolve_entry(pos, None, &mut resolver)
                .map_err(|e| CommandError::error(e.to_string()))?;
            store.write(&resolved.object).map_err(CommandError::from)?;
            pos += resolved.entry_len;
            if pos > end {
                return Err(CommandError::error("pack extends past its trailer"));
            }
        }
        Ok(())
    }
}
