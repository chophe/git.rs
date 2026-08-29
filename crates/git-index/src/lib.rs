//! The git index file (version 2: read + write).
//!
//! Port of the `read-cache.c` v2 on-disk format. Entries are padded to an
//! 8-byte boundary; the trailing 20/32-byte checksum covers everything before
//! it. Version 3 (extended flags) and 4 (path compression) are not yet
//! supported.

use std::error::Error;
use std::fmt;
use std::path::Path;

use git_hash::{HashAlgorithm, Oid};

pub const CE_VALID: u16 = 0x8000; // assume-valid
pub const CE_STAGEMASK: u16 = 0x3000;
pub const CE_NAMEMASK: u16 = 0x0fff;

/// One index entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub ctime_sec: u32,
    pub ctime_nsec: u32,
    pub mtime_sec: u32,
    pub mtime_nsec: u32,
    pub dev: u32,
    pub ino: u32,
    /// The file mode (e.g. `0o100644`).
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u32,
    pub oid: Oid,
    pub assume_valid: bool,
    /// Stage number (0, 1, 2, or 3).
    pub stage: u8,
    pub name: String,
}

impl IndexEntry {
    /// A bare-bones entry with zeroed stat data (used when stat info is
    /// unavailable or intentionally skipped).
    pub fn bare(oid: Oid, mode: u32, name: String) -> IndexEntry {
        IndexEntry {
            ctime_sec: 0,
            ctime_nsec: 0,
            mtime_sec: 0,
            mtime_nsec: 0,
            dev: 0,
            ino: 0,
            mode,
            uid: 0,
            gid: 0,
            size: 0,
            oid,
            assume_valid: false,
            stage: 0,
            name,
        }
    }
}

/// The parsed index.
#[derive(Debug, Clone)]
pub struct Index {
    pub version: u32,
    pub entries: Vec<IndexEntry>,
}

impl Default for Index {
    fn default() -> Index {
        Index { version: 2, entries: Vec::new() }
    }
}

/// Errors from index parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexError {
    Io(String),
    Truncated,
    BadMagic,
    UnsupportedVersion(u32),
    BadChecksum,
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IndexError::Io(e) => write!(f, "index I/O error: {e}"),
            IndexError::Truncated => write!(f, "index file truncated"),
            IndexError::BadMagic => write!(f, "index has incorrect signature"),
            IndexError::UnsupportedVersion(v) => write!(f, "unsupported index version {v}"),
            IndexError::BadChecksum => write!(f, "index checksum mismatch"),
        }
    }
}

impl Error for IndexError {}

/// The size of the fixed portion of an entry: 40 stat bytes + raw oid + flags.
fn fixed_size(algo: HashAlgorithm) -> usize {
    40 + algo.raw_len() + 2
}

