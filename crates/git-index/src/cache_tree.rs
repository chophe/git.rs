//! The `TREE` index extension: a cached tree hierarchy (port of
//! `cache-tree.c:cache_tree_write` / `cache_tree_read`).
//!
//! Serialized form, one node recursively:
//! ```text
//! <name> NUL
//! <entry_count> <subtree_nr>\n
//! <raw-oid>            (only when entry_count >= 0)
//! <subtree nodes...>
//! ```
//! `entry_count` is the recursive number of index entries covered by the
//! subtree, `subtree_nr` the number of direct child directories. The root
//! node's name is empty.

use git_hash::{HashAlgorithm, Oid};

/// One node of the cached tree hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheTree {
    /// Path component relative to the parent (empty for the root).
    pub name: Vec<u8>,
    /// Recursive index-entry count; `-1` marks an invalid (oid-less) node.
    pub entry_count: i32,
    /// The tree object id (present iff `entry_count >= 0`).
    pub oid: Option<Oid>,
    /// Direct child directories.
    pub subtrees: Vec<CacheTree>,
}

impl CacheTree {
    /// A valid node.
    pub fn new(name: Vec<u8>, entry_count: i32, oid: Oid, subtrees: Vec<CacheTree>) -> CacheTree {
        CacheTree { name, entry_count, oid: Some(oid), subtrees }
    }

    /// Serialize this node and its subtrees.
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write_into(&mut out);
        out
    }

    fn write_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.name);
        out.push(0);
        out.extend_from_slice(format!("{} {}\n", self.entry_count, self.subtrees.len()).as_bytes());
        if self.entry_count >= 0 {
            if let Some(oid) = &self.oid {
                out.extend_from_slice(oid.as_slice());
            }
        }
        for s in &self.subtrees {
            s.write_into(out);
        }
    }

    /// Parse a full cache-tree blob (as stored in the `TREE` extension).
    pub fn parse(data: &[u8], algo: HashAlgorithm) -> Option<CacheTree> {
        let mut pos = 0usize;
        let tree = parse_node(data, &mut pos, algo)?;
        if pos != data.len() {
            return None;
        }
        Some(tree)
    }
}

fn parse_node(data: &[u8], pos: &mut usize, algo: HashAlgorithm) -> Option<CacheTree> {
    // Name up to NUL.
    let nul = data[*pos..].iter().position(|&b| b == 0)?;
    let name = data[*pos..*pos + nul].to_vec();
    *pos += nul + 1;

    let entry_count = parse_int(data, pos)?;
    if data.get(*pos) != Some(&b' ') {
        return None;
    }
    *pos += 1;
    let subtree_nr = parse_int(data, pos)?;
    if subtree_nr < 0 || data.get(*pos) != Some(&b'\n') {
        return None;
    }
    *pos += 1;

    let oid = if entry_count >= 0 {
        let raw = algo.raw_len();
        if data.len() < *pos + raw {
            return None;
        }
        let o = Oid::new(algo, &data[*pos..*pos + raw]);
        *pos += raw;
        Some(o)
    } else {
        None
    };

    let mut subtrees = Vec::with_capacity(subtree_nr as usize);
    for _ in 0..subtree_nr {
        subtrees.push(parse_node(data, pos, algo)?);
    }
    Some(CacheTree { name, entry_count, oid, subtrees })
}

/// Parse a signed decimal integer (C `parse_int`).
fn parse_int(data: &[u8], pos: &mut usize) -> Option<i32> {
    let start = *pos;
    let mut sign = 1i32;
    while data.get(*pos) == Some(&b'-') {
        sign = -sign;
        *pos += 1;
    }
    let mut val: i32 = 0;
    while let Some(&c) = data.get(*pos) {
        if !c.is_ascii_digit() {
            break;
        }
        val = val.wrapping_mul(10).wrapping_add((c - b'0') as i32);
        *pos += 1;
    }
    if *pos == start {
        return None;
    }
    Some(sign * val)
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_hash::HashAlgorithm;

    #[test]
    fn round_trip_nested() {
        let algo = HashAlgorithm::Sha1;
        let root_oid = *algo.empty_tree();
        let sub_oid = *algo.empty_blob();
        let tree = CacheTree::new(
            b"".to_vec(),
            2,
            root_oid,
            vec![CacheTree::new(b"sub".to_vec(), 1, sub_oid, vec![])],
        );
        let bytes = tree.serialize();
        // Root: NUL, "2 1\n", oid, then "sub\0" "1 0\n" oid.
        assert_eq!(bytes[0], 0);
        assert!(bytes.starts_with(b"\x002 1\n"));
        let parsed = CacheTree::parse(&bytes, algo).unwrap();
        assert_eq!(parsed, tree);
    }

    #[test]
    fn invalid_node_has_no_oid() {
        let algo = HashAlgorithm::Sha1;
        let tree = CacheTree { name: b"".to_vec(), entry_count: -1, oid: None, subtrees: vec![] };
        let bytes = tree.serialize();
        assert_eq!(bytes, b"\x00-1 0\n");
        assert_eq!(CacheTree::parse(&bytes, algo).unwrap(), tree);
    }

    #[test]
    fn rejects_trailing_garbage() {
        let algo = HashAlgorithm::Sha1;
        let mut bytes = CacheTree { name: b"".to_vec(), entry_count: -1, oid: None, subtrees: vec![] }
            .serialize();
        bytes.push(0);
        assert!(CacheTree::parse(&bytes, algo).is_none());
    }
}
