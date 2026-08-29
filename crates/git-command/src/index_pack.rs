//! `git index-pack`: build a pack index from a pack file.

use std::io::Write;

use crate::{Command, CommandError, RepoContext};
use git_hash::Oid;
use git_odb::pack::crc32::crc32;
use git_odb::pack::{write_idx, PackFile, PackIndex};

pub struct IndexPack;

impl Command for IndexPack {
    fn name(&self) -> &'static str {
        "index-pack"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut verify = false;
        let mut files: Vec<String> = Vec::new();
        for a in args {
            match a.as_str() {
                "--verify" => verify = true,
                "-v" | "-q" | "--stdin" | "--fix-thin" => {}
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("index-pack: option '{s}' not supported")));
                }
                s => files.push(s.to_string()),
            }
        }
        if files.len() != 1 {
            return Err(CommandError::usage("index-pack: requires a <pack-file>"));
        }
        let pack_path = std::path::PathBuf::from(&files[0]);
        let idx_path = pack_path.with_extension("idx");

        if verify {
            // Verify an existing pack against its index.
            let repo = ctx.repository().ok();
            let algo = repo.as_ref().map(|r| r.hash_algo).unwrap_or(git_hash::HashAlgorithm::Sha1);
            let idx_data = std::fs::read(&idx_path)
                .map_err(|e| CommandError::error(format!("cannot open '{}': {e}", idx_path.display())))?;
            let idx = PackIndex::parse(&idx_data, algo).map_err(|e| CommandError::fatal(e.to_string()))?;
            let pf = PackFile::open(&pack_path, algo).map_err(|e| CommandError::fatal(e.to_string()))?;
            pf.verify(&idx).map_err(|e| CommandError::error(e.to_string()))?;
            return Ok(());
        }

        let repo = ctx.repository()?;
        let algo = repo.hash_algo;
        let data = std::fs::read(&pack_path)
            .map_err(|e| CommandError::error(format!("cannot open '{}': {e}", pack_path.display())))?;
        let pf = PackFile::from_bytes(data.clone(), algo).map_err(|e| CommandError::fatal(e.to_string()))?;

        // Walk the entries, resolving objects (thin-pack bases may come from
        // the repository's loose store).
        let loose = git_odb::LooseStore::from_repo(&repo);
        let mut entries: Vec<(Oid, u64, u32)> = Vec::new();
        let mut pos = pf.first_entry_offset();
        let end = pf.data_end();
        while pos < end {
            let mut resolver = |oid: &Oid| loose.read(oid).ok();
            let resolved = pf
                .resolve_entry(pos, None, &mut resolver)
                .map_err(|e| CommandError::error(e.to_string()))?;
            let oid = resolved.object.compute_id(algo);
            let crc = crc32(&data[pos..pos + resolved.entry_len]);
            entries.push((oid, pos as u64, crc));
            pos += resolved.entry_len;
        }

        // The index requires oids sorted; offsets/crcs follow their oid.
        entries.sort_by_key(|e| e.0);
        let trailer = pf.trailer().to_vec();
        let idx_bytes = write_idx(&entries, &trailer, algo);
        std::fs::write(&idx_path, &idx_bytes).map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "{}", pack_path.display()).map_err(|e| CommandError::fatal(e.to_string()))?;
        Ok(())
    }
}