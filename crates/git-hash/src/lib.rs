//! Object IDs and hash algorithms.
//!
//! This crate provides the object-ID abstraction used throughout the port:
//! a fixed 32-byte hash buffer plus the algorithm it belongs to, and pure-Rust
//! SHA-1 / SHA-256 hashers behind a single trait so the backend can be swapped
//! (e.g. collision-detecting SHA-1) later.

pub mod sha1;
pub mod sha256;

use std::error::Error;
use std::fmt;
use std::io::{self, Write};

/// The maximum size, in bytes, of any supported object id.
pub const GIT_MAX_RAWSZ: usize = 32;

/// A hash algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u32)]
pub enum HashAlgorithm {
    Sha1 = 1,
    Sha256 = 2,
}

impl HashAlgorithm {
    pub const SHA1_NULL_OID: Oid = Oid {
        hash: [0u8; GIT_MAX_RAWSZ],
        algo: HashAlgorithm::Sha1,
    };
    pub const SHA256_NULL_OID: Oid = Oid {
        hash: [0u8; GIT_MAX_RAWSZ],
        algo: HashAlgorithm::Sha256,
    };

    pub const SHA1_EMPTY_TREE: Oid = Oid {
        hash: *b"\x4b\x82\x5d\xc6\x42\xcb\x6e\xb9\xa0\x60\xe5\x4b\xf8\xd6\x92\x88\xfb\xee\x49\x04\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00",
        algo: HashAlgorithm::Sha1,
    };
    pub const SHA256_EMPTY_TREE: Oid = Oid {
        hash: *b"\x6e\xf1\x9b\x41\x22\x5c\x53\x69\xf1\xc1\x04\xd4\x5d\x8d\x85\xef\xa9\xb0\x57\xb5\x3b\x14\xb4\xb9\xb9\x39\xdd\x74\xde\xcc\x53\x21",
        algo: HashAlgorithm::Sha256,
    };

    pub const SHA1_EMPTY_BLOB: Oid = Oid {
        hash: *b"\xe6\x9d\xe2\x9b\xb2\xd1\xd6\x43\x4b\x8b\x29\xae\x77\x5a\xd8\xc2\xe4\x8c\x53\x91\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00",
        algo: HashAlgorithm::Sha1,
    };
    pub const SHA256_EMPTY_BLOB: Oid = Oid {
        hash: *b"\x47\x3a\x0f\x4c\x3b\xe8\xa9\x36\x81\xa2\x67\xe3\xb1\xe9\xa7\xdc\xda\x11\x85\x43\x6f\xe1\x41\xf7\x74\x91\x20\xa3\x03\x72\x18\x13",
        algo: HashAlgorithm::Sha256,
    };

    /// The length of binary object ids for this algorithm, in bytes.
    pub const fn raw_len(self) -> usize {
        match self {
            HashAlgorithm::Sha1 => 20,
            HashAlgorithm::Sha256 => 32,
        }
    }

    /// The length of object ids in hex characters.
    pub const fn hex_len(self) -> usize {
        self.raw_len() * 2
    }

    /// The number of bytes processed by one iteration of the compression
    /// function.
    pub const fn block_size(self) -> usize {
        64
    }

    /// The name used in configuration and user-visible output.
    pub const fn name(self) -> &'static str {
        match self {
            HashAlgorithm::Sha1 => "sha1",
            HashAlgorithm::Sha256 => "sha256",
        }
    }

    /// The format id used in binary formats (written big-endian).
    pub const fn format_id(self) -> u32 {
        match self {
            HashAlgorithm::Sha1 => 0x7368_6131,
            HashAlgorithm::Sha256 => 0x7332_3536,
        }
    }

    /// Convert an internal integer id to an algorithm.
    pub const fn from_u32(n: u32) -> Option<HashAlgorithm> {
        match n {
            1 => Some(HashAlgorithm::Sha1),
            2 => Some(HashAlgorithm::Sha256),
            _ => None,
        }
    }

    /// Convert a binary-format id to an algorithm.
    pub const fn from_format_id(n: u32) -> Option<HashAlgorithm> {
        match n {
            0x7368_6131 => Some(HashAlgorithm::Sha1),
            0x7332_3536 => Some(HashAlgorithm::Sha256),
            _ => None,
        }
    }

    /// The all-zero object id.
    pub const fn null_oid(self) -> &'static Oid {
        match self {
            HashAlgorithm::Sha1 => &Self::SHA1_NULL_OID,
            HashAlgorithm::Sha256 => &Self::SHA256_NULL_OID,
        }
    }

    /// The object id of the empty tree.
    pub const fn empty_tree(self) -> &'static Oid {
        match self {
            HashAlgorithm::Sha1 => &Self::SHA1_EMPTY_TREE,
            HashAlgorithm::Sha256 => &Self::SHA256_EMPTY_TREE,
        }
    }

    /// The object id of the empty blob.
    pub const fn empty_blob(self) -> &'static Oid {
        match self {
            HashAlgorithm::Sha1 => &Self::SHA1_EMPTY_BLOB,
            HashAlgorithm::Sha256 => &Self::SHA256_EMPTY_BLOB,
        }
    }

    /// Create a hasher for this algorithm.
    pub fn hasher(self) -> CryptoHasher {
        CryptoHasher::new(self)
    }
}

