//! `git count-objects`: report loose and packed object counts.

use std::collections::HashSet;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::{Command, CommandError, RepoContext};
use git_core::Repository;
use git_odb::Odb;

/// On-disk size of a file in KiB (git uses `st_blocks * 512 / 1024`).
fn on_disk_kib(path: &Path) -> u64 {
    match std::fs::metadata(path) {
        Ok(m) => m.blocks() * 512 / 1024,
        Err(_) => 0,
    }
}

pub struct CountObjects;

impl Command for CountObjects {
    fn name(&self) -> &'static str {
        "count-objects"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut verbose = false;
        for a in args {
            match a.as_str() {
                "-v" => verbose = true,
                _ => return Err(CommandError::usage(format!("count-objects: unknown option '{a}'"))),
            }
        }
        let repo = ctx.repository()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let loose_oids = odb.loose.iter_oids();
        let loose = loose_oids.len();
        let in_pack: usize = odb.packs.iter().map(|(_, idx)| idx.len()).sum();

        if !verbose {
            writeln!(out, "{loose}").map_err(|e| CommandError::fatal(e.to_string()))?;
            return Ok(());
        }

        // size: on-disk KiB of all loose objects.
        let mut size = 0u64;
        for oid in &loose_oids {
            size += on_disk_kib(&odb.loose.oid_path(oid));
        }

        // size-pack: byte sizes of the .pack + .idx files, divided by 1024
        // (git: `size_pack += p->pack_size + p->index_size`, then /1024).
        let mut size_pack = 0u64;
        let pack_dir = repo.common_dir.join("objects/pack");
        if let Ok(rd) = std::fs::read_dir(&pack_dir) {
            for e in rd.flatten() {
                let p = e.path();
                let ext = p.extension().and_then(|x| x.to_str());
                if ext == Some("pack") || ext == Some("idx") {
                    size_pack += std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                }
            }
        }
        size_pack /= 1024;

        // prune-packable: loose objects that also exist in a pack.
        let mut prune_packable = 0usize;
        let packed: HashSet<git_hash::Oid> = odb
            .packs
            .iter()
            .flat_map(|(_, idx)| idx.oids().iter().cloned())
            .collect();
        for oid in &loose_oids {
            if packed.contains(oid) {
                prune_packable += 1;
            }
        }

        // garbage: unrecognized files under objects/ (not fanout dirs, not
        // info/pack contents).
        let (garbage, size_garbage) = scan_garbage(&repo);

        writeln!(out, "count: {loose}").map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "size: {size}").map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "in-pack: {in_pack}").map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "packs: {}", odb.packs.len()).map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "size-pack: {size_pack}").map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "prune-packable: {prune_packable}").map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "garbage: {garbage}").map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "size-garbage: {size_garbage}").map_err(|e| CommandError::fatal(e.to_string()))?;
        Ok(())
    }
}

/// Count unrecognized files: git counts garbage only in `objects/pack` —
/// files without a recognized pack-file extension.
fn scan_garbage(repo: &Repository) -> (usize, u64) {
    let mut garbage = 0usize;
    let mut size = 0u64;
    let pack_dir = repo.common_dir.join("objects/pack");
    if let Ok(rd) = std::fs::read_dir(pack_dir) {
        for e in rd.flatten() {
            let path = e.path();
            let ext = path.extension().and_then(|x| x.to_str()).map(|s| s.to_string());
            let known_ext = matches!(
                ext.as_deref(),
                Some("pack") | Some("idx") | Some("rev") | Some("mtimes") | Some("bitmap")
                    | Some("promisor") | Some("keep")
            );
            if !known_ext {
                garbage += 1;
                size += on_disk_kib(&path);
            }
        }
    }
    (garbage, size)
}