//! Pack file reading and object resolution.

use std::path::Path;

use super::crc32::crc32;
use super::delta::apply_delta;
use super::index::PackIndex;
use super::{inflate_exact, PackError};
use git_hash::{HashAlgorithm, Oid};
use git_object::{Object, ObjectKind};

const MAX_DELTA_DEPTH: usize = 128;

/// What kind of object an entry encodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Base(ObjectKind),
    OfsDelta,
    RefDelta,
}

/// A parsed pack entry header.
#[derive(Debug, Clone)]
pub struct Entry {
    pub kind: EntryKind,
    /// For base objects: payload size. For deltas: result size.
    pub size: u64,
    /// Offset where the compressed payload begins.
    pub data_start: usize,
    /// For `OfsDelta`: the absolute offset of the base entry.
    pub base_offset: Option<u64>,
    /// For `RefDelta`: the base object id.
    pub base_oid: Option<Oid>,
}

/// A resolved object and the total byte length of its pack entry.
#[derive(Debug, Clone)]
pub struct ResolvedEntry {
    pub object: Object,
    /// Byte length of the (top-level) pack entry: header + compressed data.
    pub entry_len: usize,
}

/// An in-memory pack file.
#[derive(Debug, Clone)]
pub struct PackFile {
    data: Vec<u8>,
    algo: HashAlgorithm,
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

impl PackFile {
    pub fn from_bytes(data: Vec<u8>, algo: HashAlgorithm) -> Result<PackFile, PackError> {
        let f = PackFile { data, algo };
        let (version, _count) = f.parse_header()?;
        if version != 2 {
            return Err(PackError::BadVersion);
        }
        Ok(f)
    }

    pub fn open(path: &Path, algo: HashAlgorithm) -> Result<PackFile, PackError> {
        let data = std::fs::read(path).map_err(|e| PackError::Io(e.to_string()))?;
        PackFile::from_bytes(data, algo)
    }

    /// Parse the 12-byte header: `PACK`, version, object count.
    pub fn parse_header(&self) -> Result<(u32, u32), PackError> {
        if self.data.len() < 12 + self.algo.raw_len() {
            return Err(PackError::Truncated);
        }
        if &self.data[0..4] != b"PACK" {
            return Err(PackError::BadMagic);
        }
        let version = be32(&self.data[4..8]);
        let count = be32(&self.data[8..12]);
        Ok((version, count))
    }

    pub fn object_count(&self) -> u32 {
        self.parse_header().map(|(_, c)| c).unwrap_or(0)
    }

