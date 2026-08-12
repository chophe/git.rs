//! Chunk-based file formats (commit-graph, multi-pack-index, etc.).
//!
//! A chunk file is a fixed-size header followed by a table of contents of
//! `(chunk_id u32, chunk_offset u64)` entries (one per chunk, plus a trailing
//! terminator entry whose id is 0), then the chunk payloads, then a trailing
//! hash of everything before it. Port of `chunk-format.c`.

use std::error::Error;
use std::fmt;

use git_hash::{CryptoDigest, HashAlgorithm};

pub const CHUNK_TOC_ENTRY_SIZE: usize = 12;

/// Errors from chunk-file parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkError {
    Truncated,
    BadChecksum,
    Corrupt(String),
}

impl fmt::Display for ChunkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChunkError::Truncated => write!(f, "chunk file truncated"),
            ChunkError::BadChecksum => write!(f, "chunk file checksum mismatch"),
            ChunkError::Corrupt(m) => write!(f, "corrupt chunk file: {m}"),
        }
    }
}

impl Error for ChunkError {}

/// A parsed chunk file: the raw bytes plus the chunk table of contents.
#[derive(Debug, Clone)]
pub struct ChunkFile {
    data: Vec<u8>,
    chunks: Vec<(u32, usize, usize)>, // (id, start, size)
}

impl ChunkFile {
    /// Parse a chunk file.
    ///
    /// `header_size` is the fixed header preceding the table of contents,
    /// `num_chunks` the number of chunk entries (the trailing terminator is
    /// implicit), `alignment` the required alignment of chunk offsets, and
    /// `algo` the hash used for the trailing checksum.
    pub fn parse(
        data: Vec<u8>,
        header_size: usize,
        num_chunks: usize,
        alignment: usize,
        algo: HashAlgorithm,
    ) -> Result<ChunkFile, ChunkError> {
        let raw = algo.raw_len();
        if data.len() < raw {
            return Err(ChunkError::Truncated);
        }
        let trailer_off = data.len() - raw;

        // Verify the trailing checksum (hash of everything before it).
        let mut h = algo.hasher();
        h.update(&data[..trailer_off]);
        if h.finalize() != &data[trailer_off..] {
            return Err(ChunkError::BadChecksum);
        }

        let toc_start = header_size;
        let toc_end = toc_start.checked_add((num_chunks + 1) * CHUNK_TOC_ENTRY_SIZE)
            .ok_or(ChunkError::Truncated)?;
        if toc_end > trailer_off {
            return Err(ChunkError::Truncated);
        }
        let toc = &data[toc_start..toc_end];

        let mut chunks = Vec::with_capacity(num_chunks);
        for i in 0..num_chunks {
            let e = &toc[i * CHUNK_TOC_ENTRY_SIZE..(i + 1) * CHUNK_TOC_ENTRY_SIZE];
            let id = u32::from_be_bytes([e[0], e[1], e[2], e[3]]);
            let off = u64::from_be_bytes(e[4..12].try_into().unwrap()) as usize;
            if id == 0 {
                return Err(ChunkError::Corrupt("terminating chunk id appears earlier than expected".into()));
            }
            if off % alignment != 0 {
                return Err(ChunkError::Corrupt(format!("chunk id {id:#x} not aligned")));
            }
            let next = &toc[(i + 1) * CHUNK_TOC_ENTRY_SIZE..(i + 2) * CHUNK_TOC_ENTRY_SIZE];
            let next_off = u64::from_be_bytes(next[4..12].try_into().unwrap()) as usize;
            if next_off < off || next_off > trailer_off {
                return Err(ChunkError::Corrupt("improper chunk offsets".into()));
            }
            if chunks.iter().any(|(id2, _, _)| *id2 == id) {
                return Err(ChunkError::Corrupt(format!("duplicate chunk id {id:#x}")));
            }
            chunks.push((id, off, next_off - off));
        }

        let trailing = &toc[num_chunks * CHUNK_TOC_ENTRY_SIZE..(num_chunks + 1) * CHUNK_TOC_ENTRY_SIZE];
        let tid = u32::from_be_bytes([trailing[0], trailing[1], trailing[2], trailing[3]]);
        if tid != 0 {
            return Err(ChunkError::Corrupt("final chunk has non-zero id".into()));
        }

        Ok(ChunkFile { data, chunks })
    }

    /// The bytes of a chunk by id, if present.
    pub fn chunk(&self, id: u32) -> Option<&[u8]> {
        self.chunk_range(id).map(|(s, z)| &self.data[s..s + z])
    }

    /// The (start, size) of a chunk by id, if present.
    pub fn chunk_range(&self, id: u32) -> Option<(usize, usize)> {
        self.chunks.iter().find(|(i, _, _)| *i == id).map(|(_, s, z)| (*s, *z))
    }

    pub fn num_chunks(&self) -> usize {
        self.chunks.len()
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be32(v: u32) -> [u8; 4] {
        v.to_be_bytes()
    }
    fn be64(v: u64) -> [u8; 8] {
        v.to_be_bytes()
    }

    /// Build a small chunk file with two chunks ("AAAA", "BBBB").
    fn build(algo: HashAlgorithm) -> Vec<u8> {
        let mut out = Vec::new();
        let hdr = 4usize;
        let toc_entries = 3; // 2 chunks + trailing
        let toc_len = toc_entries * CHUNK_TOC_ENTRY_SIZE;
        let a_start = hdr + toc_len;
        let b_start = a_start + 4;
        let trailer_off = b_start + 6;

        out.extend_from_slice(b"HDR!");
        out.extend_from_slice(&be32(0x4141_4141));
        out.extend_from_slice(&be64(a_start as u64));
        out.extend_from_slice(&be32(0x4242_4242));
        out.extend_from_slice(&be64(b_start as u64));
        out.extend_from_slice(&be32(0));
        out.extend_from_slice(&be64(trailer_off as u64));
        out.extend_from_slice(b"AAAA");
        out.extend_from_slice(b"BBBBBB");
        let mut h = algo.hasher();
        h.update(&out);
        out.extend_from_slice(&h.finalize());
        out
    }

    #[test]
    fn parses_chunks_and_verifies_checksum() {
        let algo = HashAlgorithm::Sha1;
        let data = build(algo);
        let cf = ChunkFile::parse(data, 4, 2, 4, algo).unwrap();
        assert_eq!(cf.num_chunks(), 2);
        assert_eq!(cf.chunk(0x4141_4141).unwrap(), b"AAAA");
        assert_eq!(cf.chunk(0x4242_4242).unwrap(), b"BBBBBB");
        assert_eq!(cf.chunk(0xdead_beef), None);
    }

    #[test]
    fn rejects_bad_checksum() {
        let algo = HashAlgorithm::Sha1;
        let mut data = build(algo);
        let n = data.len();
        data[n - 5] ^= 0xff;
        assert_eq!(ChunkFile::parse(data, 4, 2, 4, algo), Err(ChunkError::BadChecksum));
    }

    #[test]
    fn rejects_unaligned_offsets() {
        let algo = HashAlgorithm::Sha1;
        let mut data = build(algo);
        // Corrupt an offset to be non-multiple of 4.
        data[4 + 4] = 1;
        let cf = ChunkFile::parse(data, 4, 2, 4, algo);
        assert!(matches!(cf, Err(ChunkError::Corrupt(_))));
    }
}
