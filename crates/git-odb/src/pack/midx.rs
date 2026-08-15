//! Multi-pack-index (MIDX) reading, verification, and writing.
//!
//! Port of the non-incremental MIDX format (`midx.c` / `midx-write.c`): a chunk
//! file holding the union of objects across several pack files. The header is
//! 12 bytes: `MIDX`, version, hash version, chunk count, base-layer count, and
//! the number of packs.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use git_commitgraph::chunk_format::{ChunkFile, ChunkError};
use git_hash::{HashAlgorithm, Oid};

use super::index::PackIndex;

pub const MIDX_SIGNATURE: u32 = 0x4d49_4458; // "MIDX"
pub const MIDX_HEADER_SIZE: usize = 12;
pub const MIDX_CHUNK_ALIGNMENT: usize = 4;

pub const MIDX_CHUNKID_PACKNAMES: u32 = 0x504e_414d; // "PNAM"
pub const MIDX_CHUNKID_OIDFANOUT: u32 = 0x4f49_4446; // "OIDF"
pub const MIDX_CHUNKID_OIDLOOKUP: u32 = 0x4f49_444c; // "OIDL"
pub const MIDX_CHUNKID_OBJECTOFFSETS: u32 = 0x4f4f_4646; // "OOFF"
pub const MIDX_CHUNKID_LARGEOFFSETS: u32 = 0x4c4f_4646; // "LOFF"

pub const MIDX_LARGE_OFFSET_NEEDED: u32 = 0x8000_0000;
const MIDX_CHUNK_OFFSET_WIDTH: usize = 8;

/// Errors from MIDX processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidxError {
    Io(String),
    Truncated,
    BadMagic,
    BadChecksum,
    NotFound,
    Corrupt(String),
}

impl fmt::Display for MidxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MidxError::Io(e) => write!(f, "MIDX I/O error: {e}"),
            MidxError::Truncated => write!(f, "multi-pack-index file too small"),
            MidxError::BadMagic => write!(f, "multi-pack-index signature does not match"),
            MidxError::BadChecksum => write!(f, "multi-pack-index checksum mismatch"),
            MidxError::NotFound => write!(f, "object not found in multi-pack-index"),
            MidxError::Corrupt(m) => write!(f, "corrupt multi-pack-index: {m}"),
        }
    }
}

impl Error for MidxError {}

impl From<ChunkError> for MidxError {
    fn from(e: ChunkError) -> MidxError {
        MidxError::Corrupt(e.to_string())
    }
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// A parsed multi-pack-index.
#[derive(Debug, Clone)]
pub struct Midx {
    algo: HashAlgorithm,
    file: ChunkFile,
    num_packs: u32,
    num_objects: usize,
    fanout: Vec<u32>,
    pack_names: Vec<String>,
    large_offset_count: usize,
}

impl Midx {
    pub fn parse(data: Vec<u8>, algo: HashAlgorithm) -> Result<Midx, MidxError> {
        if data.len() < MIDX_HEADER_SIZE + algo.raw_len() {
            return Err(MidxError::Truncated);
        }
        if be32(&data[0..4]) != MIDX_SIGNATURE {
            return Err(MidxError::BadMagic);
        }
        if data[4] != 1 && data[4] != 2 {
            return Err(MidxError::Corrupt(format!("unsupported MIDX version {}", data[4])));
        }
        let expected_oid = match algo {
            HashAlgorithm::Sha1 => 1,
            HashAlgorithm::Sha256 => 2,
        };
        if data[5] != expected_oid {
            return Err(MidxError::Corrupt("MIDX hash version does not match".into()));
        }
        let num_chunks = data[6] as usize;
        let num_packs = be32(&data[8..12]);

        let file = ChunkFile::parse(data, MIDX_HEADER_SIZE, num_chunks, MIDX_CHUNK_ALIGNMENT, algo)?;

        let pnam = file
            .chunk(MIDX_CHUNKID_PACKNAMES)
            .ok_or_else(|| MidxError::Corrupt("missing pack-name chunk".into()))?;
        let mut pack_names = Vec::with_capacity(num_packs as usize);
        let mut pos = 0usize;
        for _ in 0..num_packs {
            let end = pnam[pos..]
                .iter()
                .position(|&b| b == 0)
                .map(|i| pos + i)
                .ok_or_else(|| MidxError::Corrupt("pack-name chunk too short".into()))?;
            let name = std::str::from_utf8(&pnam[pos..end])
                .map_err(|_| MidxError::Corrupt("pack name is not UTF-8".into()))?;
            pack_names.push(name.to_string());
            pos = end + 1;
        }

        let fanout_chunk = file
            .chunk(MIDX_CHUNKID_OIDFANOUT)
            .ok_or_else(|| MidxError::Corrupt("missing OID fanout chunk".into()))?;
        if fanout_chunk.len() != 1024 {
            return Err(MidxError::Corrupt("OID fanout chunk of wrong size".into()));
        }
        let mut fanout = Vec::with_capacity(256);
        for i in 0..256 {
            fanout.push(be32(&fanout_chunk[i * 4..i * 4 + 4]));
        }
        for i in 0..255 {
            if fanout[i] > fanout[i + 1] {
                return Err(MidxError::Corrupt("OID fanout out of order".into()));
            }
        }
        let num_objects = fanout[255] as usize;

        let oidl = file
            .chunk(MIDX_CHUNKID_OIDLOOKUP)
            .ok_or_else(|| MidxError::Corrupt("missing OID lookup chunk".into()))?;
        if oidl.len() != num_objects * algo.raw_len() {
            return Err(MidxError::Corrupt("OID lookup chunk of wrong size".into()));
        }
        let ooff = file
            .chunk(MIDX_CHUNKID_OBJECTOFFSETS)
            .ok_or_else(|| MidxError::Corrupt("missing object offsets chunk".into()))?;
        if ooff.len() != num_objects * MIDX_CHUNK_OFFSET_WIDTH {
            return Err(MidxError::Corrupt("object offsets chunk of wrong size".into()));
        }
        let large_offset_count = match file.chunk(MIDX_CHUNKID_LARGEOFFSETS) {
            Some(l) => l.len() / 8,
            None => 0,
        };

        Ok(Midx {
            algo,
            file,
            num_packs,
            num_objects,
            fanout,
            pack_names,
            large_offset_count,
        })
    }

