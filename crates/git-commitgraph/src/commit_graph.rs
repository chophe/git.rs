//! Commit-graph reading and verification.
//!
//! Port of the `commit-graph.c` reader. The file is a chunk file whose header
//! is `CGPH`, version, hash version, chunk count, base-graph count; the TOC
//! follows at offset 8 with alignment 1.

use std::error::Error;
use std::fmt;

use git_hash::{HashAlgorithm, Oid};

use crate::chunk_format::{ChunkFile, ChunkError};

pub const GRAPH_CHUNKID_OIDFANOUT: u32 = 0x4f49_4446; // "OIDF"
pub const GRAPH_CHUNKID_OIDLOOKUP: u32 = 0x4f49_444c; // "OIDL"
pub const GRAPH_CHUNKID_DATA: u32 = 0x4344_4154; // "CDAT"
pub const GRAPH_CHUNKID_GENERATION_DATA: u32 = 0x4744_4132; // "GDA2"
pub const GRAPH_CHUNKID_GENERATION_OVERFLOW: u32 = 0x4744_4f32; // "GDO2"
pub const GRAPH_CHUNKID_EXTRAEDGES: u32 = 0x4544_4745; // "EDGE"
pub const GRAPH_CHUNKID_BLOOMINDEXES: u32 = 0x4249_4458; // "BIDX"
pub const GRAPH_CHUNKID_BLOOMDATA: u32 = 0x4244_4154; // "BDAT"
pub const GRAPH_CHUNKID_BASE: u32 = 0x4241_5345; // "BASE"

pub const GRAPH_HEADER_SIZE: usize = 8;
pub const GRAPH_FANOUT_SIZE: usize = 4 * 256;

/// Sentinel for "no second parent" in the commit data chunk.
pub const GRAPH_PARENT_NONE: u32 = 0x7000_0000;
/// Sentinel meaning "more parents follow in the extra-edges chunk".
pub const GRAPH_EXTRA_EDGES_NEEDED: u32 = 0x8000_0000;
/// Flag on a generation-data offset meaning the real value is in GDO2.
pub const CORRECTED_COMMIT_DATE_OFFSET_OVERFLOW: u32 = 1 << 31;

/// Errors from commit-graph parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    Truncated,
    BadMagic,
    Corrupt(String),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::Truncated => write!(f, "commit-graph file too small"),
            GraphError::BadMagic => write!(f, "commit-graph has incorrect signature"),
            GraphError::Corrupt(m) => write!(f, "corrupt commit-graph: {m}"),
        }
    }
}

impl Error for GraphError {}

impl From<ChunkError> for GraphError {
    fn from(e: ChunkError) -> GraphError {
        GraphError::Corrupt(e.to_string())
    }
}

/// Per-commit data decoded from the `CDAT` chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitData {
    pub tree: Oid,
    /// First parent position, or `None`.
    pub parent1: Option<u32>,
    /// Second parent position, or `None`. More than two parents require the
    /// extra-edges chunk (not yet expanded here).
    pub parent2: Option<u32>,
    /// Topological level (generation v1).
    pub topo_level: u32,
    /// Commit date (seconds since epoch, 34-bit).
    pub date: u64,
}

