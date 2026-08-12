//! Bloom-filter chunks of the commit-graph.
//!
//! A port of the `bloom.c` reader side. The `BDAT` chunk starts with a version
//! byte and a 3-byte big-endian "number of hashes per entry" (stored as
//! `1 << hashes`), followed by the filter entries. The `BIDX` chunk holds a
//! u32 offset into `BDAT` for each commit. Queries are not yet implemented
//! (they require the changed-path hashing, deferred to the revision-walking
//! phase); this module validates and exposes the raw entries.

use git_hash::HashAlgorithm;

use super::commit_graph::{GRAPH_CHUNKID_BLOOMDATA, GRAPH_CHUNKID_BLOOMINDEXES, GraphError};
use crate::chunk_format::ChunkFile;

pub const BLOOM_VERSION_1: u8 = 1;
pub const BLOOM_BYTES_PER_ENTRY: usize = 3; // hash-count field width

/// The bloom-filter data of a commit-graph.
#[derive(Debug, Clone)]
pub struct BloomData {
    /// `1 << hashes` (the 3-byte field value) -> number of hashes used.
    pub num_hashes_log2: u32,
    /// The version byte.
    pub version: u8,
    /// The concatenated filter entries (from `BDAT`, after the 4-byte header).
    pub entries: Vec<u8>,
    /// Per-commit offsets into `entries` (from `BIDX`).
    pub indexes: Vec<u32>,
}

impl BloomData {
    /// Parse the bloom chunks from a chunk file with `num_commits` commits.
    pub fn parse(file: &ChunkFile, num_commits: usize, _algo: HashAlgorithm) -> Result<BloomData, GraphError> {
        let bdat = file
            .chunk(GRAPH_CHUNKID_BLOOMDATA)
            .ok_or_else(|| GraphError::Corrupt("missing bloom data chunk".into()))?;
        let bidx = file
            .chunk(GRAPH_CHUNKID_BLOOMINDEXES)
            .ok_or_else(|| GraphError::Corrupt("missing bloom index chunk".into()))?;
        if bidx.len() != num_commits * 4 {
            return Err(GraphError::Corrupt("bloom index chunk of wrong size".into()));
        }
        if bdat.len() < 4 {
            return Err(GraphError::Corrupt("bloom data chunk too small".into()));
        }
        let version = bdat[0];
        if version != BLOOM_VERSION_1 {
            return Err(GraphError::Corrupt(format!("unsupported bloom filter version {version}")));
        }
        let num_hashes_log2 = u32::from_be_bytes([0, bdat[1], bdat[2], bdat[3]]);
        let mut indexes = Vec::with_capacity(num_commits);
        for i in 0..num_commits {
            indexes.push(u32::from_be_bytes(bidx[i * 4..i * 4 + 4].try_into().unwrap()));
        }
        // Indexes must be monotonic within the entries buffer.
        for w in indexes.windows(2) {
            if w[0] > w[1] {
                return Err(GraphError::Corrupt("bloom index out of order".into()));
            }
        }
        Ok(BloomData {
            num_hashes_log2,
            version,
            entries: bdat[4..].to_vec(),
            indexes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk_format::ChunkFile;

    #[test]
    fn parses_bloom_chunks() {
        let algo = HashAlgorithm::Sha1;
        let mut data = Vec::new();
        let hdr = 0usize;
        let toc_entries = 3;
        let toc_len = toc_entries * 12;
        let bdat_off = hdr + toc_len;
        let bidx_off = bdat_off + 6;
        let trailer_off = bidx_off + 4;
        data.extend_from_slice(&GRAPH_CHUNKID_BLOOMDATA.to_be_bytes());
        data.extend_from_slice(&(bdat_off as u64).to_be_bytes());
        data.extend_from_slice(&GRAPH_CHUNKID_BLOOMINDEXES.to_be_bytes());
        data.extend_from_slice(&(bidx_off as u64).to_be_bytes());
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&(trailer_off as u64).to_be_bytes());
        // BDAT: version 1, hashes field = 3 (=> 8 hashes), one 2-byte entry.
        data.extend_from_slice(&[1, 0, 0, 3, 0xaa, 0xbb]);
        // BIDX: one commit at offset 0.
        data.extend_from_slice(&0u32.to_be_bytes());
        let mut h = algo.hasher();
        h.update(&data);
        data.extend_from_slice(&h.finalize());

        let file = ChunkFile::parse(data, 0, 2, 1, algo).unwrap();
        let b = BloomData::parse(&file, 1, algo).unwrap();
        assert_eq!(b.version, 1);
        assert_eq!(b.num_hashes_log2, 3);
        assert_eq!(b.entries, vec![0xaa, 0xbb]);
        assert_eq!(b.indexes, vec![0]);
    }
}