    pub fn num_packs(&self) -> u32 {
        self.num_packs
    }

    pub fn num_objects(&self) -> usize {
        self.num_objects
    }

    pub fn pack_names(&self) -> &[String] {
        &self.pack_names
    }

    pub fn large_offset_count(&self) -> usize {
        self.large_offset_count
    }

    /// The object id at lexicographic position `pos`.
    pub fn oid_at(&self, pos: usize) -> Option<Oid> {
        if pos >= self.num_objects {
            return None;
        }
        let oidl = self.file.chunk(MIDX_CHUNKID_OIDLOOKUP)?;
        let raw = self.algo.raw_len();
        Some(Oid::new(self.algo, &oidl[pos * raw..pos * raw + raw]))
    }

    /// Look up `oid`, returning `(pack_int_id, offset)`.
    pub fn find(&self, oid: &Oid) -> Option<(u32, u64)> {
        if oid.algorithm() != self.algo {
            return None;
        }
        let first = oid.as_slice()[0] as usize;
        let lo = if first == 0 { 0 } else { self.fanout[first - 1] as usize };
        let hi = self.fanout[first] as usize;
        let oidl = self.file.chunk(MIDX_CHUNKID_OIDLOOKUP)?;
        let raw = self.algo.raw_len();
        let slice = &oidl[lo * raw..hi * raw];
        let idx = slice
            .chunks_exact(raw)
            .position(|c| Oid::new(self.algo, c) == *oid)?;
        let pos = lo + idx;

        let ooff = self.file.chunk(MIDX_CHUNKID_OBJECTOFFSETS)?;
        let entry = &ooff[pos * 8..pos * 8 + 8];
        let pack_id = be32(&entry[0..4]);
        let offset32 = be32(&entry[4..8]);
        let offset = if offset32 & MIDX_LARGE_OFFSET_NEEDED != 0 {
            let idx = (offset32 ^ MIDX_LARGE_OFFSET_NEEDED) as usize;
            let loff = self.file.chunk(MIDX_CHUNKID_LARGEOFFSETS)?;
            if idx >= self.large_offset_count {
                return None;
            }
            u64::from_be_bytes(loff[idx * 8..idx * 8 + 8].try_into().ok()?)
        } else {
            u64::from(offset32)
        };
        Some((pack_id, offset))
    }