/// Error returned when parsing an object id from hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HexParseError {
    /// The string was not valid lowercase hex.
    InvalidHex,
    /// The string was shorter than the required minimum length.
    TooShort { min: usize },
    /// The string was longer than the full object id length.
    TooLong { max: usize },
}

impl fmt::Display for HexParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HexParseError::InvalidHex => write!(f, "invalid hex character in object id"),
            HexParseError::TooShort { min } => write!(f, "object id too short (minimum {min} hex chars)"),
            HexParseError::TooLong { max } => write!(f, "object id too long (maximum {max} hex chars)"),
        }
    }
}

impl Error for HexParseError {}

/// A binary object id.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Oid {
    /// Raw hash bytes, zero-filled beyond the algorithm's raw length.
    pub hash: [u8; GIT_MAX_RAWSZ],
    /// The algorithm this id belongs to.
    pub algo: HashAlgorithm,
}

impl Oid {
    /// Construct an id from `hash`, which must be exactly `algo.raw_len()`
    /// bytes. Panics otherwise.
    pub fn new(algo: HashAlgorithm, hash: &[u8]) -> Oid {
        assert_eq!(
            hash.len(),
            algo.raw_len(),
            "hash length {} does not match algorithm {} raw length {}",
            hash.len(),
            algo.name(),
            algo.raw_len()
        );
        let mut data = [0u8; GIT_MAX_RAWSZ];
        data[..hash.len()].copy_from_slice(hash);
        Oid { hash: data, algo }
    }

    /// The raw hash bytes for this id.
    pub fn as_slice(&self) -> &[u8] {
        &self.hash[..self.algo.raw_len()]
    }

    /// The raw hash bytes as a mutable slice.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.hash[..self.algo.raw_len()]
    }

    /// The algorithm of this id.
    pub const fn algorithm(&self) -> HashAlgorithm {
        self.algo
    }

    /// Parse a full-length lowercase hex id.
    pub fn from_hex(s: &str, algo: HashAlgorithm) -> Result<Oid, HexParseError> {
        Self::from_hex_abbrev(s, algo, algo.hex_len())
    }

    /// Parse an abbreviated lowercase hex id of at least `min_len` characters.
    ///
    /// The unknown trailing bytes are zero-filled; the resulting id matches by
    /// prefix (as git's `--abbrev` matching does). Use [`Oid::from_hex`] for
    /// full-length ids.
    pub fn from_hex_abbrev(s: &str, algo: HashAlgorithm, min_len: usize) -> Result<Oid, HexParseError> {
        if s.len() < min_len {
            return Err(HexParseError::TooShort { min: min_len });
        }
        if s.len() > algo.hex_len() {
            return Err(HexParseError::TooLong { max: algo.hex_len() });
        }
        let mut oid = Oid {
            hash: [0u8; GIT_MAX_RAWSZ],
            algo,
        };
        let mut nibble = 0u8;
        for (i, c) in s.bytes().enumerate() {
            let v = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                _ => return Err(HexParseError::InvalidHex),
            };
            if i % 2 == 0 {
                nibble = v << 4;
            } else {
                oid.hash[i / 2] = nibble | v;
            }
        }
        if s.len() % 2 == 1 {
            // Commit the trailing high nibble of an odd-length abbreviation.
            oid.hash[s.len() / 2] = nibble;
        }
        Ok(oid)
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.as_slice() {
            write!(f, "{:02x}", b)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self, self.algo.name())
    }
}

/// A trait for incremental hashing with a cryptographic algorithm.
pub trait CryptoDigest {
    /// True if this digest is safe for use with untrusted data.
    fn is_safe(&self) -> bool;

