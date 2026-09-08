//! Delta application (`patch-delta.c`) and creation (`diff-delta.c`).
//!
//! A delta blob is: source-size varint, result-size varint, then a series of
//! copy (`0x80` set) and insert instructions.

use super::PackError;

/// Decode a delta header size field (git's "+1" varint scheme).
fn get_delta_hdr_size(data: &mut &[u8]) -> Option<u64> {
    let mut cmd = *data.first()?;
    *data = &data[1..];
    let mut val = u64::from(cmd & 0x7f);
    while cmd & 0x80 != 0 {
        cmd = *data.first()?;
        *data = &data[1..];
        val = ((val + 1) << 7) | u64::from(cmd & 0x7f);
        if val.leading_zeros() < 7 {
            return None; // overflow
        }
    }
    Some(val)
}

/// Apply `delta` on top of `base`, returning the reconstructed object.
pub fn apply_delta(base: &[u8], delta: &[u8]) -> Result<Vec<u8>, PackError> {
    let mut d = delta;
    let src_size = get_delta_hdr_size(&mut d).ok_or(PackError::BadDelta)?;
    let dst_size = get_delta_hdr_size(&mut d).ok_or(PackError::BadDelta)?;
    if src_size as usize != base.len() {
        return Err(PackError::BadDelta);
    }
    if dst_size > 0x7fff_ffff {
        return Err(PackError::BadDelta);
    }
    let dst_size = dst_size as usize;
    let mut out = Vec::with_capacity(dst_size);

    while !d.is_empty() {
        let cmd = d[0];
        d = &d[1..];
        if cmd & 0x80 != 0 {
            let mut cp_off: u64 = 0;
            let mut cp_size: u64 = 0;
            for i in 0..4 {
                if cmd & (1 << i) != 0 {
                    cp_off |= u64::from(d[0]) << (i * 8);
                    d = &d[1..];
                }
            }
            for i in 0..3 {
                if cmd & (1 << (4 + i)) != 0 {
                    cp_size |= u64::from(d[0]) << (i * 8);
                    d = &d[1..];
                }
            }
            // A size of zero means a full 0x10000-byte copy.
            if cp_size == 0 {
                cp_size = 0x1_0000;
            }
            let off = cp_off as usize;
            let sz = cp_size as usize;
            if off.checked_add(sz).is_none() || off + sz > base.len() {
                return Err(PackError::BadDelta);
            }
            if out.len().checked_add(sz).is_none() || out.len() + sz > dst_size {
                return Err(PackError::BadDelta);
            }
            out.extend_from_slice(&base[off..off + sz]);
        } else if cmd != 0 {
            let insn = (cmd & 0x7f) as usize;
            if d.len() < insn || out.len() + insn > dst_size {
                return Err(PackError::BadDelta);
            }
            out.extend_from_slice(&d[..insn]);
            d = &d[insn..];
        } else {
            return Err(PackError::BadDelta);
        }
    }

    if out.len() != dst_size {
        return Err(PackError::BadDelta);
    }
    Ok(out)
}

/// Maximum size for any opcode sequence, including the initial header
/// plus Rabin window plus biggest copy (C's `MAX_OP_SIZE`).
pub const MAX_OP_SIZE: usize = 5 + 5 + 1 + RABIN_WINDOW + 7;

/// Maximum hash entry list for the same hash bucket (C's `HASH_LIMIT`).
const HASH_LIMIT: u32 = 64;

const RABIN_SHIFT: u32 = 23;
const RABIN_WINDOW: usize = 16;

