//! `git multi-pack-index`: write and verify the multi-pack-index.

use std::io::Write;
use std::path::Path;

use crate::{Command, CommandError, RepoContext};
use git_core::Repository;
use git_odb::pack::midx::{write_from_indexes, Midx};
use git_odb::pack::PackIndex;

pub struct MultiPackIndex;

impl Command for MultiPackIndex {
    fn name(&self) -> &'static str {
        "multi-pack-index"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], _out: &mut dyn Write) -> Result<(), CommandError> {
        let mut subcommand: Option<String> = None;
        for a in args {
            match a.as_str() {
                "write" | "verify" => {
                    if subcommand.is_some() {
                        return Err(CommandError::usage("multi-pack-index: too many subcommands"));
                    }
                    subcommand = Some(a.clone());
                }
                s if s.starts_with('-') => {
                    // --object-dir etc. are accepted for CLI compat.
                }
                _ => return Err(CommandError::usage(format!("multi-pack-index: unknown argument '{a}'"))),
            }
        }
        let subcommand = subcommand.ok_or_else(|| CommandError::usage("multi-pack-index: need a subcommand (write|verify)"))?;

        let repo = ctx.repository()?;
        let pack_dir = repo.common_dir.join("objects/pack");
        match subcommand.as_str() {
            "write" => write(&repo, &pack_dir),
            "verify" => verify(&pack_dir, repo.hash_algo),
            _ => unreachable!(),
        }
    }
}

fn write(repo: &Repository, pack_dir: &Path) -> Result<(), CommandError> {
    let mut indexes: Vec<(String, PackIndex)> = Vec::new();
    let rd = std::fs::read_dir(pack_dir)
        .map_err(|e| CommandError::fatal(format!("cannot read '{}': {e}", pack_dir.display())))?;
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) == Some("idx") {
            let data = std::fs::read(&p).map_err(|e| CommandError::fatal(e.to_string()))?;
            let idx = PackIndex::parse(&data, repo.hash_algo).map_err(CommandError::from)?;
            let base = p
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| CommandError::fatal("bad pack file name"))?
                .to_string();
            indexes.push((base, idx));
        }
    }
    let midx = write_from_indexes(&indexes, repo.hash_algo).map_err(CommandError::from)?;
    let path = pack_dir.join("multi-pack-index");
    std::fs::write(&path, &midx).map_err(|e| CommandError::fatal(e.to_string()))?;
    Ok(())
}

fn verify(pack_dir: &Path, algo: git_hash::HashAlgorithm) -> Result<(), CommandError> {
    let path = pack_dir.join("multi-pack-index");
    let data = std::fs::read(&path)
        .map_err(|e| CommandError::error(format!("cannot open '{}': {e}", path.display())))?;
    let midx = Midx::parse(data, algo).map_err(CommandError::from)?;
    midx.verify().map_err(|e| CommandError::error(e.to_string()))?;
    Ok(())
}