    pub fn algorithm(&self) -> HashAlgorithm {
        self.algo
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The trailing hash of the pack.
    pub fn trailer(&self) -> &[u8] {
        &self.data[self.data.len() - self.algo.raw_len()..]
    }

    /// The offset of the first object entry.
    pub fn first_entry_offset(&self) -> usize {
        12
    }

    /// The offset just past the last entry (start of the trailer).
    pub fn data_end(&self) -> usize {
        self.data.len() - self.algo.raw_len()
    }

    /// Verify the pack's own trailing checksum.
    pub fn verify_trailer(&self) -> Result<(), PackError> {
        let mut h = self.algo.hasher();
        h.update(&self.data[..self.data_end()]);
        if h.finalize() != self.trailer() {
            return Err(PackError::BadChecksum);
        }
        Ok(())
    }

    /// Parse the entry header at `offset`.
    pub fn entry_at(&self, offset: usize) -> Result<Entry, PackError> {
        parse_entry_header(&self.data, offset, self.algo)
    }

    /// Resolve the object at `offset`, following delta chains.
    ///
    /// `index` (if provided) is used to locate `RefDelta` bases within this
    /// pack; `resolver` supplies bases not found in the pack (thin packs,
    /// loose objects).
    pub fn resolve_entry(
        &self,
        offset: usize,
        index: Option<&PackIndex>,
        resolver: &mut dyn FnMut(&Oid) -> Option<Object>,
    ) -> Result<ResolvedEntry, PackError> {
        self.resolve_entry_inner(offset, index, resolver, 0)
    }

    fn resolve_entry_inner(
        &self,
        offset: usize,
        index: Option<&PackIndex>,
        resolver: &mut dyn FnMut(&Oid) -> Option<Object>,
        depth: usize,
    ) -> Result<ResolvedEntry, PackError> {
        if depth > MAX_DELTA_DEPTH {
            return Err(PackError::BadDelta);
        }
        if offset >= self.data_end() {
            return Err(PackError::Truncated);
        }
        let entry = self.entry_at(offset)?;
        let (payload, compressed_len) = inflate_exact(&self.data[entry.data_start..], entry.size as usize)?;

        let object = match entry.kind {
            EntryKind::Base(kind) => Object::from_data(kind, payload),
            EntryKind::OfsDelta => {
                let base_off = entry.base_offset.ok_or(PackError::BadDelta)? as usize;
                let base = self.resolve_entry_inner(base_off, index, resolver, depth + 1)?.object;
                Object::from_data(base.kind, apply_delta(&base.data, &payload)?)
            }
            EntryKind::RefDelta => {
                let base_oid = entry.base_oid.clone().ok_or(PackError::BadDelta)?;
                let base = if let Some(idx) = index {
                    match idx.find(&base_oid) {
                        Some(boff) => self.resolve_entry_inner(boff as usize, index, resolver, depth + 1)?.object,
                        None => resolver(&base_oid).ok_or(PackError::NotFound)?,
                    }
                } else {
                    resolver(&base_oid).ok_or(PackError::NotFound)?
                };
                Object::from_data(base.kind, apply_delta(&base.data, &payload)?)
            }
        };

        let entry_len = entry.data_start + compressed_len - offset;
        Ok(ResolvedEntry { object, entry_len })
    }

    /// Verify this pack against its index: trailer, count, per-object CRC,
    /// and that each object's computed id matches the index.
    pub fn verify(&self, index: &PackIndex) -> Result<(), PackError> {
        self.verify_trailer()?;
        if index.pack_checksum() != self.trailer() {
            return Err(PackError::BadChecksum);
        }
        if index.len() != self.object_count() as usize {
            return Err(PackError::Corrupt("pack/index object count mismatch".to_string()));
        }

        let mut resolver = |_: &Oid| -> Option<Object> { None };
        for i in 0..index.len() {
            let offset = index.offset_at(i)? as usize;
            if offset >= self.data_end() {
                return Err(PackError::Truncated);
            }
            let resolved = self.resolve_entry(offset, Some(index), &mut resolver)?;
            let entry_crc = crc32(&self.data[offset..offset + resolved.entry_len]);
            if entry_crc != index.crc_at(i) {
                return Err(PackError::Corrupt(format!("crc mismatch at index {i}")));
            }
            let oid = resolved.object.compute_id(self.algo);
            if &oid != index.oid_at(i) {
                return Err(PackError::Corrupt(format!("oid mismatch at index {i}")));
            }
        }
        Ok(())
    }
}

/// Parse a pack entry header at `start`.
fn parse_entry_header(data: &[u8], start: usize, algo: HashAlgorithm) -> Result<Entry, PackError> {
    let mut pos = start;
    let c0 = *data.get(pos).ok_or(PackError::Truncated)?;
    pos += 1;
    let type_code = (c0 >> 4) & 7;
    // Pack entry sizes use plain 7-bit chunks (low 4 bits in the first byte).
    let mut size = u64::from(c0 & 0x0f);
    let mut shift = 4u32;
    let mut c = c0;
    while c & 0x80 != 0 {
        c = *data.get(pos).ok_or(PackError::Truncated)?;
        pos += 1;
        size |= u64::from(c & 0x7f) << shift;
        shift += 7;
        if shift > 64 {
            return Err(PackError::Corrupt("object size overflow".to_string()));
        }
    }

    let kind = match type_code {
        1 => EntryKind::Base(ObjectKind::Commit),
        2 => EntryKind::Base(ObjectKind::Tree),
        3 => EntryKind::Base(ObjectKind::Blob),
        4 => EntryKind::Base(ObjectKind::Tag),
        6 => EntryKind::OfsDelta,
        7 => EntryKind::RefDelta,
        other => return Err(PackError::BadObjectType(other)),
    };

    let (base_offset, base_oid) = match kind {
        EntryKind::OfsDelta => {
            // Negative relative offset, encoded with git's "+1" varint scheme.
            let c = *data.get(pos).ok_or(PackError::Truncated)?;
            pos += 1;
            let mut off = u64::from(c & 0x7f);
            let mut c = c;
            while c & 0x80 != 0 {
                c = *data.get(pos).ok_or(PackError::Truncated)?;
                pos += 1;
                off = ((off + 1) << 7) | u64::from(c & 0x7f);
            }
            let base = (start as u64)
                .checked_sub(off)
                .ok_or_else(|| PackError::Corrupt("bad ofs_delta base offset".to_string()))?;
            (Some(base), None)
        }
        EntryKind::RefDelta => {
            let raw = algo.raw_len();
            let end = pos + raw;
            if data.len() < end {
                return Err(PackError::Truncated);
            }
            let oid = Oid::new(algo, &data[pos..end]);
            pos = end;
            (None, Some(oid))
        }
        _ => (None, None),
    };

    Ok(Entry {
        kind,
        size,
        data_start: pos,
        base_offset,
        base_oid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::write::{write_pack, PackObject};
    use git_object::ObjectKind;

    #[test]
    fn header_and_trailer() {
        let algo = HashAlgorithm::Sha1;
        let (pack, _idx) = write_pack(&[], algo).unwrap();
        let pf = PackFile::from_bytes(pack, algo).unwrap();
        assert_eq!(pf.object_count(), 0);
        assert_eq!(pf.first_entry_offset(), 12);
    }

    #[test]
    fn resolves_all_objects() {
        let algo = HashAlgorithm::Sha1;
        let objects = [
            Object::from_data(ObjectKind::Blob, b"payload one".to_vec()),
            Object::from_data(ObjectKind::Blob, b"payload two".to_vec()),
            Object::from_data(ObjectKind::Tree, b"100644 f\0abc".as_slice().to_vec()),
        ];
        let pos: Vec<PackObject> = objects
            .iter()
            .map(|o| PackObject {
                oid: o.compute_id(algo),
                kind: o.kind,
                data: o.data.clone(),
            })
            .collect();
        let (pack, idx_bytes) = write_pack(&pos, algo).unwrap();
        let pf = PackFile::from_bytes(pack.clone(), algo).unwrap();
        let idx = PackIndex::parse(&idx_bytes, algo).unwrap();
        let mut resolver = |_: &Oid| -> Option<Object> { None };
        for obj in &objects {
            let oid = obj.compute_id(algo);
            let off = idx.find(&oid).unwrap() as usize;
            let resolved = pf.resolve_entry(off, Some(&idx), &mut resolver).unwrap();
            assert_eq!(resolved.object, *obj);
            assert!(resolved.entry_len > 0);
        }
        let _ = pack;
    }

    #[test]
    fn verify_passes_and_detects_corruption() {
        let algo = HashAlgorithm::Sha1;
        let objects = [
            Object::from_data(ObjectKind::Blob, b"alpha".to_vec()),
            Object::from_data(ObjectKind::Commit, b"tree 0000\n".to_vec()),
        ];
        let pos: Vec<PackObject> = objects
            .iter()
            .map(|o| PackObject {
                oid: o.compute_id(algo),
                kind: o.kind,
                data: o.data.clone(),
            })
            .collect();
        let (mut pack, idx_bytes) = write_pack(&pos, algo).unwrap();
        let idx = PackIndex::parse(&idx_bytes, algo).unwrap();
        let pf = PackFile::from_bytes(pack.clone(), algo).unwrap();
        assert!(pf.verify(&idx).is_ok());

        // Corrupt one payload byte.
        let mid = 20;
        pack[mid] ^= 0xff;
        let pf = PackFile::from_bytes(pack, algo).unwrap();
        assert!(pf.verify(&idx).is_err());
    }
}