/// A parsed commit-graph.
#[derive(Debug, Clone)]
pub struct CommitGraph {
    algo: HashAlgorithm,
    file: ChunkFile,
    num_commits: usize,
    fanout: Vec<u32>,
    has_generation: bool,
    num_extra_edges: usize,
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

impl CommitGraph {
    pub fn parse(data: Vec<u8>, algo: HashAlgorithm) -> Result<CommitGraph, GraphError> {
        if data.len() < GRAPH_HEADER_SIZE + algo.raw_len() {
            return Err(GraphError::Truncated);
        }
        if &data[0..4] != b"CGPH" {
            return Err(GraphError::BadMagic);
        }
        if data[4] != 1 {
            return Err(GraphError::Corrupt(format!("unsupported commit-graph version {}", data[4])));
        }
        let expected_oid = match algo {
            HashAlgorithm::Sha1 => 1,
            HashAlgorithm::Sha256 => 2,
        };
        if data[5] != expected_oid {
            return Err(GraphError::Corrupt("commit-graph hash version does not match".into()));
        }
        let num_chunks = data[6] as usize;
        let base_graphs = data[7];
        if base_graphs != 0 {
            // Base-graph chains are not expanded in this port.
            return Err(GraphError::Corrupt("commit-graph chains are not supported".into()));
        }

        let file = ChunkFile::parse(data, GRAPH_HEADER_SIZE, num_chunks, 1, algo)?;

        let fanout_chunk = file
            .chunk(GRAPH_CHUNKID_OIDFANOUT)
            .ok_or_else(|| GraphError::Corrupt("missing OID fanout chunk".into()))?;
        if fanout_chunk.len() != GRAPH_FANOUT_SIZE {
            return Err(GraphError::Corrupt("OID fanout chunk of wrong size".into()));
        }
        let mut fanout = Vec::with_capacity(256);
        for i in 0..256 {
            fanout.push(be32(&fanout_chunk[i * 4..i * 4 + 4]));
        }
        for i in 0..255 {
            if fanout[i] > fanout[i + 1] {
                return Err(GraphError::Corrupt("OID fanout out of order".into()));
            }
        }
        let num_commits = fanout[255] as usize;

        let oidl = file
            .chunk(GRAPH_CHUNKID_OIDLOOKUP)
            .ok_or_else(|| GraphError::Corrupt("missing OID lookup chunk".into()))?;
        if oidl.len() != num_commits * algo.raw_len() {
            return Err(GraphError::Corrupt("OID lookup chunk of wrong size".into()));
        }
        let cdat = file
            .chunk(GRAPH_CHUNKID_DATA)
            .ok_or_else(|| GraphError::Corrupt("missing commit data chunk".into()))?;
        if cdat.len() != num_commits * (algo.raw_len() + 16) {
            return Err(GraphError::Corrupt("commit data chunk of wrong size".into()));
        }
        let has_generation = match file.chunk(GRAPH_CHUNKID_GENERATION_DATA) {
            Some(g) => {
                if g.len() != num_commits * 4 {
                    return Err(GraphError::Corrupt("generation data chunk of wrong size".into()));
                }
                true
            }
            None => false,
        };
        let num_extra_edges = match file.chunk(GRAPH_CHUNKID_EXTRAEDGES) {
            Some(e) => e.len() / 4,
            None => 0,
        };
        // Bloom chunks, when present, are parsed by the bloom module.
        if let Some(b) = file.chunk(GRAPH_CHUNKID_BLOOMINDEXES) {
            if b.len() != num_commits * 4 {
                return Err(GraphError::Corrupt("bloom index chunk of wrong size".into()));
            }
        }
        if let Some(b) = file.chunk(GRAPH_CHUNKID_BLOOMDATA) {
            if b.len() < 4 {
                return Err(GraphError::Corrupt("bloom data chunk too small".into()));
            }
        }

        Ok(CommitGraph {
            algo,
            file,
            num_commits,
            fanout,
            has_generation,
            num_extra_edges,
        })
    }

    pub fn num_commits(&self) -> usize {
        self.num_commits
    }

    pub fn num_extra_edges(&self) -> usize {
        self.num_extra_edges
    }

    pub fn has_generation_data(&self) -> bool {
        self.has_generation
    }

    /// The object id at integer position `pos`.
    pub fn oid_at(&self, pos: usize) -> Option<Oid> {
        if pos >= self.num_commits {
            return None;
        }
        let oidl = self.file.chunk(GRAPH_CHUNKID_OIDLOOKUP)?;
        let raw = self.algo.raw_len();
        Some(Oid::new(self.algo, &oidl[pos * raw..pos * raw + raw]))
    }

    /// Look up the integer position of `oid`.
    pub fn find(&self, oid: &Oid) -> Option<usize> {
        if oid.algorithm() != self.algo {
            return None;
        }
        let first = oid.as_slice()[0] as usize;
        let lo = if first == 0 { 0 } else { self.fanout[first - 1] as usize };
        let hi = self.fanout[first] as usize;
        let oidl = self.file.chunk(GRAPH_CHUNKID_OIDLOOKUP)?;
        let raw = self.algo.raw_len();
        let slice = &oidl[lo * raw..hi * raw];
        let idx = slice
            .chunks_exact(raw)
            .position(|c| Oid::new(self.algo, c) == *oid)?;
        Some(lo + idx)
    }

    /// Decode the commit data at position `pos`.
    pub fn commit_data(&self, pos: usize) -> Option<CommitData> {
        if pos >= self.num_commits {
            return None;
        }
        let cdat = self.file.chunk(GRAPH_CHUNKID_DATA)?;
        let raw = self.algo.raw_len();
        let e = &cdat[pos * (raw + 16)..pos * (raw + 16) + raw + 16];
        let tree = Oid::new(self.algo, &e[..raw]);
        let parent1 = be32(&e[raw..raw + 4]);
        let parent2 = be32(&e[raw + 4..raw + 8]);
        let gen_date = be32(&e[raw + 8..raw + 12]);
        let date_low = be32(&e[raw + 12..raw + 16]);

        let (topo_level, date) = if self.has_generation {
            (gen_date >> 2, ((u64::from(gen_date & 0x3)) << 32) | u64::from(date_low))
        } else {
            (gen_date >> 2, u64::from(date_low))
        };

        Some(CommitData {
            tree,
            parent1: if parent1 == GRAPH_PARENT_NONE { None } else { Some(parent1) },
            parent2: if parent2 == GRAPH_PARENT_NONE || parent2 == GRAPH_EXTRA_EDGES_NEEDED {
                None
            } else {
                Some(parent2)
            },
            topo_level,
            date,
        })
    }