    /// Feed `data` into the digest.
    fn update(&mut self, data: &[u8]);

    /// Consume the hasher and return the object id.
    fn into_oid(self) -> Oid;

    /// Consume the hasher and return the raw hash bytes.
    fn into_vec(self) -> Vec<u8>;
}

/// A hasher that wraps the pure-Rust SHA-1 and SHA-256 implementations.
#[derive(Clone)]
pub enum CryptoHasher {
    Sha1(sha1::Sha1),
    Sha256(sha256::Sha256),
}

impl CryptoHasher {
    /// Create a new hasher for `algo`.
    pub fn new(algo: HashAlgorithm) -> CryptoHasher {
        match algo {
            HashAlgorithm::Sha1 => CryptoHasher::Sha1(sha1::Sha1::new()),
            HashAlgorithm::Sha256 => CryptoHasher::Sha256(sha256::Sha256::new()),
        }
    }

    /// The algorithm being hashed.
    pub fn algorithm(&self) -> HashAlgorithm {
        match self {
            CryptoHasher::Sha1(_) => HashAlgorithm::Sha1,
            CryptoHasher::Sha256(_) => HashAlgorithm::Sha256,
        }
    }

    /// Feed `data` into the hasher.
    pub fn update(&mut self, data: &[u8]) {
        match self {
            CryptoHasher::Sha1(h) => h.update(data),
            CryptoHasher::Sha256(h) => h.update(data),
        }
    }

    /// Consume the hasher and return the raw hash bytes.
    pub fn finalize(self) -> Vec<u8> {
        match self {
            CryptoHasher::Sha1(h) => h.finalize().to_vec(),
            CryptoHasher::Sha256(h) => h.finalize().to_vec(),
        }
    }

    /// Consume the hasher and return the object id.
    pub fn finalize_oid(self) -> Oid {
        Oid::new(self.algorithm(), &self.finalize())
    }
}

impl CryptoDigest for CryptoHasher {
    fn is_safe(&self) -> bool {
        // Standard SHA-1 is not collision safe; only SHA-256 is.
        match self {
            CryptoHasher::Sha1(_) => false,
            CryptoHasher::Sha256(_) => true,
        }
    }

    fn update(&mut self, data: &[u8]) {
        self.update(data);
    }

    fn into_oid(self) -> Oid {
        self.finalize_oid()
    }

    fn into_vec(self) -> Vec<u8> {
        self.finalize()
    }
}

impl Write for CryptoHasher {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.update(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{CryptoDigest, HashAlgorithm, Oid};
    use std::io::Write;

    fn all_algos() -> &'static [HashAlgorithm] {
        &[HashAlgorithm::Sha1, HashAlgorithm::Sha256]
    }

    #[test]
    fn algorithm_id_round_trips() {
        for algo in all_algos() {
            assert_eq!(HashAlgorithm::from_u32(*algo as u32), Some(*algo));
            assert_eq!(HashAlgorithm::from_format_id(algo.format_id()), Some(*algo));
            assert_eq!(algo.raw_len(), algo.hex_len() / 2);
            assert_eq!(algo.hex_len(), algo.raw_len() * 2);
        }
        assert_eq!(HashAlgorithm::from_u32(0), None);
        assert_eq!(HashAlgorithm::from_format_id(0), None);
    }

    #[test]
    fn slices_have_correct_length() {
        for algo in all_algos() {
            for oid in [*algo.null_oid(), *algo.empty_blob(), *algo.empty_tree()] {
                assert_eq!(oid.as_slice().len(), algo.raw_len());
            }
        }
    }

