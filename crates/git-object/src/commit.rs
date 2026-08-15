//! Commit and tag object parsing.

use std::error::Error;
use std::fmt;

use git_hash::{HashAlgorithm, Oid};

use crate::ObjectKind;

/// Errors from commit/tag parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderError(pub String);

impl fmt::Display for HeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "malformed object header: {}", self.0)
    }
}

impl Error for HeaderError {}

/// Split an object into its header lines (with continuation handling) and the
/// message body that follows the first blank line.
///
/// Returns `(headers, message)`. Continuation lines (leading whitespace) are
/// joined to the previous header with a `\n`.
pub fn parse_headers(data: &[u8]) -> (Vec<(String, Vec<u8>)>, Vec<u8>) {
    let mut headers: Vec<(String, Vec<u8>)> = Vec::new();
    let mut message = Vec::new();

    let mut lines = data.split_inclusive(|&b| b == b'\n');
    let mut saw_blank = false;
    while let Some(line) = lines.next() {
        if saw_blank {
            message.extend_from_slice(line);
            continue;
        }
        let line_body = line.strip_suffix(b"\n").unwrap_or(line);
        if line_body.is_empty() {
            saw_blank = true;
            continue;
        }
        if line_body[0] == b' ' || line_body[0] == b'\t' {
            // Continuation of the previous header.
            if let Some((_, value)) = headers.last_mut() {
                value.push(b'\n');
                value.extend_from_slice(line_body);
            }
            continue;
        }
        // New header: `key SP value` (or `key` with empty value).
        let sp = line_body.iter().position(|&b| b == b' ');
        match sp {
            Some(i) => {
                let key = String::from_utf8_lossy(&line_body[..i]).into_owned();
                headers.push((key, line_body[i + 1..].to_vec()));
            }
            None => headers.push((String::from_utf8_lossy(line_body).into_owned(), Vec::new())),
        }
    }
    (headers, message)
}

/// Look up a header value by key.
pub fn header_value<'a>(headers: &'a [(String, Vec<u8>)], key: &str) -> Option<&'a [u8]> {
    headers.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_slice())
}

fn parse_oid(bytes: &[u8], algo: HashAlgorithm) -> Result<Oid, HeaderError> {
    let s = std::str::from_utf8(bytes).map_err(|_| HeaderError("non-UTF-8 oid".into()))?;
    Oid::from_hex(s, algo).map_err(|_| HeaderError(format!("bad object id '{s}'")))
}

/// A parsed commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub tree: Oid,
    pub parents: Vec<Oid>,
    pub author: Option<String>,
    pub committer: Option<String>,
    pub message: Vec<u8>,
}

/// Parse a commit object.
pub fn parse_commit(data: &[u8], algo: HashAlgorithm) -> Result<Commit, HeaderError> {
    let (headers, message) = parse_headers(data);
    let tree_bytes = header_value(&headers, "tree").ok_or_else(|| HeaderError("missing tree".into()))?;
    let tree = parse_oid(tree_bytes, algo)?;

    let mut parents = Vec::new();
    for (key, value) in &headers {
        if key == "parent" {
            parents.push(parse_oid(value, algo)?);
        }
    }
    let author = header_value(&headers, "author").map(|b| String::from_utf8_lossy(b).into_owned());
    let committer = header_value(&headers, "committer").map(|b| String::from_utf8_lossy(b).into_owned());

    Ok(Commit {
        tree,
        parents,
        author,
        committer,
        message,
    })
}

/// A parsed tag object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub object: Oid,
    pub kind: ObjectKind,
    pub tag: Vec<u8>,
    pub tagger: Option<String>,
    pub message: Vec<u8>,
}

/// Parse an annotated tag object.
pub fn parse_tag(data: &[u8], algo: HashAlgorithm) -> Result<Tag, HeaderError> {
    let (headers, message) = parse_headers(data);
    let object = parse_oid(
        header_value(&headers, "object").ok_or_else(|| HeaderError("missing object".into()))?,
        algo,
    )?;
    let type_str = header_value(&headers, "type").ok_or_else(|| HeaderError("missing type".into()))?;
    let type_str = std::str::from_utf8(type_str).map_err(|_| HeaderError("bad type".into()))?;
    let kind = ObjectKind::from_str(type_str).ok_or_else(|| HeaderError(format!("unknown type '{type_str}'")))?;
    let tag = header_value(&headers, "tag").ok_or_else(|| HeaderError("missing tag".into()))?.to_vec();
    let tagger = header_value(&headers, "tagger").map(|b| String::from_utf8_lossy(b).into_owned());
    Ok(Tag {
        object,
        kind,
        tag,
        tagger,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commit() {
        let algo = HashAlgorithm::Sha1;
        let data = b"tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904\n\
                     parent 0000000000000000000000000000000000000001\n\
                     parent 0000000000000000000000000000000000000002\n\
                     author A <a@b> 1582024274 +0000\n\
                     committer C <c@d> 1582024274 +0000\n\
                     \n\
                     subject\n\nbody\n";
        let c = parse_commit(data, algo).unwrap();
        assert_eq!(c.tree, *algo.empty_tree());
        assert_eq!(c.parents.len(), 2);
        assert!(c.author.as_deref().unwrap().starts_with("A <a@b>"));
        assert_eq!(c.message, b"subject\n\nbody\n");
    }

    #[test]
    fn handles_gpgsig_continuation() {
        let algo = HashAlgorithm::Sha1;
        let data = b"tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904\n\
author A <a@b> 0 +0000\n\
committer C <c@d> 0 +0000\n\
gpgsig -----BEGIN PGP SIGNATURE-----\n\
\x20\x20\n\
\x20iQEcBAAB\n\
\x20-----END PGP SIGNATURE-----\n\
\n\
msg\n";
        let c = parse_commit(data, algo).unwrap();
        assert_eq!(c.message, b"msg\n");
    }

    #[test]
    fn parses_tag() {
        let algo = HashAlgorithm::Sha1;
        let data = b"object 4b825dc642cb6eb9a060e54bf8d69288fbee4904\n\
                     type tree\n\
                     tag v1.0\n\
                     tagger T <t@e> 0 +0000\n\
                     \n\
                     message\n";
        let t = parse_tag(data, algo).unwrap();
        assert_eq!(t.object, *algo.empty_tree());
        assert_eq!(t.kind, ObjectKind::Tree);
        assert_eq!(t.tag, b"v1.0");
        assert_eq!(t.message, b"message\n");
    }

    #[test]
    fn rejects_missing_tree() {
        let algo = HashAlgorithm::Sha1;
        let data = b"author A <a@b> 0 +0000\n\nmsg\n";
        assert!(parse_commit(data, algo).is_err());
    }
}