    /// Verify structural invariants (checksum is verified at parse time).
    pub fn verify(&self) -> Result<(), MidxError> {
        // OIDs must be strictly increasing.
        let mut prev: Option<Oid> = None;
        for pos in 0..self.num_objects {
            let oid = self.oid_at(pos).ok_or_else(|| MidxError::Corrupt("short OID lookup".into()))?;
            if let Some(p) = prev {
                if p >= oid {
                    return Err(MidxError::Corrupt("OID order is not strictly increasing".into()));
                }
            }
            prev = Some(oid);
        }
        // Object offsets: pack ids must be in range.
        let ooff = self
            .file
            .chunk(MIDX_CHUNKID_OBJECTOFFSETS)
            .ok_or_else(|| MidxError::Corrupt("missing object offsets chunk".into()))?;
        for pos in 0..self.num_objects {
            let pack_id = be32(&ooff[pos * 8..pos * 8 + 4]);
            if pack_id >= self.num_packs {
                return Err(MidxError::Corrupt(format!("object offset pack id {pack_id} out of range")));
            }
        }
        Ok(())
    }
}

/// Write a MIDX covering the given pack indexes.
///
/// `indexes` maps each pack's file base name (e.g. `pack-<hex>`, without
/// extension) to its parsed index. Packs are ordered by name (as git v1
/// requires). When an object appears in multiple packs, the copy in the
/// earliest pack (preferred pack) is used.
pub fn write_from_indexes(indexes: &[(String, PackIndex)], algo: HashAlgorithm) -> Result<Vec<u8>, MidxError> {
    let mut sorted: Vec<&(String, PackIndex)> = indexes.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    // Union of oids, keeping the first (preferred) copy.
    let mut union: BTreeMap<Oid, (u32, u64)> = BTreeMap::new();
    for (pack_id, (name, idx)) in sorted.iter().enumerate() {
        for (j, oid) in idx.oids().iter().enumerate() {
            let offset = idx.offset_at(j).map_err(|_| MidxError::Corrupt("bad index offset".into()))?;
            union.entry(*oid).or_insert((pack_id as u32, offset));
        }
        let _ = name;
    }
    let num_objects = union.len();

    // Pack names (with .pack extension), NUL separated, padded to 4.
    // Pack names (git stores the `.idx` file name), NUL separated, padded to 4.
    let mut pnam = Vec::new();
    for (name, _) in &sorted {
        pnam.extend_from_slice(name.as_bytes());
        pnam.extend_from_slice(b".idx");
        pnam.push(0);
    }
    while pnam.len() % MIDX_CHUNK_ALIGNMENT != 0 {
        pnam.push(0);
    }

    // OOFF + LOFF.
    let mut ooff = Vec::with_capacity(num_objects * 8);
    let mut loff: Vec<u64> = Vec::new();
    for (_, (pack_id, offset)) in &union {
        ooff.extend_from_slice(&pack_id.to_be_bytes());
        if *offset >= 0x8000_0000 {
            ooff.extend_from_slice(&(MIDX_LARGE_OFFSET_NEEDED | loff.len() as u32).to_be_bytes());
            loff.push(*offset);
        } else {
            ooff.extend_from_slice(&(*offset as u32).to_be_bytes());
        }
    }

    let has_loff = !loff.is_empty();
    let num_chunks = 4 + if has_loff { 1 } else { 0 };

    // Assemble: header + TOC + chunks + trailer.
    let toc_len = (num_chunks + 1) * git_commitgraph::CHUNK_TOC_ENTRY_SIZE;
    let mut chunk_ids = vec![
        MIDX_CHUNKID_PACKNAMES,
        MIDX_CHUNKID_OIDFANOUT,
        MIDX_CHUNKID_OIDLOOKUP,
        MIDX_CHUNKID_OBJECTOFFSETS,
    ];
    let mut chunk_sizes = vec![pnam.len(), 1024, num_objects * algo.raw_len(), num_objects * 8];
    if has_loff {
        chunk_ids.push(MIDX_CHUNKID_LARGEOFFSETS);
        chunk_sizes.push(loff.len() * 8);
    }
    let mut offsets: Vec<u64> = Vec::with_capacity(num_chunks);
    let mut cur = (MIDX_HEADER_SIZE + toc_len) as u64;
    for sz in &chunk_sizes {
        offsets.push(cur);
        cur += *sz as u64;
    }
    let trailer_off = cur;

    let mut out = Vec::new();
    out.extend_from_slice(b"MIDX");
    out.push(1); // version
    out.push(match algo {
        HashAlgorithm::Sha1 => 1,
        HashAlgorithm::Sha256 => 2,
    });
    out.push(num_chunks as u8);
    out.push(0); // base layer count
    out.extend_from_slice(&(sorted.len() as u32).to_be_bytes());

    let mut toc = Vec::new();
    for i in 0..num_chunks {
        toc.extend_from_slice(&chunk_ids[i].to_be_bytes());
        toc.extend_from_slice(&offsets[i].to_be_bytes());
    }
    toc.extend_from_slice(&0u32.to_be_bytes());
    toc.extend_from_slice(&trailer_off.to_be_bytes());
    out.extend_from_slice(&toc);

    out.extend_from_slice(&pnam);
    // OIDF fanout.
    let mut counts = [0u32; 256];
    for (oid, _) in &union {
        counts[oid.as_slice()[0] as usize] += 1;
    }
    let mut acc = 0u32;
    for i in 0..256 {
        acc += counts[i];
        out.extend_from_slice(&acc.to_be_bytes());
    }
    for (oid, _) in &union {
        out.extend_from_slice(oid.as_slice());
    }
    out.extend_from_slice(&ooff);
    for lo in &loff {
        out.extend_from_slice(&lo.to_be_bytes());
    }

    let mut h = algo.hasher();
    h.update(&out);
    out.extend_from_slice(&h.finalize());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::{write_pack, PackObject};
    use git_object::{Object, ObjectKind};

    fn idx_for(objects: &[Object], algo: HashAlgorithm) -> PackIndex {
        let pos: Vec<PackObject> = objects
            .iter()
            .map(|o| PackObject {
                oid: o.compute_id(algo),
                kind: o.kind,
                data: o.data.clone(),
            })
            .collect();
        let (_pack, idx) = write_pack(&pos, algo).unwrap();
        PackIndex::parse(&idx, algo).unwrap()
    }

    #[test]
    fn write_read_find_round_trip() {
        let algo = HashAlgorithm::Sha1;
        let pack_a = [
            Object::from_data(ObjectKind::Blob, b"aaa".to_vec()),
            Object::from_data(ObjectKind::Blob, b"bbb".to_vec()),
        ];
        let pack_b = [
            Object::from_data(ObjectKind::Blob, b"ccc".to_vec()),
            Object::from_data(ObjectKind::Blob, b"ddd".to_vec()),
        ];
        let idx_a = idx_for(&pack_a, algo);
        let idx_b = idx_for(&pack_b, algo);
        let names = [
            ("pack-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(), idx_a),
            ("pack-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(), idx_b),
        ];
        let data = write_from_indexes(&names, algo).unwrap();
        let midx = Midx::parse(data, algo).unwrap();
        assert_eq!(midx.num_packs(), 2);
        assert_eq!(midx.num_objects(), 4);
        assert_eq!(midx.pack_names(), &["pack-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.idx", "pack-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.idx"]);
        assert!(midx.verify().is_ok());

        for obj in pack_a.iter().chain(&pack_b) {
            let oid = obj.compute_id(algo);
            let (pack_id, offset) = midx.find(&oid).unwrap();
            assert!(pack_id < 2);
            // The offset must match the pack it came from.
            let idx = if pack_id == 0 { &names[0].1 } else { &names[1].1 };
            assert_eq!(offset, idx.find(&oid).unwrap());
        }
        assert_eq!(midx.find(&Oid::new(HashAlgorithm::Sha1, &[0; 20])), None);
    }

    #[test]
    fn dedups_objects_across_packs() {
        let algo = HashAlgorithm::Sha1;
        let blob = Object::from_data(ObjectKind::Blob, b"shared".to_vec());
        let idx_a = idx_for(&[blob.clone()], algo);
        let idx_b = idx_for(&[blob.clone()], algo);
        let names = [
            ("pack-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(), idx_a),
            ("pack-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(), idx_b),
        ];
        let data = write_from_indexes(&names, algo).unwrap();
        let midx = Midx::parse(data, algo).unwrap();
        assert_eq!(midx.num_objects(), 1, "duplicate object should be deduplicated");
    }

    #[test]
    fn rejects_bad_signature() {
        let algo = HashAlgorithm::Sha1;
        let mut data = vec![0u8; 64];
        data[..4].copy_from_slice(b"XXXX");
        assert!(matches!(Midx::parse(data, algo), Err(MidxError::BadMagic)));
    }
}
