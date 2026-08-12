//! Pure-Rust SHA-1 (FIPS 180-1).

/// Streaming SHA-1 hash state.
#[derive(Clone)]
pub struct Sha1 {
    state: [u32; 5],
    buf: [u8; 64],
    buflen: usize,
    total: u64,
}

const K: [u32; 4] = [0x5A82_7999, 0x6ED9_EBA1, 0x8F1B_BCDC, 0xCA62_C1D6];

impl Sha1 {
    pub const DIGEST_LEN: usize = 20;

    pub fn new() -> Sha1 {
        Sha1 {
            state: [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0],
            buf: [0; 64],
            buflen: 0,
            total: 0,
        }
    }

    /// Feed `data` into the hash.
    pub fn update(&mut self, mut data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.total = self.total.wrapping_add(data.len() as u64);

        if self.buflen > 0 {
            let take = std::cmp::min(64 - self.buflen, data.len());
            self.buf[self.buflen..self.buflen + take].copy_from_slice(&data[..take]);
            self.buflen += take;
            data = &data[take..];
            if self.buflen == 64 {
                let block = self.buf;
                self.process(&block);
                self.buflen = 0;
            }
        }

        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.process(&block);
            data = &data[64..];
        }

        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buflen = data.len();
        }
    }

    /// Finalize, returning the 20-byte digest. Consumes the hasher.
    ///
    /// Padding is applied by processing blocks directly rather than routing
    /// through `update`, which would re-buffer already-buffered bytes.
    pub fn finalize(mut self) -> [u8; 20] {
        let bit_len = self.total.wrapping_mul(8);
        // Terminate the message in the current block, preserving the pending
        // bytes already buffered in `buf[..buflen]`.
        self.buf[self.buflen] = 0x80;
        self.buflen += 1;
        for b in &mut self.buf[self.buflen..] {
            *b = 0;
        }
        if self.buflen > 56 {
            // No room for the 64-bit length; process this block and start a
            // fresh one holding the length.
            let block = self.buf;
            self.process(&block);
            let mut fresh = [0u8; 64];
            fresh[56..64].copy_from_slice(&bit_len.to_be_bytes());
            self.process(&fresh);
        } else {
            self.buf[56..64].copy_from_slice(&bit_len.to_be_bytes());
            let block = self.buf;
            self.process(&block);
        }

        let mut out = [0u8; 20];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn process(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let [mut a, mut b, mut c, mut d, mut e] = self.state;
        for (i, &w) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), K[0]),
                20..=39 => (b ^ c ^ d, K[1]),
                40..=59 => ((b & c) | (b & d) | (c & d), K[2]),
                _ => (b ^ c ^ d, K[3]),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
    }
}

impl Default for Sha1 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::Sha1;

    fn digest(data: &[u8]) -> String {
        let mut h = Sha1::new();
        h.update(data);
        let out = h.finalize();
        let mut s = String::with_capacity(40);
        for b in out {
            s.push_str(&format!("{:02x}", b));
        }
        s
    }

    #[test]
    fn fips_vectors() {
        assert_eq!(digest(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(digest(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            digest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn million_a_vector() {
        assert_eq!(digest(&[b'a'; 1_000_000]), "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
    }

    #[test]
    fn incremental_matches_oneshot() {
        let data: Vec<u8> = (0..=255u8).flat_map(|b| [b, b, b, b]).collect();
        let mut inc = Sha1::new();
        for chunk in data.chunks(7) {
            inc.update(chunk);
        }
        let mut one = Sha1::new();
        one.update(&data);
        assert_eq!(inc.finalize(), one.finalize());
    }

    #[test]
    fn clone_is_independent() {
        let mut a = Sha1::new();
        a.update(b"abc");
        let mut b = a.clone();
        b.update(b"def");
        let mut c = a.clone();
        c.update(b"def");
        assert_eq!(b.finalize(), c.finalize());
        let mut a2 = a.clone();
        a2.update(b"def");
        assert_ne!(a.finalize(), a2.finalize());
    }
}