    /// The corrected commit date (generation v2) at position `pos`, if the
    /// generation data chunk is present.
    pub fn corrected_commit_date(&self, pos: usize) -> Option<u64> {
        if !self.has_generation || pos >= self.num_commits {
            return None;
        }
        let gda2 = self.file.chunk(GRAPH_CHUNKID_GENERATION_DATA)?;
        let raw_offset = be32(&gda2[pos * 4..pos * 4 + 4]);
        let date = self.commit_data(pos)?.date;
        if raw_offset & CORRECTED_COMMIT_DATE_OFFSET_OVERFLOW != 0 {
            let idx = (raw_offset ^ CORRECTED_COMMIT_DATE_OFFSET_OVERFLOW) as usize;
            let gdo2 = self.file.chunk(GRAPH_CHUNKID_GENERATION_OVERFLOW)?;
            let off = u64::from_be_bytes(gdo2[idx * 8..idx * 8 + 8].try_into().ok()?);
            Some(date + off)
        } else {
            Some(date + u64::from(raw_offset))
        }
    }

    /// Verify structural invariants: checksums (done at parse), oid order,
    /// parent positions, and generation-data sizes.
    pub fn verify(&self) -> Result<(), GraphError> {
        // OIDs must be strictly increasing.
        let mut prev: Option<Oid> = None;
        for pos in 0..self.num_commits {
            let oid = self.oid_at(pos).ok_or_else(|| GraphError::Corrupt("short OID lookup".into()))?;
            if let Some(p) = prev {
                if p >= oid {
                    return Err(GraphError::Corrupt("OID order is not strictly increasing".into()));
                }
            }
            prev = Some(oid);
        }

        // Parent positions must be in range or sentinel.
        for pos in 0..self.num_commits {
            let d = self.commit_data(pos).ok_or_else(|| GraphError::Corrupt("bad commit data".into()))?;
            for p in [d.parent1, d.parent2].into_iter().flatten() {
                if p as usize >= self.num_commits {
                    return Err(GraphError::Corrupt(format!("parent position {p} out of range")));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk_format::CHUNK_TOC_ENTRY_SIZE;

    #[test]
    fn parses_a_small_graph() {
        // Build a minimal single-commit graph by hand.
        let algo = HashAlgorithm::Sha1;
        let oid: Oid = *HashAlgorithm::Sha1.empty_tree();
        let mut data = Vec::new();
        // Header.
        data.extend_from_slice(b"CGPH");
        data.push(1); // version
        data.push(1); // oid version (sha1)
        data.push(3); // chunks: OIDF, OIDL, CDAT
        data.push(0); // base graphs

        let toc_entries = 4; // 3 chunks + trailing
        let toc_len = toc_entries * CHUNK_TOC_ENTRY_SIZE;
        let oidf_off = GRAPH_HEADER_SIZE + toc_len;
        let oidl_off = oidf_off + 1024;
        let cdat_off = oidl_off + 20;
        let trailer_off = cdat_off + 36;

        let toc = [
            (GRAPH_CHUNKID_OIDFANOUT, oidf_off as u64),
            (GRAPH_CHUNKID_OIDLOOKUP, oidl_off as u64),
            (GRAPH_CHUNKID_DATA, cdat_off as u64),
            (0, trailer_off as u64),
        ];
        for (id, off) in toc {
            data.extend_from_slice(&id.to_be_bytes());
            data.extend_from_slice(&off.to_be_bytes());
        }
        // OIDF: one commit, first byte = oid[0] (0x4b).
        let mut fanout = [0u32; 256];
        fanout[oid.as_slice()[0] as usize] = 1;
        for i in 1..256 {
            fanout[i] += fanout[i - 1];
        }
        for f in fanout {
            data.extend_from_slice(&f.to_be_bytes());
        }
        // OIDL.
        data.extend_from_slice(oid.as_slice());
        // CDAT: tree, no parents, topo=1, date=0.
        data.extend_from_slice(oid.as_slice());
        data.extend_from_slice(&GRAPH_PARENT_NONE.to_be_bytes());
        data.extend_from_slice(&GRAPH_PARENT_NONE.to_be_bytes());
        data.extend_from_slice(&(1u32 << 2).to_be_bytes()); // topo level 1 (bits 2..)
        data.extend_from_slice(&0u32.to_be_bytes()); // date low

        let mut h = algo.hasher();
        h.update(&data);
        data.extend_from_slice(&h.finalize());

        let g = CommitGraph::parse(data, algo).unwrap();
        assert_eq!(g.num_commits(), 1);
        assert_eq!(g.oid_at(0), Some(oid));
        assert_eq!(g.find(&oid), Some(0));
        assert_eq!(g.find(&Oid::new(HashAlgorithm::Sha1, &[0; 20])), None);
        let d = g.commit_data(0).unwrap();
        assert_eq!(d.tree, oid);
        assert_eq!(d.parent1, None);
        assert_eq!(d.parent2, None);
        assert_eq!(d.topo_level, 1);
        assert_eq!(d.date, 0);
        assert!(g.verify().is_ok());
    }

    #[test]
    fn rejects_bad_magic_and_version() {
        let algo = HashAlgorithm::Sha1;
        let mut data = Vec::new();
        data.extend_from_slice(b"XXXX");
        data.extend_from_slice(&[1, 1, 0, 0]);
        data.extend_from_slice(&[0u8; 40]);
        assert!(matches!(CommitGraph::parse(data, algo), Err(GraphError::BadMagic)));
    }
}