#[rustfmt::skip]
const T: [u32; 256] = [
    0x00000000, 0xab59b4d1, 0x56b369a2, 0xfdeadd73, 0x063f6795, 0xad66d344,
    0x508c0e37, 0xfbd5bae6, 0x0c7ecf2a, 0xa7277bfb, 0x5acda688, 0xf1941259,
    0x0a41a8bf, 0xa1181c6e, 0x5cf2c11d, 0xf7ab75cc, 0x18fd9e54, 0xb3a42a85,
    0x4e4ef7f6, 0xe5174327, 0x1ec2f9c1, 0xb59b4d10, 0x48719063, 0xe32824b2,
    0x1483517e, 0xbfdae5af, 0x423038dc, 0xe9698c0d, 0x12bc36eb, 0xb9e5823a,
    0x440f5f49, 0xef56eb98, 0x31fb3ca8, 0x9aa28879, 0x6748550a, 0xcc11e1db,
    0x37c45b3d, 0x9c9defec, 0x6177329f, 0xca2e864e, 0x3d85f382, 0x96dc4753,
    0x6b369a20, 0xc06f2ef1, 0x3bba9417, 0x90e320c6, 0x6d09fdb5, 0xc6504964,
    0x2906a2fc, 0x825f162d, 0x7fb5cb5e, 0xd4ec7f8f, 0x2f39c569, 0x846071b8,
    0x798aaccb, 0xd2d3181a, 0x25786dd6, 0x8e21d907, 0x73cb0474, 0xd892b0a5,
    0x23470a43, 0x881ebe92, 0x75f463e1, 0xdeadd730, 0x63f67950, 0xc8afcd81,
    0x354510f2, 0x9e1ca423, 0x65c91ec5, 0xce90aa14, 0x337a7767, 0x9823c3b6,
    0x6f88b67a, 0xc4d102ab, 0x393bdfd8, 0x92626b09, 0x69b7d1ef, 0xc2ee653e,
    0x3f04b84d, 0x945d0c9c, 0x7b0be704, 0xd05253d5, 0x2db88ea6, 0x86e13a77,
    0x7d348091, 0xd66d3440, 0x2b87e933, 0x80de5de2, 0x7775282e, 0xdc2c9cff,
    0x21c6418c, 0x8a9ff55d, 0x714a4fbb, 0xda13fb6a, 0x27f92619, 0x8ca092c8,
    0x520d45f8, 0xf954f129, 0x04be2c5a, 0xafe7988b, 0x5432226d, 0xff6b96bc,
    0x02814bcf, 0xa9d8ff1e, 0x5e738ad2, 0xf52a3e03, 0x08c0e370, 0xa39957a1,
    0x584ced47, 0xf3155996, 0x0eff84e5, 0xa5a63034, 0x4af0dbac, 0xe1a96f7d,
    0x1c43b20e, 0xb71a06df, 0x4ccfbc39, 0xe79608e8, 0x1a7cd59b, 0xb125614a,
    0x468e1486, 0xedd7a057, 0x103d7d24, 0xbb64c9f5, 0x40b17313, 0xebe8c7c2,
    0x16021ab1, 0xbd5bae60, 0x6cb54671, 0xc7ecf2a0, 0x3a062fd3, 0x915f9b02,
    0x6a8a21e4, 0xc1d39535, 0x3c394846, 0x9760fc97, 0x60cb895b, 0xcb923d8a,
    0x3678e0f9, 0x9d215428, 0x66f4eece, 0xcdad5a1f, 0x3047876c, 0x9b1e33bd,
    0x7448d825, 0xdf116cf4, 0x22fbb187, 0x89a20556, 0x7277bfb0, 0xd92e0b61,
    0x24c4d612, 0x8f9d62c3, 0x7836170f, 0xd36fa3de, 0x2e857ead, 0x85dcca7c,
    0x7e09709a, 0xd550c44b, 0x28ba1938, 0x83e3ade9, 0x5d4e7ad9, 0xf617ce08,
    0x0bfd137b, 0xa0a4a7aa, 0x5b711d4c, 0xf028a99d, 0x0dc274ee, 0xa69bc03f,
    0x5130b5f3, 0xfa690122, 0x0783dc51, 0xacda6880, 0x570fd266, 0xfc5666b7,
    0x01bcbbc4, 0xaae50f15, 0x45b3e48d, 0xeeea505c, 0x13008d2f, 0xb85939fe,
    0x438c8318, 0xe8d537c9, 0x153feaba, 0xbe665e6b, 0x49cd2ba7, 0xe2949f76,
    0x1f7e4205, 0xb427f6d4, 0x4ff24c32, 0xe4abf8e3, 0x19412590, 0xb2189141,
    0x0f433f21, 0xa41a8bf0, 0x59f05683, 0xf2a9e252, 0x097c58b4, 0xa225ec65,
    0x5fcf3116, 0xf49685c7, 0x033df00b, 0xa86444da, 0x558e99a9, 0xfed72d78,
    0x0502979e, 0xae5b234f, 0x53b1fe3c, 0xf8e84aed, 0x17bea175, 0xbce715a4,
    0x410dc8d7, 0xea547c06, 0x1181c6e0, 0xbad87231, 0x4732af42, 0xec6b1b93,
    0x1bc06e5f, 0xb099da8e, 0x4d7307fd, 0xe62ab32c, 0x1dff09ca, 0xb6a6bd1b,
    0x4b4c6068, 0xe015d4b9, 0x3eb80389, 0x95e1b758, 0x680b6a2b, 0xc352defa,
    0x3887641c, 0x93ded0cd, 0x6e340dbe, 0xc56db96f, 0x32c6cca3, 0x999f7872,
    0x6475a501, 0xcf2c11d0, 0x34f9ab36, 0x9fa01fe7, 0x624ac294, 0xc9137645,
    0x26459ddd, 0x8d1c290c, 0x70f6f47f, 0xdbaf40ae, 0x207afa48, 0x8b234e99,
    0x76c993ea, 0xdd90273b, 0x2a3b52f7, 0x8162e626, 0x7c883b55, 0xd7d18f84,
    0x2c043562, 0x875d81b3, 0x7ab75cc0, 0xd1eee811,
];

