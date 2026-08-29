//! `git count-objects`: report loose and packed object counts.
//!
//! Port of `builtin/count-objects.c`: loose-object and pack scanning with
//! garbage reporting (`warning:` lines on stderr), byte-accumulated sizes
//! divided by 1024, and `-H` human-readable output via C git's
//! `humanise_bytes`.

use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::{Command, CommandError, RepoContext};
use git_odb::Odb;

/// On-disk bytes of a file (C git's `on_disk_bytes`: `st_blocks * 512`).
fn on_disk_bytes(path: &Path) -> u64 {
    match std::fs::metadata(path) {
        Ok(m) => m.blocks() * 512,
        Err(_) => 0,
    }
}

/// Render a path the way C git does: relative to the cwd when it lies under
/// it (C git constructs object paths from the discovered `.git` location).
fn display_relative(path: &Path) -> String {
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(_) => return path.display().to_string(),
    };
    match path.strip_prefix(&cwd) {
        Ok(rel) => rel.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

/// Human-readable byte count matching C git's `humanise_bytes`.
fn humanise_bytes(bytes: u64) -> String {
    if bytes > 1 << 30 {
        let whole = bytes >> 30;
        let frac = (bytes & ((1 << 30) - 1)) / 10737419;
        format!("{whole}.{frac:02} GiB")
    } else if bytes > 1 << 20 {
        let x = bytes + 5243;
        let whole = x >> 20;
        let frac = ((x & ((1 << 20) - 1)) * 100) >> 20;
        format!("{whole}.{frac:02} MiB")
    } else if bytes > 1 << 10 {
        let x = bytes + 5;
        let whole = x >> 10;
        let frac = ((x & ((1 << 10) - 1)) * 100) >> 10;
        format!("{whole}.{frac:02} KiB")
    } else if bytes == 1 {
        "1 byte".to_string()
    } else {
        format!("{bytes} bytes")
    }
}

pub struct CountObjects;

impl Command for CountObjects {
    fn name(&self) -> &'static str {
        "count-objects"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut verbose = false;
        let mut human = false;
        for a in args {
            match a.as_str() {
                "-v" | "--verbose" => verbose = true,
                "-H" | "--human-readable" => human = true,
                _ => {
                    return Err(CommandError::usage(
                        "usage: git count-objects [-v] [-H | --human-readable]\n\n    -v, --verbose         be verbose\n    -H, --human-readable  print sizes in human readable format\n",
                    ));
                }
            }
        }
        let repo = ctx.repository()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let objects_dir = repo.common_dir.join("objects");

        let mut loose = 0usize;
        let mut loose_size = 0u64;
        let mut packed_loose = 0usize;
        let mut garbage = 0usize;
        let mut size_garbage = 0u64;

        let packed: std::collections::HashSet<git_hash::Oid> = odb
            .packs
            .iter()
            .flat_map(|(_, idx)| idx.oids().iter().cloned())
            .collect();

        // Loose objects: iterate fanout dirs; files with non-oid names are
        // garbage (C's `count_loose` / `count_cruft`).
        let rd = match std::fs::read_dir(&objects_dir) {
            Ok(rd) => rd,
            Err(_) => return Ok(()),
        };
        for entry in rd.flatten() {
            let fan = entry.path();
            let fname = entry.file_name().to_string_lossy().into_owned();
            if !(fname.len() == 2 && fname.bytes().all(|b| b.is_ascii_hexdigit())) || !fan.is_dir() {
                continue;
            }
            let Ok(e) = std::fs::read_dir(&fan) else { continue };
            for e in e.flatten() {
                let path = e.path();
                let name = e.file_name().to_string_lossy().into_owned();
                let full = format!("{fname}{name}");
                let oid_ok = name.len() == repo.hash_algo.hex_len() - 2
                    && full.bytes().all(|b| b.is_ascii_hexdigit())
                    && git_hash::Oid::from_hex(&full, repo.hash_algo).is_ok();
                let is_file = path.is_file();
                if !oid_ok || !is_file {
                    if verbose {
                        eprintln!("warning: garbage found: {}", display_relative(&path));
                        garbage += 1;
                        size_garbage += std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    }
                    continue;
                }
                loose_size += on_disk_bytes(&path);
                loose += 1;
                if verbose {
                    if let Ok(oid) = git_hash::Oid::from_hex(&full, repo.hash_algo) {
                        if packed.contains(&oid) {
                            packed_loose += 1;
                        }
                    }
                }
            }
        }

        if !verbose {
            writeln!(out, "{} objects, {} kilobytes", loose, loose_size / 1024)
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            return Ok(());
        }

        // Pack directory, following C git's `prepare_pack` +
        // `report_pack_garbage`: known extensions group by basename; a
        // group missing either .pack or .idx is garbage, as are files with
        // unrecognized names.
        let mut in_pack = 0usize;
        let mut num_pack = 0usize;
        let mut size_pack = 0u64;
        let pack_dir = objects_dir.join("pack");
        let mut grouped: Vec<(String, PathBuf, u32)> = Vec::new(); // (base, path, bits)
        if let Ok(rd) = std::fs::read_dir(&pack_dir) {
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().into_owned();
                if name == "multi-pack-index" || name == "multi-pack-index.d" {
                    continue;
                }
                if name.starts_with("multi-pack-index")
                    && (name.ends_with(".bitmap") || name.ends_with(".rev"))
                {
                    continue;
                }
                const KNOWN: [&str; 7] =
                    ["idx", "rev", "pack", "bitmap", "keep", "promisor", "mtimes"];
                let ext = p.extension().and_then(|x| x.to_str());
                if !KNOWN.contains(&ext.unwrap_or("")) {
                    eprintln!("warning: garbage found: {}", display_relative(&p));
                    garbage += 1;
                    size_garbage += std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                    continue;
                }
                let base = p
                    .file_name()
                    .map(|f| {
                        let s = f.to_string_lossy().into_owned();
                        match s.rfind('.') {
                            Some(i) => s[..i].to_string(),
                            None => s,
                        }
                    })
                    .unwrap_or_default();
                let bits = match ext {
                    Some("pack") => 1u32,
                    Some("idx") => 2u32,
                    _ => 0u32,
                };
                grouped.push((base, p, bits));
            }
        }
        grouped.sort_by(|a, b| (a.0.as_str(), a.1.as_path()).cmp(&(b.0.as_str(), b.1.as_path())));
        let mut i = 0;
        while i < grouped.len() {
            let base = grouped[i].0.clone();
            let mut bits = 0u32;
            let mut j = i;
            while j < grouped.len() && grouped[j].0 == base {
                bits |= grouped[j].2;
                j += 1;
            }
            if bits == 3 {
                // Complete pack: count objects and sizes.
                let pack_path = grouped[i..j]
                    .iter()
                    .find(|(_, _, b)| *b == 1)
                    .map(|(_, p, _)| p.clone())
                    .unwrap();
                size_pack += std::fs::metadata(&pack_path).map(|m| m.len()).unwrap_or(0);
                size_pack += std::fs::metadata(pack_path.with_extension("idx"))
                    .map(|m| m.len())
                    .unwrap_or(0);
                if let Ok(data) = std::fs::read(pack_path.with_extension("idx")) {
                    if let Ok(idx) = git_odb::pack::PackIndex::parse(&data, repo.hash_algo) {
                        in_pack += idx.len();
                        num_pack += 1;
                    }
                }
            } else {
                let desc = match bits {
                    1 => "no corresponding .idx",
                    2 => "no corresponding .pack",
                    _ => "no corresponding .idx or .pack",
                };
                for (base, p, _b) in &grouped[i..j] {
                    let _ = base;
                    eprintln!("warning: {desc}: {}", display_relative(p));
                    garbage += 1;
                    size_garbage += std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                }
            }
            i = j;
        }

        let fmt_size = |bytes: u64| -> String {
            if human {
                humanise_bytes(bytes)
            } else {
                format!("{}", bytes / 1024)
            }
        };

        writeln!(out, "count: {loose}").map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "size: {}", fmt_size(loose_size))
            .map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "in-pack: {in_pack}").map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "packs: {num_pack}").map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "size-pack: {}", fmt_size(size_pack))
            .map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "prune-packable: {packed_loose}")
            .map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "garbage: {garbage}").map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "size-garbage: {}", fmt_size(size_garbage))
            .map_err(|e| CommandError::fatal(e.to_string()))?;
        Ok(())
    }
}
