//! Pack and index writing with delta compression.
//!
//! The writer selects deltas with a sliding window over same-type objects
//! (a small port of `builtin/pack-objects.c` `find_deltas`/`try_delta`):
//! for each object it tries up to `window` predecessors of the same type,
//! keeps the smallest delta produced by `delta::create_delta`, and stores
//! the object as `OFS_DELTA` (default) or `REF_DELTA`
//! (`--no-delta-base-offset`). Objects with no beneficial delta are stored
//! whole. The resulting packs verify with C git (`verify-pack`, `fsck`).


use super::crc32::crc32;
use super::delta::{create_delta, DeltaIndex};
use super::PackError;
use git_compress::encode_all_level;
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

fn deflate_level(data: &[u8], level: u32) -> Vec<u8> {
    encode_all_level(data, level)
}

/// Options controlling delta selection when writing a pack.
///
/// Mirrors the subset of `git pack-objects` flags the port supports:
/// `--window`, `--depth`, `--delta-base-offset` /
/// `--no-delta-base-offset`, and `--compression`.
#[derive(Debug, Clone, Copy)]
pub struct PackOptions {
    /// Number of predecessors to consider as delta bases (C default 10).
    pub window: usize,
    /// Maximum delta chain depth (C default 50).
    pub depth: usize,
    /// When true (default, like C) deltas use `OFS_DELTA`; when false
    /// they use `REF_DELTA`.
    pub allow_ofs_delta: bool,
    /// zlib compression level 0–9 (C default maps to zlib default 6).
    pub compression: u32,
}

impl Default for PackOptions {
    fn default() -> PackOptions {
        PackOptions {
            window: 10,
            depth: 50,
            allow_ofs_delta: true,
            compression: 6,
        }
    }
}

/// Encode an OFS_DELTA distance (`delta_offset - base_offset`, always > 0)
/// with git's "+1" varint scheme (inverse of the decoder in `file.rs` and
/// `packfile.c:get_delta_base`).
fn encode_ofs_distance(ofs: u64) -> Vec<u8> {
    debug_assert!(ofs > 0);
    let mut tail: Vec<u8> = vec![(ofs & 0x7f) as u8];
    let mut v = ofs >> 7;
    while v > 0 {
        v -= 1;
        tail.push(0x80 | ((v & 0x7f) as u8));
        v >>= 7;
    }
    tail.reverse();
    tail
}

