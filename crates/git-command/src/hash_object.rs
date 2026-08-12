//! `git hash-object`: compute (and optionally store) the object id of
//! content. A port of `builtin/hash-object.c`.

use std::io::{Read, Write};

use crate::{Command, CommandError};
use git_core::Repository;
use git_hash::HashAlgorithm;
use git_object::{Object, ObjectKind};
use git_odb::LooseStore;

pub struct HashObject;

impl Command for HashObject {
    fn name(&self) -> &'static str {
        "hash-object"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut write = false;
        let mut kind = ObjectKind::Blob;
        let mut stdin = false;
        let mut paths: Vec<String> = Vec::new();

        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "-w" => write = true,
                "--stdin" => stdin = true,
                "-t" => {
                    let t = it
                        .next()
                        .ok_or_else(|| CommandError::usage("hash-object: option '-t' requires an argument"))?;
                    kind = ObjectKind::from_str(t)
                        .ok_or_else(|| CommandError::error(format!("invalid object type '{t}'")))?;
                }
                "--" => {
                    paths.extend(it.cloned());
                    break;
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("hash-object: unknown option '{s}'")));
                }
                p => paths.push(p.to_string()),
            }
        }

        // The store is only needed to actually write objects; without `-w` the
        // hash is computed with SHA-1 (git's default outside a repository).
        let store = if write {
            let repo = Repository::discover()?;
            Some(LooseStore::from_repo(&repo))
        } else {
            None
        };
        let algo = store.as_ref().map(LooseStore::algorithm).unwrap_or(HashAlgorithm::Sha1);

        let mut inputs: Vec<(ObjectKind, Vec<u8>)> = Vec::new();
        if stdin {
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            inputs.push((kind, buf));
        }
        for p in &paths {
            let data =
                std::fs::read(p).map_err(|e| CommandError::fatal(format!("could not open '{p}': {e}")))?;
            inputs.push((kind, data));
        }
        if inputs.is_empty() {
            return Err(CommandError::usage(
                "hash-object: no input (use --stdin or provide file paths)",
            ));
        }

        for (k, data) in inputs {
            let obj = Object::from_data(k, data);
            let oid = match &store {
                Some(s) => s.write(&obj).map_err(CommandError::from)?,
                None => obj.compute_id(algo),
            };
            writeln!(out, "{oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn tempdir() -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("git-hash-object-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn init_repo(base: &std::path::Path) {
        let git = base.join(".git");
        std::fs::create_dir_all(git.join("objects")).unwrap();
        std::fs::create_dir_all(git.join("refs")).unwrap();
    }

    #[test]
    fn hashes_known_blob() {
        // The empty blob must hash to git's known empty-blob oid.
        let dir = tempdir();
        let path = dir.join("empty");
        std::fs::write(&path, b"").unwrap();

        let mut out = Vec::new();
        HashObject
            .run(&[path.to_string_lossy().to_string()], &mut out)
            .unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap().trim(),
            format!("{}", git_hash::HashAlgorithm::Sha1.empty_blob())
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writes_object_end_to_end() {
        let dir = tempdir();
        init_repo(&dir);
        let payload = b"hello from hash-object\n";
        let path = dir.join("file.txt");
        std::fs::write(&path, payload).unwrap();

        // Switch cwd into the repo so Repository::discover() finds it.
        crate::tests::with_cwd(&dir, || {
            let mut out = Vec::new();
            HashObject
                .run(&["-w".to_string(), path.to_string_lossy().to_string()], &mut out)
                .unwrap();

            let oid_str = String::from_utf8(out).unwrap().trim().to_string();
            let oid = git_hash::Oid::from_hex(&oid_str, git_hash::HashAlgorithm::Sha1).unwrap();

            // The loose object exists on disk and reads back correctly.
            let store = LooseStore::new(dir.join(".git/objects"), git_hash::HashAlgorithm::Sha1);
            assert!(store.contains(&oid));
            let obj = store.read(&oid).unwrap();
            assert_eq!(obj.kind, ObjectKind::Blob);
            assert_eq!(obj.data, payload);
        });
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_option_is_usage_error() {
        let mut out = Vec::new();
        let res = HashObject.run(&["--bogus".to_string()], &mut out);
        assert_eq!(res.unwrap_err().code, 129);
    }
}
