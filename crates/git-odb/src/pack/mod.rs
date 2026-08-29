//! Pack file support: reading, index (v2), delta resolution, writing, and an
//! object database that reads across loose objects and packs.

pub mod crc32;
pub mod delta;
pub mod file;
pub mod index;
pub mod midx;
pub mod write;

pub use file::{EntryKind, PackFile};
pub use index::PackIndex;
pub use midx::{Midx, MidxError};
pub use write::{write_idx, write_pack, PackObject};

use std::error::Error;
use std::fmt;

use flate2::{Decompress, FlushDecompress, Status};
use git_core::Repository;
use git_hash::{HashAlgorithm, Oid};
use git_object::Object;

use crate::{LooseStore, OdbError};

/// Errors from pack processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackError {
    Io(String),
    Truncated,
    BadMagic,
    BadVersion,
    BadObjectType(u8),
    BadChecksum,
    BadDelta,
    NotFound,
    Corrupt(String),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackError::Io(e) => write!(f, "pack I/O error: {e}"),
            PackError::Truncated => write!(f, "truncated pack data"),
            PackError::BadMagic => write!(f, "bad pack or index magic"),
            PackError::BadVersion => write!(f, "unsupported pack or index version"),
            PackError::BadObjectType(t) => write!(f, "invalid object type {t} in pack"),
            PackError::BadChecksum => write!(f, "pack or index checksum mismatch"),
            PackError::BadDelta => write!(f, "corrupt delta"),
            PackError::NotFound => write!(f, "object not found in pack"),
            PackError::Corrupt(m) => write!(f, "corrupt pack: {m}"),
        }
    }
}

impl Error for PackError {}

impl From<PackError> for OdbError {
    fn from(e: PackError) -> OdbError {
        match e {
            PackError::NotFound => OdbError::NotFound,
            other => OdbError::Corrupt(other.to_string()),
        }
    }
}

/// Inflate a single zlib stream, producing exactly `expected` output bytes.
///
/// Returns the decoded bytes and the number of compressed bytes consumed.
fn inflate_exact(input: &[u8], expected: usize) -> Result<(Vec<u8>, usize), PackError> {
    let mut d = Decompress::new(true); // zlib framing
    let mut out = vec![0u8; expected.saturating_add(8)];
    let mut in_pos = 0usize;
    let mut out_pos = 0usize;

    loop {
        if out_pos >= expected {
            // Output target reached; keep feeding until StreamEnd so we learn
            // the exact compressed length.
            let before_in = d.total_in();
            let mut scratch = [0u8; 8];
            let s = d
                .decompress(&input[in_pos..], &mut scratch, FlushDecompress::None)
                .map_err(|e| PackError::Io(e.to_string()))?;
            in_pos += (d.total_in() - before_in) as usize;
            if s == Status::StreamEnd {
                break;
            }
            if s == Status::BufError && in_pos >= input.len() {
                return Err(PackError::Truncated);
            }
            continue;
        }

        let before_in = d.total_in();
        let before_out = d.total_out();
        let s = d
            .decompress(&input[in_pos..], &mut out[out_pos..], FlushDecompress::None)
            .map_err(|e| PackError::Io(e.to_string()))?;
        in_pos += (d.total_in() - before_in) as usize;
        out_pos += (d.total_out() - before_out) as usize;

        if s == Status::StreamEnd {
            break;
        }
        if s == Status::BufError && in_pos >= input.len() {
            return Err(PackError::Truncated);
        }
        if out_pos > expected {
            return Err(PackError::Corrupt(format!(
                "object larger than declared size {expected}"
            )));
        }
    }

    if out_pos != expected {
        return Err(PackError::Corrupt(format!(
            "object size mismatch: expected {expected}, decoded {out_pos}"
        )));
    }
    out.truncate(expected);
    Ok((out, in_pos))
}

/// An object database that reads from the loose store and all pack files.
#[derive(Debug, Clone)]
pub struct Odb {
    pub loose: LooseStore,
    pub packs: Vec<(file::PackFile, index::PackIndex)>,
}