#[rustfmt::skip]
const U: [u32; 256] = [
    0x00000000, 0x7eb5200d, 0x5633f4cb, 0x2886d4c6, 0x073e5d47, 0x798b7d4a,
    0x510da98c, 0x2fb88981, 0x0e7cba8e, 0x70c99a83, 0x584f4e45, 0x26fa6e48,
    0x0942e7c9, 0x77f7c7c4, 0x5f711302, 0x21c4330f, 0x1cf9751c, 0x624c5511,
    0x4aca81d7, 0x347fa1da, 0x1bc7285b, 0x65720856, 0x4df4dc90, 0x3341fc9d,
    0x1285cf92, 0x6c30ef9f, 0x44b63b59, 0x3a031b54, 0x15bb92d5, 0x6b0eb2d8,
    0x4388661e, 0x3d3d4613, 0x39f2ea38, 0x4747ca35, 0x6fc11ef3, 0x11743efe,
    0x3eccb77f, 0x40799772, 0x68ff43b4, 0x164a63b9, 0x378e50b6, 0x493b70bb,
    0x61bda47d, 0x1f088470, 0x30b00df1, 0x4e052dfc, 0x6683f93a, 0x1836d937,
    0x250b9f24, 0x5bbebf29, 0x73386bef, 0x0d8d4be2, 0x2235c263, 0x5c80e26e,
    0x740636a8, 0x0ab316a5, 0x2b7725aa, 0x55c205a7, 0x7d44d161, 0x03f1f16c,
    0x2c4978ed, 0x52fc58e0, 0x7a7a8c26, 0x04cfac2b, 0x73e5d470, 0x0d50f47d,
    0x25d620bb, 0x5b6300b6, 0x74db8937, 0x0a6ea93a, 0x22e87dfc, 0x5c5d5df1,
    0x7d996efe, 0x032c4ef3, 0x2baa9a35, 0x551fba38, 0x7aa733b9, 0x041213b4,
    0x2c94c772, 0x5221e77f, 0x6f1ca16c, 0x11a98161, 0x392f55a7, 0x479a75aa,
    0x6822fc2b, 0x1697dc26, 0x3e1108e0, 0x40a428ed, 0x61601be2, 0x1fd53bef,
    0x3753ef29, 0x49e6cf24, 0x665e46a5, 0x18eb66a8, 0x306db26e, 0x4ed89263,
    0x4a173e48, 0x34a21e45, 0x1c24ca83, 0x6291ea8e, 0x4d29630f, 0x339c4302,
    0x1b1a97c4, 0x65afb7c9, 0x446b84c6, 0x3adea4cb, 0x1258700d, 0x6ced5000,
    0x4355d981, 0x3de0f98c, 0x15662d4a, 0x6bd30d47, 0x56ee4b54, 0x285b6b59,
    0x00ddbf9f, 0x7e689f92, 0x51d01613, 0x2f65361e, 0x07e3e2d8, 0x7956c2d5,
    0x5892f1da, 0x2627d1d7, 0x0ea10511, 0x7014251c, 0x5facac9d, 0x21198c90,
    0x099f5856, 0x772a785b, 0x4c921c31, 0x32273c3c, 0x1aa1e8fa, 0x6414c8f7,
    0x4bac4176, 0x3519617b, 0x1d9fb5bd, 0x632a95b0, 0x42eea6bf, 0x3c5b86b2,
    0x14dd5274, 0x6a687279, 0x45d0fbf8, 0x3b65dbf5, 0x13e30f33, 0x6d562f3e,
    0x506b692d, 0x2ede4920, 0x06589de6, 0x78edbdeb, 0x5755346a, 0x29e01467,
    0x0166c0a1, 0x7fd3e0ac, 0x5e17d3a3, 0x20a2f3ae, 0x08242768, 0x76910765,
    0x59298ee4, 0x279caee9, 0x0f1a7a2f, 0x71af5a22, 0x7560f609, 0x0bd5d604,
    0x235302c2, 0x5de622cf, 0x725eab4e, 0x0ceb8b43, 0x246d5f85, 0x5ad87f88,
    0x7b1c4c87, 0x05a96c8a, 0x2d2fb84c, 0x539a9841, 0x7c2211c0, 0x029731cd,
    0x2a11e50b, 0x54a4c506, 0x69998315, 0x172ca318, 0x3faa77de, 0x411f57d3,
    0x6ea7de52, 0x1012fe5f, 0x38942a99, 0x46210a94, 0x67e5399b, 0x19501996,
    0x31d6cd50, 0x4f63ed5d, 0x60db64dc, 0x1e6e44d1, 0x36e89017, 0x485db01a,
    0x3f77c841, 0x41c2e84c, 0x69443c8a, 0x17f11c87, 0x38499506, 0x46fcb50b,
    0x6e7a61cd, 0x10cf41c0, 0x310b72cf, 0x4fbe52c2, 0x67388604, 0x198da609,
    0x36352f88, 0x48800f85, 0x6006db43, 0x1eb3fb4e, 0x238ebd5d, 0x5d3b9d50,
    0x75bd4996, 0x0b08699b, 0x24b0e01a, 0x5a05c017, 0x728314d1, 0x0c3634dc,
    0x2df207d3, 0x534727de, 0x7bc1f318, 0x0574d315, 0x2acc5a94, 0x54797a99,
    0x7cffae5f, 0x024a8e52, 0x06852279, 0x78300274, 0x50b6d6b2, 0x2e03f6bf,
    0x01bb7f3e, 0x7f0e5f33, 0x57888bf5, 0x293dabf8, 0x08f998f7, 0x764cb8fa,
    0x5eca6c3c, 0x207f4c31, 0x0fc7c5b0, 0x7172e5bd, 0x59f4317b, 0x27411176,
    0x1a7c5765, 0x64c97768, 0x4c4fa3ae, 0x32fa83a3, 0x1d420a22, 0x63f72a2f,
    0x4b71fee9, 0x35c4dee4, 0x1400edeb, 0x6ab5cde6, 0x42331920, 0x3c86392d,
    0x133eb0ac, 0x6d8b90a1, 0x450d4467, 0x3bb8646a,
];