/// The aligned total size of an entry given its name length.
fn entry_size(algo: HashAlgorithm, name_len: usize) -> usize {
    (fixed_size(algo) + name_len + 8) & !7
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

impl Index {
    /// Read and validate the index file.
    pub fn read(path: &Path, algo: HashAlgorithm) -> Result<Index, IndexError> {
        let data = std::fs::read(path).map_err(|e| IndexError::Io(e.to_string()))?;
        Index::parse(&data, algo)
    }

    /// Parse index bytes.
    pub fn parse(data: &[u8], algo: HashAlgorithm) -> Result<Index, IndexError> {
        let raw = algo.raw_len();
        if data.len() < 12 + raw {
            return Err(IndexError::Truncated);
        }
        if &data[0..4] != b"DIRC" {
            return Err(IndexError::BadMagic);
        }
        let version = be32(&data[4..8]);
        if version != 2 {
            return Err(IndexError::UnsupportedVersion(version));
        }
        let count = be32(&data[8..12]) as usize;

        // Verify the trailing checksum.
        let trailer = data.len() - raw;
        let mut h = algo.hasher();
        h.update(&data[..trailer]);
        if h.finalize() != &data[trailer..] {
            return Err(IndexError::BadChecksum);
        }

        let mut entries = Vec::with_capacity(count);
        let mut pos = 12usize;
        for _ in 0..count {
            if data.len() < pos + fixed_size(algo) {
                return Err(IndexError::Truncated);
            }
            let entry_start = pos;
            let ctime_sec = be32(&data[pos..pos + 4]);
            let ctime_nsec = be32(&data[pos + 4..pos + 8]);
            let mtime_sec = be32(&data[pos + 8..pos + 12]);
            let mtime_nsec = be32(&data[pos + 12..pos + 16]);
            let dev = be32(&data[pos + 16..pos + 20]);
            let ino = be32(&data[pos + 20..pos + 24]);
            let mode = be32(&data[pos + 24..pos + 28]);
            let uid = be32(&data[pos + 28..pos + 32]);
            let gid = be32(&data[pos + 32..pos + 36]);
            let size = be32(&data[pos + 36..pos + 40]);
            pos += 40;
            let oid = Oid::new(algo, &data[pos..pos + raw]);
            pos += raw;
            let flags = u16::from_be_bytes([data[pos], data[pos + 1]]);
            pos += 2;

            let mut name_len = (flags & CE_NAMEMASK) as usize;
            let name = if name_len < CE_NAMEMASK as usize {
                if data.len() < pos + name_len + 1 {
                    return Err(IndexError::Truncated);
                }
                let name = String::from_utf8_lossy(&data[pos..pos + name_len]).into_owned();

                name
            } else {
                // Length was capped; find the NUL.
                let rel = data[pos..]
                    .iter()
                    .position(|&b| b == 0)
                    .ok_or(IndexError::Truncated)?;
                let name = String::from_utf8_lossy(&data[pos..pos + rel]).into_owned();
                name_len = rel;

                name
            };

            entries.push(IndexEntry {
                ctime_sec,
                ctime_nsec,
                mtime_sec,
                mtime_nsec,
                dev,
                ino,
                mode,
                uid,
                gid,
                size,
                oid,
                assume_valid: flags & CE_VALID != 0,
                stage: ((flags & CE_STAGEMASK) >> 12) as u8,
                name,
            });

            // Advance to the next 8-byte boundary.
            pos = entry_start + entry_size(algo, name_len);
            if pos > data.len() {
                return Err(IndexError::Truncated);
            }
        }

        Ok(Index { version, entries })
    }

    /// Serialize the index (version 2) with a trailing checksum.
    pub fn to_bytes(&self, algo: HashAlgorithm) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"DIRC");
        out.extend_from_slice(&2u32.to_be_bytes());
        out.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());

        for e in &self.entries {
            let start = out.len();
            out.extend_from_slice(&e.ctime_sec.to_be_bytes());
            out.extend_from_slice(&e.ctime_nsec.to_be_bytes());
            out.extend_from_slice(&e.mtime_sec.to_be_bytes());
            out.extend_from_slice(&e.mtime_nsec.to_be_bytes());
            out.extend_from_slice(&e.dev.to_be_bytes());
            out.extend_from_slice(&e.ino.to_be_bytes());
            out.extend_from_slice(&e.mode.to_be_bytes());
            out.extend_from_slice(&e.uid.to_be_bytes());
            out.extend_from_slice(&e.gid.to_be_bytes());
            out.extend_from_slice(&e.size.to_be_bytes());
            out.extend_from_slice(e.oid.as_slice());
            let name_len = e.name.len().min(CE_NAMEMASK as usize);
            let mut flags = if e.assume_valid { CE_VALID } else { 0 };
            flags |= (e.stage as u16) << 12;
            flags |= name_len as u16;
            out.extend_from_slice(&flags.to_be_bytes());
            out.extend_from_slice(e.name.as_bytes());
            out.push(0);
            // Pad to 8-byte alignment.
            let end = start + entry_size(algo, e.name.len());
            while out.len() < end {
                out.push(0);
            }
        }

        let mut h = algo.hasher();
        h.update(&out);
        out.extend_from_slice(&h.finalize());
        out
    }

    /// Write the index atomically (temp file + rename).
    pub fn write(&self, path: &Path, algo: HashAlgorithm) -> Result<(), IndexError> {
        let data = self.to_bytes(algo);
        let tmp = path.with_extension(format!("lock.{}", std::process::id()));
        std::fs::write(&tmp, &data).map_err(|e| IndexError::Io(e.to_string()))?;
        std::fs::rename(&tmp, path).map_err(|e| IndexError::Io(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Index {
        let algo = HashAlgorithm::Sha1;
        let e1 = IndexEntry::bare(*algo.empty_blob(), 0o100644, "a.txt".to_string());
        let e2 = IndexEntry {
            mode: 0o100644,
            oid: *algo.empty_tree(),
            name: "sub/dir/b.txt".to_string(),
            stage: 1,
            ..IndexEntry::bare(*algo.empty_tree(), 0o100644, "".to_string())
        };
        Index {
            version: 2,
            entries: vec![e1, e2],
        }
    }

    #[test]
    fn round_trips() {
        let algo = HashAlgorithm::Sha1;
        let idx = sample();
        let bytes = idx.to_bytes(algo);
        let parsed = Index::parse(&bytes, algo).unwrap();
        assert_eq!(parsed.entries, idx.entries);
        assert_eq!(parsed.entries[1].stage, 1);
    }

    #[test]
    fn empty_index_round_trips() {
        let algo = HashAlgorithm::Sha1;
        let idx = Index { version: 2, entries: vec![] };
        let bytes = idx.to_bytes(algo);
        let parsed = Index::parse(&bytes, algo).unwrap();
        assert_eq!(parsed.entries.len(), 0);
    }

    #[test]
    fn rejects_bad_checksum() {
        let algo = HashAlgorithm::Sha1;
        let mut bytes = sample().to_bytes(algo);
        let n = bytes.len();
        bytes[n - 3] ^= 0xff;
        assert!(matches!(Index::parse(&bytes, algo), Err(IndexError::BadChecksum)));
    }

    #[test]
    fn rejects_bad_magic() {
        let algo = HashAlgorithm::Sha1;
        let mut bytes = sample().to_bytes(algo);
        bytes[0] = b'X';
        assert!(matches!(Index::parse(&bytes, algo), Err(IndexError::BadMagic)));
    }

    #[test]
    fn long_names_are_capped() {
        let algo = HashAlgorithm::Sha1;
        let long_name = "x".repeat(0x2000);
        let idx = Index {
            version: 2,
            entries: vec![IndexEntry::bare(*algo.empty_blob(), 0o100644, long_name)],
        };
        let bytes = idx.to_bytes(algo);
        let parsed = Index::parse(&bytes, algo).unwrap();
        assert_eq!(parsed.entries[0].name.len(), 0x2000);
    }
}