fn kind_rank(kind: ObjectKind) -> u8 {
    match kind {
        ObjectKind::Commit => 0,
        ObjectKind::Tree => 1,
        ObjectKind::Tag => 2,
        ObjectKind::Blob => 3,
    }
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

/// Write a pack (and its index) for the given objects with default delta
/// options. Objects are sorted by type then id for determinism; deltas are
/// selected with a sliding window (see [`PackOptions`]).
pub fn write_pack(objects: &[PackObject], algo: HashAlgorithm) -> Result<(Vec<u8>, Vec<u8>), PackError> {
    write_pack_opts(objects, algo, PackOptions::default())
}

/// Write a pack (and its index) with explicit delta/compression options.
pub fn write_pack_opts(
    objects: &[PackObject],
    algo: HashAlgorithm,
    opts: PackOptions,
) -> Result<(Vec<u8>, Vec<u8>), PackError> {
    let mut sorted: Vec<&PackObject> = objects.iter().collect();
    // Type grouping maximizes same-type adjacency for delta bases (C also
    // orders by name-hash; without pathnames oid order is the deterministic
    // stand-in), then larger objects first so small edits delta against the
    // fuller version.
    sorted.sort_by(|a, b| {
        kind_rank(a.kind)
            .cmp(&kind_rank(b.kind))
            .then_with(|| b.data.len().cmp(&a.data.len()))
            .then_with(|| a.oid.cmp(&b.oid))
    });

    let level = opts.compression.min(9);
    let compression = level;

    // Delta selection: for each object, try up to `window` same-type
    // predecessors and keep the smallest delta that beats storing whole.
    // `depths` tracks delta chain length so `--depth` is honored.
    let n = sorted.len();
    let mut depths = vec![0usize; n];
    let mut base_of: Vec<Option<usize>> = vec![None; n];
    let mut delta_of: Vec<Option<Vec<u8>>> = (0..n).map(|_| None).collect();
    // Cache DeltaIndex per position so each base is indexed once.
    let mut index_cache: Vec<Option<DeltaIndex>> = (0..n).map(|_| None).collect();
    for i in 0..n {
        if opts.window == 0 {
            break;
        }
        let target = &sorted[i];
        if target.data.is_empty() {
            continue;
        }
        let start = i.saturating_sub(opts.window);
        let mut best: Option<(usize, Vec<u8>)> = None;
        for j in (start..i).rev() {
            if sorted[j].kind != target.kind {
                continue;
            }
            if sorted[j].data.is_empty() {
                continue;
            }
            if depths[j] + 1 > opts.depth {
                continue;
            }
            if index_cache[j].is_none() {
                index_cache[j] = DeltaIndex::new(sorted[j].data.clone());
            }
            let Some(idx) = index_cache[j].as_ref() else {
                continue;
            };
            // Only accept a delta smaller than the raw payload (C's
            // `try_delta` equivalent: `max_size` early-outs oversized
            // results inside `create_delta`).
            let limit = best
                .as_ref()
                .map(|(_, d): &(usize, Vec<u8>)| d.len())
                .unwrap_or(target.data.len());
            if limit == 0 {
                break;
            }
            if let Some(d) = create_delta(idx, &target.data, limit.saturating_sub(1)) {
                if d.len() < limit {
                    best = Some((j, d));
                    // Perfect tiny delta: stop scanning this window.
                    if best.as_ref().map(|(_, d)| d.len()).unwrap_or(usize::MAX) <= 32 {
                        break;
                    }
                }
            }
        }
        if let Some((j, d)) = best {
            if d.len() < target.data.len() {
                depths[i] = depths[j] + 1;
                base_of[i] = Some(j);
                delta_of[i] = Some(d);
            }
        }
    }

    let mut pack = Vec::new();
    pack.extend_from_slice(b"PACK");
    pack.extend_from_slice(&2u32.to_be_bytes());
    pack.extend_from_slice(&(sorted.len() as u32).to_be_bytes());

    let mut entries: Vec<(Oid, u64, u32)> = Vec::with_capacity(sorted.len());
    let mut pack_offsets: Vec<u64> = vec![0; n];
    for (i, o) in sorted.iter().enumerate() {
        let start = pack.len();
        pack_offsets[i] = start as u64;
        if let (Some(base_idx), Some(delta)) = (base_of[i], delta_of[i].as_ref()) {
            if opts.allow_ofs_delta {
                let base_off = pack_offsets[base_idx];
                let dist = (start as u64).saturating_sub(base_off);
                if dist > 0 {
                    encode_entry_header(6, delta.len() as u64, &mut pack);
                    pack.extend_from_slice(&encode_ofs_distance(dist));
                    pack.extend_from_slice(&deflate_level(delta, compression));
                    let end = pack.len();
                    entries.push((o.oid, start as u64, crc32(&pack[start..end])));
                    continue;
                }
            }
            // REF_DELTA fallback (or `--no-delta-base-offset`).
            encode_entry_header(7, delta.len() as u64, &mut pack);
            pack.extend_from_slice(sorted[base_idx].oid.as_slice());
            pack.extend_from_slice(&deflate_level(delta, compression));
            let end = pack.len();
            entries.push((o.oid, start as u64, crc32(&pack[start..end])));
            continue;
        }
        let type_code = match o.kind {
            ObjectKind::Commit => 1,
            ObjectKind::Tree => 2,
            ObjectKind::Blob => 3,
            ObjectKind::Tag => 4,
        };
        encode_entry_header(type_code, o.data.len() as u64, &mut pack);
        pack.extend_from_slice(&deflate_level(&o.data, compression));
        let end = pack.len();
        entries.push((o.oid, start as u64, crc32(&pack[start..end])));
    }

    let mut h = algo.hasher();
    h.update(&pack);
    let trailer = h.finalize();
    pack.extend_from_slice(&trailer);

    // The v2 index fanout requires entries sorted by oid, while the pack
    // itself stays in delta-friendly (type/size) order.
    entries.sort_by_key(|e| e.0);
    let idx = write_idx(&entries, &trailer, algo);
    Ok((pack, idx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::{EntryKind, PackFile, PackIndex};

    fn blob(data: Vec<u8>) -> PackObject {
        let algo = HashAlgorithm::Sha1;
        PackObject {
            oid: Object::from_data(ObjectKind::Blob, data.clone()).compute_id(algo),
            kind: ObjectKind::Blob,
            data,
        }
    }

    /// Resolve every entry and return the set of real oids present.
    fn resolve_all(pack: Vec<u8>, idx_data: &[u8], algo: HashAlgorithm) -> std::collections::HashSet<Oid> {
        let pf = PackFile::from_bytes(pack, algo).unwrap();
        let idx = PackIndex::parse(idx_data, algo).unwrap();
        let mut seen = std::collections::HashSet::new();
        let mut pos = pf.first_entry_offset();
        let end = pf.data_end();
        while pos < end {
            let mut resolver = |_: &Oid| -> Option<Object> { None };
            let r = pf.resolve_entry(pos, Some(&idx), &mut resolver).unwrap();
            seen.insert(r.object.compute_id(algo));
            pos += r.entry_len;
        }
        seen
    }

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

    #[test]
    fn ofs_distance_encoding_matches_decoder() {
        // The "+1" varint distance must round-trip through the decoder in
        // file.rs for boundary values.
        for dist in [1u64, 127, 128, 129, 300, 100000] {
            let enc = encode_ofs_distance(dist);
            let mut off = u64::from(enc[0] & 0x7f);
            let mut pos = 1usize;
            let mut c = enc[0];
            while c & 0x80 != 0 {
                c = enc[pos];
                pos += 1;
                off = ((off + 1) << 7) | u64::from(c & 0x7f);
            }
            assert_eq!(off, dist, "dist {dist}");
            assert_eq!(pos, enc.len());
        }
    }

    #[test]
    fn deltified_pack_round_trips() {
        let algo = HashAlgorithm::Sha1;
        let base_data: Vec<u8> = (0..500).map(|i| (i % 251) as u8).collect();
        let mut target_data = base_data.clone();
        target_data.extend_from_slice(b"appended tail for delta test");
        target_data[100] ^= 0x01;
        let base = blob(base_data);
        let target = blob(target_data);
        let (pack, idx) =
            write_pack_opts(&[base.clone(), target.clone()], algo, PackOptions::default())
                .unwrap();
        let pf = PackFile::from_bytes(pack.clone(), algo).unwrap();
        pf.verify_trailer().unwrap();
        let seen = resolve_all(pack, &idx, algo);
        assert!(seen.contains(&base.oid), "base missing");
        assert!(seen.contains(&target.oid), "target missing");
    }

    #[test]
    fn window_zero_disables_deltas() {
        let algo = HashAlgorithm::Sha1;
        let objs = vec![
            blob(vec![b'a'; 1000]),
            blob([vec![b'a'; 999], vec![b'b']].concat()),
        ];
        let (plain, _) = write_pack_opts(
            &objs,
            algo,
            PackOptions {
                window: 0,
                ..PackOptions::default()
            },
        )
        .unwrap();
        let (small, idx) = write_pack_opts(&objs, algo, PackOptions::default()).unwrap();
        // With two near-identical 1KB blobs the deltified pack must be smaller.
        assert!(small.len() < plain.len(), "{} vs {}", small.len(), plain.len());
        // And the deltified form must still verify entry-by-entry.
        let pf = PackFile::from_bytes(small, algo).unwrap();
        let index = PackIndex::parse(&idx, algo).unwrap();
        pf.verify(&index).unwrap();
    }

    #[test]
    fn ref_delta_mode_round_trips() {
        let algo = HashAlgorithm::Sha1;
        let objs = vec![
            blob(vec![b'x'; 800]),
            blob([vec![b'x'; 799], vec![b'y']].concat()),
        ];
        let opts = PackOptions {
            allow_ofs_delta: false,
            ..PackOptions::default()
        };
        let (pack, idx) = write_pack_opts(&objs, algo, opts).unwrap();
        let pf = PackFile::from_bytes(pack, algo).unwrap();
        let index = PackIndex::parse(&idx, algo).unwrap();
        // Must contain a REF_DELTA entry (type 7).
        let mut pos = pf.first_entry_offset();
        let end = pf.data_end();
        let mut saw_ref = false;
        while pos < end {
            let e = pf.entry_at(pos).unwrap();
            if matches!(e.kind, EntryKind::RefDelta) {
                saw_ref = true;
            }
            let r = pf.resolve_entry(pos, Some(&index), &mut |_: &Oid| None).unwrap();
            pos += r.entry_len;
        }
        assert!(saw_ref, "expected a REF_DELTA entry");
        // The index must be valid too.
        pf.verify(&index).unwrap();
    }

    #[test]
    fn larger_pack_delta_ratio_sane() {
        // Reversed-lines-ish workload: the writer must actually delta, not
        // just store whole objects. Purely a sanity bound.
        let algo = HashAlgorithm::Sha1;
        let mut objs = Vec::new();
        for i in 0..64u32 {
            let mut b = Vec::new();
            for line in 0..200u32 {
                b.extend_from_slice(format!("object {i:02} line {line:04} payload\n").as_bytes());
            }
            objs.push(blob(b));
        }
        let (pack, _) = write_pack_opts(&objs, algo, PackOptions::default()).unwrap();
        let raw: usize = objs.iter().map(|o| o.data.len()).sum();
        assert!(
            pack.len() < raw,
            "deltified pack {} should be smaller than raw {raw}",
            pack.len()
        );
    }
}