/// An entry in the packed delta index: a 16-byte block start in the source
/// buffer plus its Rabin fingerprint.
#[derive(Clone, Copy)]
struct IndexEntry {
    ptr: usize,
    val: u32,
}

/// A packed delta index over a source buffer (port of C `create_delta_index`).
pub struct DeltaIndex {
    src: Vec<u8>,
    hash: Vec<IndexEntry>,
    hash_bounds: Vec<usize>,
    mask: u32,
}

impl DeltaIndex {
    /// Build an index over `src`. Returns `None` for empty buffers (same as C,
    /// which returns NULL when there is nothing to index) or oversized sources
    /// whose offsets cannot be encoded in the delta format.
    pub fn new(src: Vec<u8>) -> Option<DeltaIndex> {
        if src.is_empty() {
            return None;
        }
        let bufsize = src.len() as u64;
        let mut entries = ((bufsize - 1) / RABIN_WINDOW as u64) as u32;
        if bufsize >= 0xffff_ffff {
            entries = 0xffff_fffe / RABIN_WINDOW as u32;
        }
        let hsize = {
            let mut target = entries / 4;
            let mut i = 4u32;
            while (1u32 << i) < target {
                i += 1;
            }
            1u32 << i
        };
        let hmask = hsize - 1;

        let mut hash: Vec<IndexEntry> = Vec::with_capacity(entries as usize);

        // Populate the index walking blocks back to front so that, for equal
        // blocks, the lowest offset wins (kept via the consecutive-identical
        // dedup below).
        let mut prev_val: u32 = !0;
        let mut data = entries as usize * RABIN_WINDOW - RABIN_WINDOW;
        loop {
            let mut val: u32 = 0;
            for i in 1..=RABIN_WINDOW {
                let b = u32::from(src[data + i]);
                val = ((val << 8) | b) ^ T[(val >> RABIN_SHIFT) as usize];
            }
            if val == prev_val {
                // keep the lowest of consecutive identical blocks
                hash.pop();
                entries -= 1;
            } else {
                prev_val = val;
                hash.push(IndexEntry {
                    ptr: data + RABIN_WINDOW,
                    val,
                });
            }
            if data == 0 {
                break;
            }
            data -= RABIN_WINDOW;
        }

        // Cull over-populated buckets uniformly to HASH_LIMIT entries
        // (guards against pathological data sets).
        let mut kept: Vec<IndexEntry> = Vec::with_capacity(hash.len());
        let mut bucket_counts = vec![0u32; hsize as usize];
        for e in &hash {
            bucket_counts[(e.val & hmask) as usize] += 1;
        }
        let mut seen = vec![0u32; hsize as usize];
        for e in hash.drain(..) {
            let b = (e.val & hmask) as usize;
            let count = bucket_counts[b];
            if count <= HASH_LIMIT {
                kept.push(e);
            } else {
                // keep every (count / HASH_LIMIT)-th entry
                seen[b] += 1;
                if seen[b] == count / HASH_LIMIT {
                    seen[b] = 0;
                    kept.push(e);
                }
            }
        }
        entries = kept.len() as u32;

        // Packed form: entries grouped by bucket with bounds array.
        kept.sort_by_key(|e| (e.val & hmask) as usize);
        let mut hash_bounds = vec![0usize; hsize as usize + 1];
        for (n, e) in kept.iter().enumerate() {
            hash_bounds[(e.val & hmask) as usize + 1] = n + 1;
        }
        for i in 1..=hsize as usize {
            if hash_bounds[i] == 0 {
                hash_bounds[i] = hash_bounds[i - 1];
            }
        }
        debug_assert_eq!(hash_bounds[hsize as usize], kept.len());

        Some(DeltaIndex {
            src,
            hash: kept,
            hash_bounds,
            mask: hmask,
        })
    }

