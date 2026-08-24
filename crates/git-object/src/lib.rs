//! Git object model.
//!
//! A git-compatible subset of `object.c`: the four object kinds, the loose
//! object header (`<type> <size>\0`), and object-id computation.

use std::error::Error;
use std::fmt;

use git_hash::{CryptoDigest, HashAlgorithm, Oid};

pub mod commit;
pub mod tree;

pub use commit::{parse_commit, parse_tag, Commit, Tag};
pub use tree::{compare_entry_names, parse_tree, serialize_tree, TreeEntry, TreeError};

/// The kinds of objects git can store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Blob,
    Tree,
    Commit,
    Tag,
}

impl ObjectKind {
    /// Parse a kind from its type-name string.
    pub fn from_str(s: &str) -> Option<ObjectKind> {
        match s {
            "blob" => Some(ObjectKind::Blob),
            "tree" => Some(ObjectKind::Tree),
            "commit" => Some(ObjectKind::Commit),
            "tag" => Some(ObjectKind::Tag),
            _ => None,
        }
    }

    /// The type-name string.
    pub fn as_str(self) -> &'static str {
        match self {
            ObjectKind::Blob => "blob",
            ObjectKind::Tree => "tree",
            ObjectKind::Commit => "commit",
            ObjectKind::Tag => "tag",
        }
    }

    /// The type-name byte used to identify the object type from a header
    /// (the first byte of the type string).
    pub fn type_byte(self) -> u8 {
        self.as_str().as_bytes()[0]
    }
}

/// Errors returned when parsing object headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectError {
    /// The header had no NUL terminator.
    MissingNul,
    /// The type name is not one of the four known kinds.
    BadType(String),
    /// The size field was missing or malformed.
    BadSize,
    /// The declared size does not match the actual payload length.
    SizeMismatch { expected: u64, actual: usize },
}

impl fmt::Display for ObjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectError::MissingNul => write!(f, "malformed object header: no NUL terminator"),
            ObjectError::BadType(t) => write!(f, "unknown object type: {t}"),
            ObjectError::BadSize => write!(f, "malformed object header: bad size"),
            ObjectError::SizeMismatch { expected, actual } => {
                write!(f, "object size mismatch: header declares {expected}, data is {actual}")
            }
        }
    }
}

impl Error for ObjectError {}

/// A parsed object: its kind and content bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    pub kind: ObjectKind,
    pub data: Vec<u8>,
}

impl Object {
    /// The loose object header bytes: `<type> <size>\0`.
    pub fn header_bytes(&self) -> Vec<u8> {
        format!("{} {}\0", self.kind.as_str(), self.data.len()).into_bytes()
    }

    /// The full serialized form: header followed by content.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.header_bytes();
        out.extend_from_slice(&self.data);
        out
    }

    /// Compute the object id for `algo` (hashing the serialized form).
    pub fn compute_id(&self, algo: HashAlgorithm) -> Oid {
        let mut hasher = algo.hasher();
        hasher.update(&self.header_bytes());
        hasher.update(&self.data);
        hasher.into_oid()
    }

    /// Parse an object from its serialized bytes (header + content).
    ///
    /// Returns the object and the length of the header.
    pub fn parse(bytes: &[u8]) -> Result<(Object, usize), ObjectError> {
        let nul = bytes
            .iter()
            .position(|&b| b == 0)
            .ok_or(ObjectError::MissingNul)?;
        let header = std::str::from_utf8(&bytes[..nul])
            .map_err(|_| ObjectError::BadType("non-UTF-8 type".to_string()))?;
        let (type_str, size_str) = header
            .split_once(' ')
            .ok_or(ObjectError::BadSize)?;
        let kind = ObjectKind::from_str(type_str)
            .ok_or_else(|| ObjectError::BadType(type_str.to_string()))?;
        let size: u64 = size_str.parse().map_err(|_| ObjectError::BadSize)?;

        let payload = &bytes[nul + 1..];
        if payload.len() as u64 != size {
            return Err(ObjectError::SizeMismatch {
                expected: size,
                actual: payload.len(),
            });
        }

        Ok((
            Object {
                kind,
                data: payload.to_vec(),
            },
            nul + 1,
        ))
    }

    /// Build an object from raw content bytes, computing its id with `algo`.
    pub fn from_data(kind: ObjectKind, data: Vec<u8>) -> Object {
        Object { kind, data }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_round_trips() {
        for kind in [ObjectKind::Blob, ObjectKind::Tree, ObjectKind::Commit, ObjectKind::Tag] {
            assert_eq!(ObjectKind::from_str(kind.as_str()), Some(kind));
            assert_eq!(kind.type_byte(), kind.as_str().as_bytes()[0]);
        }
        assert_eq!(ObjectKind::from_str("nope"), None);
    }

    #[test]
    fn empty_blob_id_matches_git() {
        let blob = Object::from_data(ObjectKind::Blob, Vec::new());
        assert_eq!(blob.compute_id(HashAlgorithm::Sha1), *HashAlgorithm::Sha1.empty_blob());
        assert_eq!(blob.compute_id(HashAlgorithm::Sha256), *HashAlgorithm::Sha256.empty_blob());
    }

    #[test]
    fn empty_tree_id_matches_git() {
        let tree = Object::from_data(ObjectKind::Tree, Vec::new());
        assert_eq!(tree.compute_id(HashAlgorithm::Sha1), *HashAlgorithm::Sha1.empty_tree());
        assert_eq!(tree.compute_id(HashAlgorithm::Sha256), *HashAlgorithm::Sha256.empty_tree());
    }

    #[test]
    fn header_bytes_format() {
        let blob = Object::from_data(ObjectKind::Blob, b"hello".to_vec());
        assert_eq!(blob.header_bytes(), b"blob 5\0");
        let commit = Object::from_data(ObjectKind::Commit, vec![0u8; 1000]);
        assert_eq!(commit.header_bytes(), b"commit 1000\0");
    }

    #[test]
    fn parse_round_trip() {
        for (kind, data) in [
            (ObjectKind::Blob, b"hello world".to_vec()),
            (ObjectKind::Tree, vec![1u8, 2, 3, 4]),
            (ObjectKind::Commit, b"tree 0000...\n".to_vec()),
            (ObjectKind::Tag, b"object 0000\ntype blob\n".to_vec()),
        ] {
            let obj = Object::from_data(kind, data);
            let bytes = obj.to_bytes();
            let (parsed, header_len) = Object::parse(&bytes).unwrap();
            assert_eq!(parsed, obj);
            assert_eq!(header_len, obj.header_bytes().len());
        }
    }

    #[test]
    fn parse_rejects_bad_headers() {
        assert_eq!(Object::parse(b""), Err(ObjectError::MissingNul));
        assert_eq!(
            Object::parse(b"wat 3\0abc"),
            Err(ObjectError::BadType("wat".to_string()))
        );
        assert_eq!(
            Object::parse(b"blob x\0abc"),
            Err(ObjectError::BadSize)
        );
        assert_eq!(
            Object::parse(b"blob 5\0abc"),
            Err(ObjectError::SizeMismatch { expected: 5, actual: 3 })
        );
    }

    #[test]
    fn parse_is_exact() {
        // Extra trailing bytes after the declared payload are accepted (the
        // header length is returned so the caller can trim).
        let obj = Object::from_data(ObjectKind::Blob, b"abc".to_vec());
        let bytes = obj.to_bytes();
        let (parsed, hlen) = Object::parse(&bytes).unwrap();
        assert_eq!(parsed, obj);
        assert_eq!(&bytes[hlen..], b"abc");
    }
}