    #[test]
    fn object_ids_format_correctly() {
        let entries: &[(Oid, &str, &str)] = &[
            (*HashAlgorithm::Sha1.null_oid(), "0000000000000000000000000000000000000000", "0000000000000000000000000000000000000000:sha1"),
            (*HashAlgorithm::Sha1.empty_blob(), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391", "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391:sha1"),
            (*HashAlgorithm::Sha1.empty_tree(), "4b825dc642cb6eb9a060e54bf8d69288fbee4904", "4b825dc642cb6eb9a060e54bf8d69288fbee4904:sha1"),
            (*HashAlgorithm::Sha256.null_oid(), "0000000000000000000000000000000000000000000000000000000000000000", "0000000000000000000000000000000000000000000000000000000000000000:sha256"),
            (*HashAlgorithm::Sha256.empty_blob(), "473a0f4c3be8a93681a267e3b1e9a7dcda1185436fe141f7749120a303721813", "473a0f4c3be8a93681a267e3b1e9a7dcda1185436fe141f7749120a303721813:sha256"),
            (*HashAlgorithm::Sha256.empty_tree(), "6ef19b41225c5369f1c104d45d8d85efa9b057b53b14b4b9b939dd74decc5321", "6ef19b41225c5369f1c104d45d8d85efa9b057b53b14b4b9b939dd74decc5321:sha256"),
        ];
        for (oid, display, debug) in entries {
            assert_eq!(format!("{}", oid), *display);
            assert_eq!(format!("{:?}", oid), *debug);
            assert_eq!(Oid::from_hex(display, oid.algo).unwrap(), *oid);
        }
    }

    #[test]
    fn hex_abbrev_matches_by_prefix() {
        let full = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";
        for n in 4..=full.len() {
            let abbrev = &full[..n];
            let oid = Oid::from_hex_abbrev(abbrev, HashAlgorithm::Sha1, 4).unwrap();
            // The known prefix bytes match the parsed hex; the tail is zero-filled.
            let full_oid = Oid::from_hex(full, HashAlgorithm::Sha1).unwrap();
            let full_bytes = n / 2;
            assert_eq!(oid.hash[..full_bytes], full_oid.hash[..full_bytes]);
            if n % 2 == 1 {
                // Odd length: the trailing high nibble of the next byte matches.
                assert_eq!(oid.hash[full_bytes] >> 4, full_oid.hash[full_bytes] >> 4);
                assert_eq!(oid.hash[full_bytes] & 0x0f, 0);
            } else {
                // Even-length abbreviations display identically.
                assert_eq!(&format!("{}", oid)[..n], abbrev);
            }
        }
        // Too short for the minimum.
        assert!(Oid::from_hex_abbrev(&full[..2], HashAlgorithm::Sha1, 4).is_err());
        // Invalid hex character.
        assert!(Oid::from_hex_abbrev("zzzz", HashAlgorithm::Sha1, 4).is_err());
        // Too long.
        assert!(Oid::from_hex_abbrev(&format!("{full}00"), HashAlgorithm::Sha1, 4).is_err());
    }

    #[test]
    fn hasher_matches_known_object_ids() {
        for algo in all_algos() {
            let tests: &[(&[u8], &Oid)] = &[
                (b"blob 0\0", algo.empty_blob()),
                (b"tree 0\0", algo.empty_tree()),
            ];
            for (data, oid) in tests {
                let mut h = algo.hasher();
                h.update(&data[0..2]);
                h.update(&data[2..]);
                assert_eq!(h.clone().into_oid(), **oid);
                assert_eq!(h.clone().into_vec(), oid.as_slice());

                let mut w = algo.hasher();
                w.write_all(&data[0..2]).unwrap();
                w.write_all(&data[2..]).unwrap();
                assert_eq!(w.into_oid(), **oid);
            }
        }
    }

    #[test]
    fn safety_flags() {
        assert!(!HashAlgorithm::Sha1.hasher().is_safe());
        assert!(HashAlgorithm::Sha256.hasher().is_safe());
    }
}

#[cfg(test)]
mod props {
    use super::{sha1::Sha1, sha256::Sha256, CryptoDigest, HashAlgorithm};
    use proptest::prelude::*;

    proptest! {
        /// Incremental updates must produce the same digest as a single shot,
        /// for both algorithms, over arbitrary byte splits.
        #[test]
        fn incremental_equals_oneshot(data: Vec<u8>, split: usize) {
            let split = split % 17 + 1;

            let mut one = Sha1::new();
            one.update(&data);
            let mut inc = Sha1::new();
            for c in data.chunks(split) {
                inc.update(c);
            }
            prop_assert_eq!(inc.finalize(), one.finalize());

            let mut one = Sha256::new();
            one.update(&data);
            let mut inc = Sha256::new();
            for c in data.chunks(split) {
                inc.update(c);
            }
            prop_assert_eq!(inc.finalize(), one.finalize());
        }

        /// Hashing the serialized form of a blob must yield an oid of the
        /// correct length.
        #[test]
        fn blob_oid_length(data: Vec<u8>) {
            let mut h = HashAlgorithm::Sha1.hasher();
            h.update(b"blob ");
            h.update(data.len().to_string().as_bytes());
            h.update(&[0]);
            h.update(&data);
            let oid = h.into_oid();
            prop_assert_eq!(oid.as_slice().len(), 20);
        }
    }
}