impl Odb {
    /// Open the object database for a repository, discovering its pack files
    /// (including `GIT_OBJECT_DIRECTORY`, alternates, and `info/alternates`).
    pub fn from_repo(repo: &Repository) -> Result<Odb, PackError> {
        let loose = LooseStore::from_repo(repo);
        let mut packs = Vec::new();
        let mut pack_dirs = vec![loose.objects_dir().join("pack")];
        pack_dirs.extend(loose.alternates().iter().map(|d| d.join("pack")));
        let mut seen = std::collections::HashSet::new();
        for pack_dir in pack_dirs {
            if !seen.insert(pack_dir.clone()) {
                continue;
            }
            if let Ok(rd) = std::fs::read_dir(&pack_dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()) == Some("idx") {
                        let idx_data = match std::fs::read(&p) {
                            Ok(d) => d,
                            Err(_) => continue,
                        };
                        let idx = match index::PackIndex::parse(&idx_data, repo.hash_algo) {
                            Ok(i) => i,
                            Err(_) => continue,
                        };
                        let pack_path = p.with_extension("pack");
                        if let Ok(pdata) = std::fs::read(&pack_path) {
                            if let Ok(pf) = file::PackFile::from_bytes(pdata, repo.hash_algo) {
                                packs.push((pf, idx));
                            }
                        }
                    }
                }
            }
        }
        Ok(Odb { loose, packs })
    }

    pub fn algorithm(&self) -> HashAlgorithm {
        self.loose.algorithm()
    }

    /// On-disk footprint and delta base (if any) of `oid`: for loose objects
    /// the loose file size and no base; for packed objects the packed entry
    /// span and the delta base when the entry is a delta.
    pub fn disk_info(&self, oid: &Oid) -> Option<(u64, Option<Oid>)> {
        let loose_path = self.loose.oid_path(oid);
        if let Ok(meta) = std::fs::metadata(&loose_path) {
            return Some((meta.len(), None));
        }
        for (pf, idx) in &self.packs {
            let Some(offset) = idx.find(oid) else {
                continue;
            };
            let mut offsets: Vec<u64> = Vec::with_capacity(idx.len());
            for i in 0..idx.len() {
                if let Ok(off) = idx.offset_at(i) {
                    offsets.push(off);
                }
            }
            offsets.sort_unstable();
            offsets.dedup();
            let disk = match offsets.binary_search(&offset) {
                Ok(i) => {
                    if i + 1 < offsets.len() {
                        offsets[i + 1] - offset
                    } else {
                        (pf.data_end() as u64).saturating_sub(offset)
                    }
                }
                Err(_) => 0,
            };
            let base = pf
                .entry_at(offset as usize)
                .ok()
                .and_then(|entry| match entry.kind {
                    EntryKind::OfsDelta => entry.base_offset.and_then(|base_off| {
                        (0..idx.len()).find_map(|i| {
                            match idx.offset_at(i) {
                                Ok(off) if off == base_off => Some(idx.oid_at(i).clone()),
                                _ => None,
                            }
                        })
                    }),
                    EntryKind::RefDelta => entry.base_oid.clone(),
                    EntryKind::Base(_) => None,
                });
            return Some((disk, base));
        }
        None
    }

    pub fn contains(&self, oid: &Oid) -> bool {
        self.loose.contains(oid) || self.packs.iter().any(|(_, idx)| idx.find(oid).is_some())
    }

    /// Read an object by id from loose storage or any pack.
    pub fn read(&self, oid: &Oid) -> Result<Object, OdbError> {
        if let Ok(o) = self.loose.read(oid) {
            return Ok(o);
        }
        for (pf, idx) in &self.packs {
            if let Some(off) = idx.find(oid) {
                let mut resolver = |boid: &Oid| self.loose.read(boid).ok();
                return pf
                    .resolve_entry(off as usize, Some(idx), &mut resolver)
                    .map(|r| r.object)
                    .map_err(OdbError::from);
            }
        }
        Err(OdbError::NotFound)
    }

    /// Number of loose objects and objects in packs.
    pub fn object_counts(&self) -> (usize, usize) {
        let packs: usize = self.packs.iter().map(|(_, idx)| idx.len()).sum();
        let loose = self.loose.object_count();
        (loose, packs)
    }
}
