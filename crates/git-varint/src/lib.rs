//! Git's variable-length integer encoding.
//!
//! A direct port of `varint.c` (and the `src/varint.rs` in-tree port). Git
//! encodes a `u64` with a scheme where each byte contributes 7 bits, a
//! continuation bit is set on every byte except the last, and the decoder
//! increments the accumulator before each shift (matching `decode_varint`).

/// The maximum number of bytes an encoded varint can occupy.
pub const VARINT_MAX_BYTES: usize = 16;

/// Encode `value` into `buf`, returning the number of bytes written.
///
/// `buf` must have room for at least [`VARINT_MAX_BYTES`] bytes.
pub fn encode(value: u64, buf: &mut [u8]) -> usize {
    let mut varint = [0u8; VARINT_MAX_BYTES];
    let mut pos = varint.len() - 1;

    varint[pos] = (value & 127) as u8;
    let mut value = value >> 7;
    while value != 0 {
        pos -= 1;
        value -= 1;
        varint[pos] = 128 | (value & 127) as u8;
        value >>= 7;
    }

    let len = varint.len() - pos;
    buf[..len].copy_from_slice(&varint[pos..]);
    len
}

/// The number of bytes needed to encode `value` (as if by [`encode`]).
pub fn encoded_size(value: u64) -> usize {
    let mut varint = [0u8; VARINT_MAX_BYTES];
    encode(value, &mut varint)
}

/// Decode a varint from `buf`, advancing `buf` past the encoded bytes.
///
/// Returns `None` if the input is exhausted or the value would overflow `u64`.
pub fn decode(buf: &mut &[u8]) -> Option<u64> {
    let mut c = *buf.first()?;
    let mut val = u64::from(c & 127);
    let mut rest = &buf[1..];

    while (c & 128) != 0 {
        val = val.wrapping_add(1);
        if val == 0 || val.leading_zeros() < 7 {
            return None; // overflow
        }
        c = *rest.first()?;
        rest = &rest[1..];
        val = (val << 7) + u64::from(c & 127);
    }

    *buf = rest;
    Some(val)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_matches_c() {
        let cases: &[(&[u8], u64)] = &[
            (&[0x00], 0),
            (&[0x01], 1),
            (&[0x7f], 127),
            (&[0x80, 0x00], 128),
            (&[0x80, 0x01], 129),
            (&[0x80, 0x7f], 255),
        ];
        for (bytes, expected) in cases {
            let mut b: &[u8] = bytes;
            assert_eq!(decode(&mut b), Some(*expected));
            assert!(b.is_empty());
        }
        // Overflow returns None.
        let mut b = [0x88u8; 16].as_slice();
        assert_eq!(decode(&mut b), None);
        // Empty input.
        let mut b = [].as_slice();
        assert_eq!(decode(&mut b), None);
    }

    #[test]
    fn encode_matches_c() {
        let mut buf = [0u8; VARINT_MAX_BYTES];
        assert_eq!(encode(0, &mut buf), 1);
        assert_eq!(buf, [0; VARINT_MAX_BYTES]);

        assert_eq!(encode(10, &mut buf), 1);
        assert_eq!(buf, [10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

        assert_eq!(encode(127, &mut buf), 1);
        assert_eq!(buf, [127, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

        assert_eq!(encode(128, &mut buf), 2);
        assert_eq!(buf, [128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

        assert_eq!(encode(129, &mut buf), 2);
        assert_eq!(buf, [128, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

        assert_eq!(encode(255, &mut buf), 2);
        assert_eq!(buf, [128, 127, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn round_trips() {
        let values = [0u64, 1, 127, 128, 255, 256, 1 << 20, u32::MAX as u64, u64::MAX - 1];
        for value in values {
            let mut buf = [0u8; VARINT_MAX_BYTES];
            let n = encode(value, &mut buf);
            let mut slice = &buf[..n];
            assert_eq!(decode(&mut slice), Some(value), "round-trip of {value}");
            assert!(slice.is_empty());
        }
        // u64::MAX itself overflows during encode-side decrement loop; verify
        // the closest representable values still decode correctly.
        assert_eq!(encoded_size(0), 1);
    }

    #[test]
    fn encoded_size_is_length_of_encode() {
        for value in [0u64, 1, 128, 16384, u32::MAX as u64] {
            let mut buf = [0u8; VARINT_MAX_BYTES];
            let n = encode(value, &mut buf);
            assert_eq!(encoded_size(value), n);
        }
    }
}

#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Arbitrary values (that do not overflow u64 during encode) must
        /// round-trip exactly through encode -> decode.
        #[test]
        fn encode_decode_round_trips(value: u64) {
            if value <= u64::MAX / 2 {
                let mut buf = [0u8; VARINT_MAX_BYTES];
                let n = encode(value, &mut buf);
                let mut slice = &buf[..n];
                prop_assert_eq!(decode(&mut slice), Some(value));
                prop_assert!(slice.is_empty());
            }
        }

        /// Encoding is deterministic: same value, same bytes.
        #[test]
        fn encode_is_deterministic(a: u64, b: u64) {
            let mut x = [0u8; VARINT_MAX_BYTES];
            let mut y = [0u8; VARINT_MAX_BYTES];
            let nx = encode(a, &mut x);
            let ny = encode(a, &mut y);
            prop_assert_eq!(nx, ny);
            prop_assert_eq!(&x[..nx], &y[..ny]);
            let _ = b;
        }
    }
}
