//! `git verify-pack`: verify a pack against its index.

use std::io::Write;
use std::path::PathBuf;

use crate::{Command, CommandError};
use git_core::Repository;
use git_hash::HashAlgorithm;
use git_odb::pack::{PackFile, PackIndex};

pub struct VerifyPack;

impl Command for VerifyPack {
    fn name(&self) -> &'static str {
        "verify-pack"
    }

    fn run(&self, args: &[String], _out: &mut dyn Write) -> Result<(), CommandError> {
        let mut files: Vec<String> = Vec::new();
        for a in args {
            if a.starts_with('-') && a.len() > 1 {
                return Err(CommandError::usage(format!("verify-pack: unknown option '{a}'")));
            }
            files.push(a.clone());
        }
        if files.len() != 1 {
            return Err(CommandError::usage(
                "verify-pack: requires exactly one <pack-idx|pack-file>",
            ));
        }

        let algo = Repository::discover().ok().map(|r| r.hash_algo).unwrap_or(HashAlgorithm::Sha1);
        let (idx_path, pack_path) = split_pack_paths(&files[0]);

        let idx_data = std::fs::read(&idx_path)
            .map_err(|e| CommandError::fatal(format!("cannot open '{}': {e}", idx_path.display())))?;
        let idx = PackIndex::parse(&idx_data, algo).map_err(CommandError::from)?;
        let pack = PackFile::open(&pack_path, algo).map_err(CommandError::from)?;
        pack.verify(&idx).map_err(|e| CommandError::error(e.to_string()))?;
        Ok(())
    }
}

fn split_pack_paths(f: &str) -> (PathBuf, PathBuf) {
    let p = PathBuf::from(f);
    if p.extension().and_then(|x| x.to_str()) == Some("idx") {
        (p.clone(), p.with_extension("pack"))
    } else {
        (p.with_extension("idx"), p)
    }
}
