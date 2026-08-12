//! Delta application (`patch-delta.c`).
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
            11u8, 13,             // sizes
            0x91, 0x00, 0x06,     // copy 6 bytes @ offset 0
            0x01, b'!',           // insert "!"
            0x91, 0x07, 0x04,     // copy 4 bytes @ offset 7 ("orld")
            0x02, b'!', b'!',     // insert "!!"
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