    /// The indexed source buffer.
    pub fn src(&self) -> &[u8] {
        &self.src
    }
}

/// Create a delta between an indexed source and a target buffer
/// (port of C `create_delta`). Returns `None` if no delta within
/// `max_size` bytes could be produced (`max_size == 0` means unlimited).
pub fn create_delta(index: &DeltaIndex, trg_buf: &[u8], max_size: usize) -> Option<Vec<u8>> {
    if trg_buf.is_empty() {
        return None;
    }
    let src = &index.src;
    let mut out: Vec<u8> = Vec::new();

    // store reference buffer size (git's "+1" varint)
    let mut l = src.len() as u64;
    while l >= 0x80 {
        out.push(((l & 0x7f) as u8) | 0x80);
        l >>= 7;
    }
    out.push(l as u8);

    // store target buffer size
    let mut l = trg_buf.len() as u64;
    while l >= 0x80 {
        out.push(((l & 0x7f) as u8) | 0x80);
        l >>= 7;
    }
    out.push(l as u8);

    // slot for the first insert-run count byte (C does `outpos++` here)
    let mut ins_slot = out.len();
    out.push(0);

    let mut data = 0usize; // cursor into trg_buf
    let mut val: u32 = 0;
    let mut inscnt = 0usize;
    while inscnt < RABIN_WINDOW && data < trg_buf.len() {
        let b = u32::from(trg_buf[data]);
        val = ((val << 8) | b) ^ T[(val >> RABIN_SHIFT) as usize];
        out.push(trg_buf[data]);
        data += 1;
        inscnt += 1;
    }

    let mut moff = 0usize;
    let mut msize = 0usize;
    while data < trg_buf.len() {
        if msize < 4096 {
            val ^= U[usize::from(trg_buf[data - RABIN_WINDOW])];
            let b = u32::from(trg_buf[data]);
            val = ((val << 8) | b) ^ T[(val >> RABIN_SHIFT) as usize];
            let bucket = (val & index.mask) as usize;
            for entry in &index.hash[index.hash_bounds[bucket]..index.hash_bounds[bucket + 1]] {
                if entry.val != val {
                    continue;
                }
                let mut ref_pos = entry.ptr;
                let mut src_pos = data;
                let mut ref_size = src.len() - ref_pos;
                if ref_size > trg_buf.len() - src_pos {
                    ref_size = trg_buf.len() - src_pos;
                }
                if ref_size <= msize {
                    break;
                }
                let mut n = ref_size;
                while n > 0 && src[src_pos] == trg_buf[ref_pos] {
                    ref_pos += 1;
                    src_pos += 1;
                    n -= 1;
                }
                if msize < ref_pos - entry.ptr {
                    // this is our best match so far
                    msize = ref_pos - entry.ptr;
                    moff = entry.ptr;
                    if msize >= 4096 {
                        break; // good enough
                    }
                }
            }
        }

        if msize < 4 {
            if inscnt == 0 {
                // start a new insert run: reserve the count slot
                ins_slot = out.len();
                out.push(0);
            }
            out.push(trg_buf[data]);
            data += 1;
            inscnt += 1;
            if inscnt == 0x7f {
                out[ins_slot] = inscnt as u8;
                inscnt = 0;
            }
            msize = 0;
        } else {
            // A copy op is currently limited to 64KB (pack v2).
            let left = if msize >= 0x1_0000 { msize - 0x1_0000 } else { 0 };
            msize -= left;

            if inscnt > 0 {
                // Try to match one byte back to fold the copy into the match.
                while moff > 0 && src[moff - 1] == trg_buf[data - 1] {
                    msize += 1;
                    moff -= 1;
                    data -= 1;
                    out.pop();
                    inscnt -= 1;
                    if inscnt > 0 {
                        continue;
                    }
                    out.pop(); // remove the empty count slot too
                    break;
                }
                if inscnt > 0 {
                    out[ins_slot] = inscnt as u8;
                }
                inscnt = 0;
            }

            let op_pos = out.len();
            out.push(0);
            let mut op: u8 = 0x80;
            if moff & 0x0000_00ff != 0 {
                out.push(moff as u8);
                op |= 0x01;
            }
            if moff & 0x0000_ff00 != 0 {
                out.push((moff >> 8) as u8);
                op |= 0x02;
            }
            if moff & 0x00ff_0000 != 0 {
                out.push((moff >> 16) as u8);
                op |= 0x04;
            }
            if moff & 0xff00_0000 != 0 {
                out.push((moff >> 24) as u8);
                op |= 0x08;
            }
            if msize & 0x0000_00ff != 0 {
                out.push(msize as u8);
                op |= 0x10;
            }
            if msize & 0x0000_ff00 != 0 {
                out.push((msize >> 8) as u8);
                op |= 0x20;
            }
            out[op_pos] = op;

            data += msize;
            moff += msize;
            msize = left;

            if moff > 0xffff_ffff {
                msize = 0;
            }

            if msize < 4096 {
                val = 0;
                for j in 1..=RABIN_WINDOW {
                    let b = u32::from(trg_buf[data - j]);
                    val = ((val << 8) | b) ^ T[(val >> RABIN_SHIFT) as usize];
                }
            }
        }

        if max_size > 0 && out.len() > max_size + MAX_OP_SIZE {
            return None;
        }
    }

    if inscnt > 0 {
        out[ins_slot] = inscnt as u8;
    }

    if max_size > 0 && out.len() > max_size {
        return None;
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_sizes_round_trip() {
        // Small sizes are a single byte.
        let mut d: &[u8] = &[5, 11];
        assert_eq!(get_delta_hdr_size(&mut d), Some(5));
        assert_eq!(get_delta_hdr_size(&mut d), Some(11));
        // Multi-byte: encode 0x80 as [0x80, 0x00].
        let mut d: &[u8] = &[0x80, 0x00, 0x01];
        assert_eq!(get_delta_hdr_size(&mut d), Some(128));
        assert_eq!(get_delta_hdr_size(&mut d), Some(1));
        // Empty input -> None.
        let mut d: &[u8] = &[];
        assert_eq!(get_delta_hdr_size(&mut d), None);
    }

    #[test]
    fn apply_copy_and_insert() {
        let base = b"hello world";
        // Header: src=11, dst=13; copy 6 bytes @0, insert "!", copy 4 @7,
        // insert "!!".
        let delta = [
            11u8, 13,          // sizes
            0x91, 0x00, 0x06,  // copy 6 bytes @ offset 0
            0x01, b'!',        // insert "!"
            0x91, 0x07, 0x04,  // copy 4 bytes @ offset 7 ("orld")
            0x02, b'!', b'!',  // insert "!!"
        ];
        assert_eq!(apply_delta(base, &delta).unwrap(), b"hello !orld!!");
    }

    #[test]
    fn apply_copy_whole_base() {
        let base = b"abcdef";
        // Copy all 6 bytes: cmd 0x91 = copy + offset byte0 + size byte0.
        let delta = [6u8, 6, 0x91, 0x00, 0x06];
        assert_eq!(apply_delta(base, &delta).unwrap(), b"abcdef");
    }

    #[test]
    fn copy_defaults_to_64k() {
        // src/dst size 0x10000 encoded with git's "+1" varint scheme, then a
        // copy command with no size byte (which defaults to 0x10000).
        let base = vec![b'x'; 0x10000];
        let delta = [0x82, 0xff, 0x00, 0x82, 0xff, 0x00, 0x81, 0x00];
        let res = apply_delta(&base, &delta).unwrap();
        assert_eq!(res.len(), 0x10000);
        assert!(res.iter().all(|&b| b == b'x'));
    }

    #[test]
    fn bad_deltas_rejected() {
        // Source size mismatch.
        let delta = [5u8, 3, 0x01, b'a', b'b', b'c'];
        assert_eq!(apply_delta(b"xx", &delta), Err(PackError::BadDelta));
        // Copy out of range.
        let delta = [2u8, 2, 0x81, 0x0a, 0x02];
        assert_eq!(apply_delta(b"ab", &delta), Err(PackError::BadDelta));
        // Empty command byte.
        let delta = [1u8, 1, 0x00];
        assert_eq!(apply_delta(b"a", &delta), Err(PackError::BadDelta));
    }
}

#[cfg(test)]
mod creation_tests {
    use super::*;

    #[test]
    fn create_delta_simple() {
        let base = b"hello world".to_vec();
        let target = b"hello !orld!!".to_vec();
        let idx = DeltaIndex::new(base.clone()).unwrap();
        let delta = create_delta(&idx, &target, usize::MAX).unwrap();
        let reconstructed = apply_delta(&base, &delta).unwrap();
        assert_eq!(reconstructed, target);
    }

    #[test]
    fn create_delta_identical() {
        let base = b"same data".to_vec();
        let target = b"same data".to_vec();
        let idx = DeltaIndex::new(base.clone()).unwrap();
        let delta = create_delta(&idx, &target, usize::MAX).unwrap();
        let reconstructed = apply_delta(&base, &delta).unwrap();
        assert_eq!(reconstructed, target);
    }

    #[test]
    fn create_delta_empty_target() {
        let base = b"hello".to_vec();
        let idx = DeltaIndex::new(base.clone()).unwrap();
        // C returns NULL for an empty target.
        assert!(create_delta(&idx, b"", usize::MAX).is_none());
    }

    #[test]
    fn create_delta_max_size_respected() {
        let base = vec![b'a'; 1000];
        let target = vec![b'a'; 1000];
        let idx = DeltaIndex::new(base.clone()).unwrap();
        // A tiny limit that the delta cannot possibly fit.
        assert!(create_delta(&idx, &target, 4).is_none());
        // Unlimited works.
        assert!(create_delta(&idx, &target, usize::MAX).is_some());
    }

    #[test]
    fn create_delta_round_trip_large() {
        // Synthetic-ish text with shared blocks: base is a repeated pattern,
        // target reorders lines and appends new content.
        let mut base = Vec::new();
        for i in 0..2000 {
            base.extend_from_slice(format!("line {i:05} of the base document\n").as_bytes());
        }
        let mut target = Vec::new();
        for i in (0..2000).rev() {
            target.extend_from_slice(format!("line {i:05} of the base document\n").as_bytes());
        }
        target.extend_from_slice(b"appended new content at the end\n");
        let idx = DeltaIndex::new(base.clone()).unwrap();
        let delta = create_delta(&idx, &target, usize::MAX).unwrap();
        let reconstructed = apply_delta(&base, &delta).unwrap();
        assert_eq!(reconstructed, target);
        // The delta should actually be smaller than the target.
        assert!(
            delta.len() < target.len(),
            "delta {} not smaller than target {}",
            delta.len(),
            target.len()
        );
    }

    #[test]
    fn create_delta_target_shorter_than_window() {
        let base = b"0123456789abcdef0123456789abcdef".to_vec();
        let target = b"XY".to_vec();
        let idx = DeltaIndex::new(base.clone()).unwrap();
        let delta = create_delta(&idx, &target, usize::MAX).unwrap();
        assert_eq!(apply_delta(&base, &delta).unwrap(), target);
    }

    #[test]
    fn create_delta_insert_only() {
        let base = b"totally unrelated base content here".to_vec();
        let target: Vec<u8> = (0..100u8).collect();
        let idx = DeltaIndex::new(base.clone()).unwrap();
        let delta = create_delta(&idx, &target, usize::MAX).unwrap();
        assert_eq!(apply_delta(&base, &delta).unwrap(), target);
    }

    #[test]
    fn create_delta_long_insert_run() {
        // Exercises the 0x7f insert-run wrap and multiple run slots.
        let base = b"a".repeat(10);
        let target: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let idx = DeltaIndex::new(base.clone()).unwrap();
        let delta = create_delta(&idx, &target, usize::MAX).unwrap();
        assert_eq!(apply_delta(&base, &delta).unwrap(), target);
    }
}
