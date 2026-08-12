//! Pack and index writing.
//!
//! Objects are written without delta compression (each entry stores its full
//! payload). The resulting packs are fully valid and can be read and verified
//! by C git (`git index-pack --verify`, `git verify-pack`). Delta selection is
//! a later optimization.

use std::io::Write;

use super::crc32::crc32;
use super::PackError;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use git_hash::{HashAlgorithm, Oid};
use git_object::{Object, ObjectKind};

/// An object ready to be packed.
#[derive(Debug, Clone)]
pub struct PackObject {
    pub oid: Oid,
    pub kind: ObjectKind,
    pub data: Vec<u8>,
}

impl From<&Object> for PackObject {
    fn from(o: &Object) -> PackObject {
        PackObject {
            oid: o.compute_id(HashAlgorithm::Sha1),
            kind: o.kind,
            data: o.data.clone(),
        }
    }
}

/// Encode a pack entry header (type + size, plain 7-bit chunks).
fn encode_entry_header(type_code: u8, size: u64, out: &mut Vec<u8>) {
    let mut byte = (type_code << 4) | (size & 0x0f) as u8;
    let mut size = size >> 4;
    if size > 0 {
        byte |= 0x80;
    }
    out.push(byte);
    while size > 0 {
        let mut b = (size & 0x7f) as u8;
        size >>= 7;
        if size > 0 {
            b |= 0x80;
        }
        out.push(b);
    }
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(data).expect("deflate");
    e.finish().expect("deflate")
}

/// Build a v2 index from (oid, offset, crc) entries sorted by oid.
pub fn write_idx(
    entries: &[(Oid, u64, u32)],
    pack_trailer: &[u8],
    algo: HashAlgorithm,
) -> Vec<u8> {
    let mut idx = Vec::new();
    idx.extend_from_slice(b"\xfftOc");
    idx.extend_from_slice(&2u32.to_be_bytes());

    let mut counts = [0u32; 256];
    for (oid, _, _) in entries {
        counts[oid.as_slice()[0] as usize] += 1;
    }
    let mut acc = 0u32;
    for i in 0..256 {
        acc += counts[i];
        idx.extend_from_slice(&acc.to_be_bytes());
    }

    for (oid, _, _) in entries {
        idx.extend_from_slice(oid.as_slice());
    }
    for (_, _, crc) in entries {
        idx.extend_from_slice(&crc.to_be_bytes());
    }
    let mut large: Vec<u64> = Vec::new();
    for (_, off, _) in entries {
        if *off < 0x8000_0000 {
            idx.extend_from_slice(&(*off as u32).to_be_bytes());
        } else {
            idx.extend_from_slice(&(0x8000_0000 | large.len() as u32).to_be_bytes());
            large.push(*off);
        }
    }
    for lo in &large {
        idx.extend_from_slice(&lo.to_be_bytes());
    }

    idx.extend_from_slice(pack_trailer);
    let mut h = algo.hasher();
    h.update(&idx);
    idx.extend_from_slice(&h.finalize());
    idx
}

/// Write a pack (and its index) for the given objects. Objects are sorted by
/// oid for determinism.
pub fn write_pack(objects: &[PackObject], algo: HashAlgorithm) -> Result<(Vec<u8>, Vec<u8>), PackError> {
    let mut sorted: Vec<&PackObject> = objects.iter().collect();
    sorted.sort_by_key(|o| o.oid);

    let mut pack = Vec::new();
    pack.extend_from_slice(b"PACK");
    pack.extend_from_slice(&2u32.to_be_bytes());
    pack.extend_from_slice(&(sorted.len() as u32).to_be_bytes());

    let mut entries: Vec<(Oid, u64, u32)> = Vec::with_capacity(sorted.len());
    for o in &sorted {
        let start = pack.len();
        let type_code = match o.kind {
            ObjectKind::Commit => 1,
            ObjectKind::Tree => 2,
            ObjectKind::Blob => 3,
            ObjectKind::Tag => 4,
        };
        encode_entry_header(type_code, o.data.len() as u64, &mut pack);
        pack.extend_from_slice(&deflate(&o.data));
        let end = pack.len();
        entries.push((o.oid, start as u64, crc32(&pack[start..end])));
    }

    let mut h = algo.hasher();
    h.update(&pack);
    let trailer = h.finalize();
    pack.extend_from_slice(&trailer);

    let idx = write_idx(&entries, &trailer, algo);
    Ok((pack, idx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_header_round_trips() {
        // Encoding of a header that needs two bytes.
        let mut out = Vec::new();
        encode_entry_header(3, 0x1f, &mut out); // blob, size 31 (low4=15, then 1)
        assert_eq!(out, vec![0x80 | 3 << 4 | 0x0f, 0x01]);
        let mut out = Vec::new();
        encode_entry_header(1, 5, &mut out);
        assert_eq!(out, vec![1 << 4 | 5]);
    }
}
