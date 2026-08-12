//! Pack index (v2) reading.

use super::PackError;
use git_hash::{HashAlgorithm, Oid};

/// A parsed pack index (v2).
#[derive(Debug, Clone)]
pub struct PackIndex {
    algo: HashAlgorithm,
    fanout: [u32; 256],
    oids: Vec<Oid>,
    crcs: Vec<u32>,
    offsets: Vec<u64>,
    pack_checksum: Vec<u8>,
}

impl PackIndex {
    /// Parse a full index file (including both trailing checksums).
    pub fn parse(data: &[u8], algo: HashAlgorithm) -> Result<PackIndex, PackError> {
        let raw = algo.raw_len();
        let header_size = 8 + 1024;
        if data.len() < header_size + raw * 2 {
            return Err(PackError::Truncated);
        }
        if &data[0..4] != b"\xfftOc" {
            return Err(PackError::BadMagic);
        }
        if u32::from_be_bytes(data[4..8].try_into().unwrap()) != 2 {
            return Err(PackError::BadVersion);
        }

        let mut fanout = [0u32; 256];
        for i in 0..256 {
            fanout[i] = u32::from_be_bytes(data[8 + i * 4..12 + i * 4].try_into().unwrap());
        }
        let n = fanout[255] as usize;
        let mut pos = header_size;

        // Verify the declared size is consistent before slicing.
        let table_bytes = n
            .checked_mul(raw)
            .and_then(|x| x.checked_add(n * 4))
            .and_then(|x| x.checked_add(n * 4))
            .ok_or(PackError::Truncated)?;
        if data.len() < pos + table_bytes + raw * 2 {
            return Err(PackError::Truncated);
        }

        let mut oids = Vec::with_capacity(n);
        for _ in 0..n {
            let start = pos;
            oids.push(Oid::new(algo, &data[start..start + raw]));
            pos += raw;
        }
        let mut crcs = Vec::with_capacity(n);
        for _ in 0..n {
            crcs.push(u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()));
            pos += 4;
        }
        let mut large_count = 0usize;
        let mut offsets4 = Vec::with_capacity(n);
        for _ in 0..n {
            let o = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap());
            offsets4.push(o);
            if o & 0x8000_0000 != 0 {
                large_count += 1;
            }
            pos += 4;
        }
        if data.len() < pos + large_count * 8 + raw * 2 {
            return Err(PackError::Truncated);
        }
        let mut offsets = Vec::with_capacity(n);
        let mut large_idx = 0usize;
        for &o in &offsets4 {
            if o & 0x8000_0000 != 0 {
                let li = pos + large_idx * 8;
                offsets.push(u64::from_be_bytes(data[li..li + 8].try_into().unwrap()));
                large_idx += 1;
            } else {
                offsets.push(u64::from(o));
            }
        }
        pos += large_count * 8;

        let pack_checksum = data[pos..pos + raw].to_vec();
        pos += raw;
        if pos + raw != data.len() {
            return Err(PackError::Corrupt("trailing bytes in index".to_string()));
        }
        let idx_checksum = &data[pos..pos + raw];

        // Verify the index checksum (hash of everything before it).
        let mut h = algo.hasher();
        h.update(&data[..pos]);
        if h.finalize() != idx_checksum {
            return Err(PackError::BadChecksum);
        }

        Ok(PackIndex {
            algo,
            fanout,
            oids,
            crcs,
            offsets,
            pack_checksum,
        })
    }

    /// The number of objects in the index.
    pub fn len(&self) -> usize {
        self.oids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.oids.is_empty()
    }

    /// The object ids, sorted.
    pub fn oids(&self) -> &[Oid] {
        &self.oids
    }

    /// The id at position `i` (index order).
    pub fn oid_at(&self, i: usize) -> &Oid {
        &self.oids[i]
    }

    /// The crc at position `i`.
    pub fn crc_at(&self, i: usize) -> u32 {
        self.crcs[i]
    }

    /// The pack offset at position `i`.
    pub fn offset_at(&self, i: usize) -> Result<u64, PackError> {
        self.offsets.get(i).copied().ok_or(PackError::NotFound)
    }

    /// The pack trailer (the pack's trailing hash) as recorded in the index.
    pub fn pack_checksum(&self) -> &[u8] {
        &self.pack_checksum
    }

    /// Look up the pack offset for `oid`.
    pub fn find(&self, oid: &Oid) -> Option<u64> {
        if oid.algorithm() != self.algo {
            return None;
        }
        let first = oid.as_slice()[0] as usize;
        let lo = if first == 0 {
            0
        } else {
            self.fanout[first - 1] as usize
        };
        let hi = self.fanout[first] as usize;
        let slice = &self.oids[lo..hi];
        let idx = slice.binary_search(oid).ok()?;
        Some(self.offsets[lo + idx])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::crc32::crc32;
    use crate::pack::write::{write_pack, PackObject};
    use git_object::{Object, ObjectKind};

    fn objs() -> Vec<Object> {
        vec![
            Object::from_data(ObjectKind::Blob, b"alpha".to_vec()),
            Object::from_data(ObjectKind::Blob, b"beta beta".to_vec()),
            Object::from_data(ObjectKind::Tree, b"100644 one\0abc".as_slice().to_vec()),
            Object::from_data(ObjectKind::Commit, b"tree 0000\nmessage\n".to_vec()),
        ]
    }

    fn to_pack_objects(objects: &[Object], algo: HashAlgorithm) -> Vec<PackObject> {
        objects
            .iter()
            .map(|o| PackObject {
                oid: o.compute_id(algo),
                kind: o.kind,
                data: o.data.clone(),
            })
            .collect()
    }

    #[test]
    fn idx_round_trip_and_lookup() {
        let algo = HashAlgorithm::Sha1;
        let objects = objs();
        let (pack, idx_bytes) = write_pack(&to_pack_objects(&objects, algo), algo).unwrap();
        let idx = PackIndex::parse(&idx_bytes, algo).unwrap();
        assert_eq!(idx.len(), objects.len());
        assert_eq!(idx.pack_checksum(), &pack[pack.len() - algo.raw_len()..]);

        for o in &objects {
            let oid = o.compute_id(algo);
            let off = idx.find(&oid).unwrap();
            assert_eq!(idx.offset_at(idx.oids().binary_search(&oid).unwrap()).unwrap(), off);
        }
        // Missing id not found.
        assert_eq!(idx.find(HashAlgorithm::Sha1.null_oid()), None);
    }

    #[test]
    fn idx_crcs_match_entries() {
        let algo = HashAlgorithm::Sha1;
        let objects = objs();
        let (pack, idx_bytes) = write_pack(&to_pack_objects(&objects, algo), algo).unwrap();
        let idx = PackIndex::parse(&idx_bytes, algo).unwrap();
        let pf = crate::pack::file::PackFile::from_bytes(pack.clone(), algo).unwrap();
        let mut resolver = |_: &Oid| -> Option<Object> { None };
        for i in 0..idx.len() {
            let off = idx.offset_at(i).unwrap() as usize;
            let resolved = pf.resolve_entry(off, Some(&idx), &mut resolver).unwrap();
            let entry_crc = crc32(&pack[off..off + resolved.entry_len]);
            assert_eq!(entry_crc, idx.crc_at(i));
            assert_eq!(resolved.object.compute_id(algo), *idx.oid_at(i));
        }
    }
}
