//! The git object database.
//!
//! This module implements the loose-object store: objects stored at
//! `objects/xx/yyyy...` with a `<type> <size>\0` header, zlib-compressed,
//! and addressed by the hash of their serialized form.
//!
//! A git-compatible subset of `object-file.c` / `odb/source-loose.c`.

use std::error::Error;
use std::fmt;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;

use git_core::Repository;
use git_hash::{HashAlgorithm, Oid};
use git_object::{Object, ObjectError, ObjectKind};

pub mod pack;

pub use pack::{Odb, PackError};

static TEMP_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Errors returned by the object store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OdbError {
    Io(String),
    Corrupt(String),
    NotFound,
}

impl fmt::Display for OdbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OdbError::Io(e) => write!(f, "object store I/O error: {e}"),
            OdbError::Corrupt(e) => write!(f, "corrupt object: {e}"),
            OdbError::NotFound => write!(f, "object not found"),
        }
    }
}

impl Error for OdbError {}

impl From<ObjectError> for OdbError {
    fn from(e: ObjectError) -> OdbError {
        OdbError::Corrupt(e.to_string())
    }
}

/// A loose-object store rooted at an `objects` directory.
#[derive(Debug, Clone)]
pub struct LooseStore {
    objects_dir: PathBuf,
    algo: HashAlgorithm,
}

impl LooseStore {
    /// Create a store rooted at `objects_dir` hashing with `algo`.
    pub fn new(objects_dir: PathBuf, algo: HashAlgorithm) -> LooseStore {
        LooseStore { objects_dir, algo }
    }

    /// Create a store for the given repository (its common dir + hash algo).
    pub fn from_repo(repo: &Repository) -> LooseStore {
        LooseStore::new(repo.common_dir.join("objects"), repo.hash_algo)
    }

    /// The hash algorithm this store writes with.
    pub fn algorithm(&self) -> HashAlgorithm {
        self.algo
    }

    /// The on-disk path for a loose object.
    pub fn oid_path(&self, oid: &Oid) -> PathBuf {
        let hex = format!("{oid}");
        self.objects_dir.join(&hex[..2]).join(&hex[2..])
    }

    /// The `objects` directory this store is rooted at.
    pub fn objects_dir(&self) -> &std::path::Path {
        &self.objects_dir
    }

    /// The number of loose objects present on disk.
    pub fn object_count(&self) -> usize {
        let mut count = 0usize;
        if let Ok(rd) = std::fs::read_dir(&self.objects_dir) {
            for e in rd.flatten() {
                let name = e.file_name();
                let name = name.to_string_lossy();
                let is_fanout = name.len() == 2 && name.bytes().all(|b| b.is_ascii_hexdigit());
                if e.path().is_dir() && is_fanout {
                    if let Ok(sub) = std::fs::read_dir(e.path()) {
                        count += sub.flatten().filter(|x| x.path().is_file()).count();
                    }
                }
            }
        }
        count
    }

    /// Whether an object exists as a loose object.
    pub fn contains(&self, oid: &Oid) -> bool {
        self.oid_path(oid).is_file()
    }

    /// Read a full object.
    pub fn read(&self, oid: &Oid) -> Result<Object, OdbError> {
        let bytes = self.read_raw(oid)?;
        let (object, _header_len) = Object::parse(&bytes)?;
        Ok(object)
    }

    /// Read only the type and size of an object.
    pub fn read_header(&self, oid: &Oid) -> Result<(ObjectKind, u64), OdbError> {
        let bytes = self.read_raw(oid)?;
        let (object, _) = Object::parse(&bytes)?;
        Ok((object.kind, object.data.len() as u64))
    }

    /// Write an object, returning its id. Writing an existing object is
    /// idempotent (returns the same id without error).
    pub fn write(&self, object: &Object) -> Result<Oid, OdbError> {
        let oid = object.compute_id(self.algo);
        if self.contains(&oid) {
            return Ok(oid);
        }
        let path = self.oid_path(&oid);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| OdbError::Io(e.to_string()))?;
        }

        let serialized = object.to_bytes();
        let compressed = compress(&serialized);

        let tmp = path.with_file_name(format!(
            "tmp_obj_{}_{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        {
            let mut f = std::fs::File::create(&tmp).map_err(|e| OdbError::Io(e.to_string()))?;
            f.write_all(&compressed).map_err(|e| OdbError::Io(e.to_string()))?;
            f.sync_all().map_err(|e| OdbError::Io(e.to_string()))?;
        }
        std::fs::rename(&tmp, &path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            OdbError::Io(e.to_string())
        })?;
        Ok(oid)
    }

    /// Write raw content as a given kind, returning its id.
    pub fn write_object(&self, kind: ObjectKind, data: &[u8]) -> Result<Oid, OdbError> {
        self.write(&Object::from_data(kind, data.to_vec()))
    }

    /// Inflate and return the raw serialized bytes of a loose object.
    fn read_raw(&self, oid: &Oid) -> Result<Vec<u8>, OdbError> {
        let path = self.oid_path(oid);
        let file = std::fs::File::open(&path).map_err(|_| OdbError::NotFound)?;
        let mut decoder = ZlibDecoder::new(file);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|e| OdbError::Io(e.to_string()))?;
        Ok(out)
    }
}

/// Deflate `data` with zlib framing (matching git's loose-object compression).
fn compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("compression failed");
    encoder.finish().expect("compression failed")
}

