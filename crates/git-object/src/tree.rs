//! Tree objects: entry parsing, serialization, and git's entry ordering.
//!
//! A tree is a concatenation of `<mode> <name>\0<raw-oid>` entries. Port of
//! `tree.c` / `tree-walk.c` (entry layout and ordering).

use std::error::Error;
use std::fmt;

use git_hash::{HashAlgorithm, Oid};

/// One tree entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    /// The mode as an octal string, e.g. `100644`, `100755`, `40000`,
    /// `120000` (symlink), `160000` (gitlink).
    pub mode: String,
    /// The entry name (path component). Not necessarily UTF-8.
    pub name: Vec<u8>,
    /// The object id this entry points to.
    pub oid: Oid,
}

impl TreeEntry {
    /// Whether this entry is a directory (mode `40000`).
    pub fn is_dir(&self) -> bool {
        self.mode == "40000"
    }

    /// A `blob`/`tree`/`commit`/`tag` type name for `ls-tree` style output.
    pub fn type_name(&self) -> &'static str {
        if self.mode == "40000" {
            "tree"
        } else if self.mode == "160000" {
            "commit"
        } else {
            "blob"
        }
    }
}

/// Errors from tree parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// An entry was truncated (missing space, NUL, or oid).
    Truncated,
    /// The mode field was not valid octal ASCII.
    BadMode,
}

impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TreeError::Truncated => write!(f, "malformed tree: truncated entry"),
            TreeError::BadMode => write!(f, "malformed tree: invalid mode"),
        }
    }
}

impl Error for TreeError {}

/// Parse the entries of a tree object.
pub fn parse_tree(data: &[u8], algo: HashAlgorithm) -> Result<Vec<TreeEntry>, TreeError> {
    let raw = algo.raw_len();
    let mut entries = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        // Mode, terminated by a space.
        let sp = data[pos..]
            .iter()
            .position(|&b| b == b' ')
            .ok_or(TreeError::Truncated)?;
        let mode = std::str::from_utf8(&data[pos..pos + sp]).map_err(|_| TreeError::BadMode)?;
        if mode.is_empty() || !mode.bytes().all(|b| b.is_ascii_digit()) {
            return Err(TreeError::BadMode);
        }
        pos += sp + 1;
        // Name, terminated by NUL.
        let nul = data[pos..]
            .iter()
            .position(|&b| b == 0)
            .ok_or(TreeError::Truncated)?;
        let name = data[pos..pos + nul].to_vec();
        pos += nul + 1;
        // Object id.
        if data.len() < pos + raw {
            return Err(TreeError::Truncated);
        }
        let oid = Oid::new(algo, &data[pos..pos + raw]);
        pos += raw;
        entries.push(TreeEntry {
            mode: mode.to_string(),
            name,
            oid,
        });
    }
    Ok(entries)
}

/// Compare two tree entry names using git's ordering (`base_name_compare`):
/// a directory sorts as if its name ended with `/`, a fully-consumed
/// non-directory name contributes `\0`, and `'\0' < '.' < '/'`.
pub fn compare_entry_names(a: &[u8], a_dir: bool, b: &[u8], b_dir: bool) -> std::cmp::Ordering {
    let mut i = 0usize;
    loop {
        let ac = if i < a.len() {
            a[i]
        } else if a_dir {
            b'/'
        } else {
            0
        };
        let bc = if i < b.len() {
            b[i]
        } else if b_dir {
            b'/'
        } else {
            0
        };
        if ac == 0 && bc == 0 {
            return std::cmp::Ordering::Equal;
        }
        match ac.cmp(&bc) {
            std::cmp::Ordering::Equal => {
                if i >= a.len() && i >= b.len() {
                    // Both fully consumed; directories compare equal here
                    // (a directory and a file of the same name cannot coexist
                    // in a valid tree).
                    return std::cmp::Ordering::Equal;
                }
                i += 1;
            }
            ord => return ord,
        }
    }
}

/// Serialize entries in git's canonical (sorted) order.
///
/// The caller is responsible for passing complete entries; the tree is built
/// by sorting with [`compare_entry_names`] and writing each entry.
pub fn serialize_tree(entries: &[TreeEntry], algo: HashAlgorithm) -> Result<Vec<u8>, TreeError> {
    let raw = algo.raw_len();
    let mut sorted: Vec<&TreeEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| compare_entry_names(&a.name, a.is_dir(), &b.name, b.is_dir()));

    let mut out = Vec::new();
    for e in &sorted {
        out.extend_from_slice(e.mode.as_bytes());
        out.push(b' ');
        out.extend_from_slice(&e.name);
        out.push(0);
        out.extend_from_slice(e.oid.as_slice());
        let _ = raw;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_serializes_round_trip() {
        let algo = HashAlgorithm::Sha1;
        let data = b"100644 a.txt\0\xe6\x9d\xe2\x9b\xb2\xd1\xd6\x43\x4b\x8b\x29\xae\x77\x5a\xd8\xc2\xe4\x8c\x53\x9140000 dir\0\x4b\x82\x5d\xc6\x42\xcb\x6e\xb9\xa0\x60\xe5\x4b\xf8\xd6\x92\x88\xfb\xee\x49\x04";
        let entries = parse_tree(data, algo).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].mode, "100644");
        assert_eq!(entries[0].name, b"a.txt");
        assert_eq!(entries[1].mode, "40000");
        assert!(entries[1].is_dir());
    }

    #[test]
    fn git_directory_ordering() {
        // Verified against real git: a tree sorts as
        //   foo.txt, foo(dir), foo0, foo2
        // because a dir sorts as "foo/" and '\0' < '.' < '/'.
        assert_eq!(
            compare_entry_names(b"foo", true, b"foo.txt", false),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_entry_names(b"foo.txt", false, b"foo", true),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_entry_names(b"foo", false, b"foo0", false),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_entry_names(b"foo", true, b"foo0", false),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_entry_names(b"a", false, b"a", true),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn rejects_truncated_entries() {
        let algo = HashAlgorithm::Sha1;
        assert_eq!(parse_tree(b"100644 no-oid", algo), Err(TreeError::Truncated));
        assert_eq!(parse_tree(b"100644\0", algo), Err(TreeError::Truncated));
        assert_eq!(parse_tree(b"zzz name\0abc", algo), Err(TreeError::BadMode));
    }

    #[test]
    fn empty_tree_serializes_to_empty() {
        let algo = HashAlgorithm::Sha1;
        assert!(serialize_tree(&[], algo).unwrap().is_empty());
    }

    #[test]
    fn empty_tree_oid_matches_git() {
        let algo = HashAlgorithm::Sha1;
        let entries: Vec<TreeEntry> = Vec::new();
        let data = serialize_tree(&entries, algo).unwrap();
        let tree = crate::Object::from_data(crate::ObjectKind::Tree, data);
        assert_eq!(tree.compute_id(algo), *algo.empty_tree());
    }
}