/// Inflate zlib-framed data (helper for tests).
#[allow(dead_code)]
fn decompress(data: &[u8]) -> Vec<u8> {
    let mut decoder = ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).expect("decompression failed");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn tempdir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("git-odb-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn store(algo: HashAlgorithm) -> (LooseStore, PathBuf) {
        let dir = tempdir();
        let store = LooseStore::new(dir.join("objects"), algo);
        (store, dir)
    }

    fn sample_objects() -> Vec<Object> {
        vec![
            Object::from_data(ObjectKind::Blob, b"hello world".to_vec()),
            Object::from_data(ObjectKind::Tree, b"100644 one\0".as_slice().to_vec()),
            Object::from_data(ObjectKind::Commit, b"tree 0000\nauthor A <a@b> 0 +0000\n\nmsg\n".to_vec()),
            Object::from_data(ObjectKind::Tag, b"object 0000\ntype commit\ntag v1\n".to_vec()),
        ]
    }

    #[test]
    fn round_trips_all_kinds() {
        for algo in [HashAlgorithm::Sha1, HashAlgorithm::Sha256] {
            let (store, dir) = store(algo);
            for obj in sample_objects() {
                let oid = store.write(&obj).unwrap();
                assert_eq!(oid, obj.compute_id(algo));
                assert!(store.contains(&oid));
                let read = store.read(&oid).unwrap();
                assert_eq!(read, obj, "{algo:?} {obj:?}");
                let (kind, size) = store.read_header(&oid).unwrap();
                assert_eq!(kind, obj.kind);
                assert_eq!(size, obj.data.len() as u64);
            }
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn empty_blob_writes_to_expected_path() {
        let (store, dir) = store(HashAlgorithm::Sha1);
        let oid = store.write_object(ObjectKind::Blob, &[]).unwrap();
        assert_eq!(oid, *HashAlgorithm::Sha1.empty_blob());
        assert!(store.oid_path(&oid).starts_with(dir.join("objects/e6")));
        assert!(store.contains(&oid));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_is_idempotent() {
        let (store, dir) = store(HashAlgorithm::Sha1);
        let obj = Object::from_data(ObjectKind::Blob, b"content".to_vec());
        let a = store.write(&obj).unwrap();
        let b = store.write(&obj).unwrap();
        assert_eq!(a, b);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_missing_object_errors() {
        let (store, dir) = store(HashAlgorithm::Sha1);
        let oid = *HashAlgorithm::Sha1.null_oid();
        assert_eq!(store.read(&oid), Err(OdbError::NotFound));
        assert!(!store.contains(&oid));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_object_is_detected() {
        let (store, dir) = store(HashAlgorithm::Sha1);
        // A valid zlib stream whose payload is a bogus header.
        let bogus = compress(b"wat 3\0abc");
        let oid = *HashAlgorithm::Sha1.empty_blob();
        let path = store.oid_path(&oid);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &bogus).unwrap();

        assert!(matches!(store.read(&oid), Err(OdbError::Corrupt(_))));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn size_mismatch_is_detected() {
        let (store, dir) = store(HashAlgorithm::Sha1);
        // Header declares 5 bytes but only 3 follow.
        let bogus = compress(b"blob 5\0abc");
        let oid = *HashAlgorithm::Sha1.empty_blob();
        let path = store.oid_path(&oid);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &bogus).unwrap();

        assert!(matches!(
            store.read(&oid),
            Err(OdbError::Corrupt(ref m)) if m.contains("size mismatch")
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn large_blob_streams() {
        let (store, dir) = store(HashAlgorithm::Sha1);
        let data: Vec<u8> = (0..1_000_000u32).map(|i| (i % 251) as u8).collect();
        let oid = store.write_object(ObjectKind::Blob, &data).unwrap();
        let read = store.read(&oid).unwrap();
        assert_eq!(read.data, data);
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod props {
    use crate::pack::{write_pack, PackFile, PackObject};
    use git_hash::HashAlgorithm;
    use git_object::{Object, ObjectKind};
    use proptest::prelude::*;

    proptest! {
        /// Writing a pack of random blobs and reading it back must yield the
        /// original objects, with all checksums intact.
        #[test]
        fn pack_round_trips(blobs: Vec<Vec<u8>>) {
            let algo = HashAlgorithm::Sha1;
            let objs: Vec<Object> = blobs
                .into_iter()
                .map(|d| Object::from_data(ObjectKind::Blob, d))
                .collect();
            let pos: Vec<PackObject> = objs
                .iter()
                .map(|o| PackObject {
                    oid: o.compute_id(algo),
                    kind: o.kind,
                    data: o.data.clone(),
                })
                .collect();
            let (pack, idx_bytes) = write_pack(&pos, algo).unwrap();
            let idx = crate::pack::PackIndex::parse(&idx_bytes, algo).unwrap();
            let pf = PackFile::from_bytes(pack, algo).unwrap();
            prop_assert!(pf.verify(&idx).is_ok());
            for obj in &objs {
                let oid = obj.compute_id(algo);
                let off = idx.find(&oid).unwrap() as usize;
                let mut resolver = |_: &git_hash::Oid| -> Option<Object> { None };
                let resolved = pf.resolve_entry(off, Some(&idx), &mut resolver).unwrap();
                prop_assert_eq!(&resolved.object, obj);
            }
        }
    }
}

