//! `git commit-tree`: create a commit object from a tree, parents, and
//! message, and write it to the object store. A port of
//! `builtin/commit-tree.c`.

use std::io::{Read, Write};

use crate::{Command, CommandError, RepoContext};
use crate::ident;
use git_hash::Oid;
use git_object::{Object, ObjectKind};
use git_odb::LooseStore;

pub struct CommitTree;

impl Command for CommitTree {
    fn name(&self) -> &'static str {
        "commit-tree"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let repo = ctx.repository()?;
        let store = LooseStore::from_repo(&repo);

        let mut tree: Option<String> = None;
        let mut parents: Vec<String> = Vec::new();
        let mut messages: Vec<String> = Vec::new();

        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "-p" => {
                    let p = it
                        .next()
                        .ok_or_else(|| CommandError::usage("commit-tree: option '-p' requires an argument"))?;
                    parents.push(p.clone());
                }
                "-m" => {
                    let m = it
                        .next()
                        .ok_or_else(|| CommandError::usage("commit-tree: option '-m' requires an argument"))?;
                    messages.push(m.clone());
                }
                "--" => {
                    if tree.is_none() {
                        tree = it.next().cloned();
                    }
                    break;
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("commit-tree: unknown option '{s}'")));
                }
                _ if tree.is_none() => tree = Some(a.clone()),
                _ => {
                    return Err(CommandError::usage("commit-tree: too many arguments"));
                }
            }
        }

        let tree = tree.ok_or_else(|| CommandError::usage("commit-tree: missing <tree> argument"))?;
        let tree_oid = Oid::from_hex(&tree, repo.hash_algo)
            .map_err(|_| CommandError::error(format!("not a valid object name: '{tree}'")))?;
        let mut parent_oids = Vec::with_capacity(parents.len());
        for p in &parents {
            let oid = Oid::from_hex(p, repo.hash_algo)
                .map_err(|_| CommandError::error(format!("not a valid object name: '{p}'")))?;
            parent_oids.push(oid);
        }

        let message = if messages.is_empty() {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            buf
        } else {
            messages.join("\n")
        };

        let author = ident::user_ident(&repo, true)?;
        let committer = ident::user_ident(&repo, false)?;

        let mut content = format!("tree {tree_oid}\n");
        for p in &parent_oids {
            content.push_str(&format!("parent {p}\n"));
        }
        content.push_str(&format!("author {author}\ncommitter {committer}\n\n{message}\n"));

        let obj = Object::from_data(ObjectKind::Commit, content.into_bytes());
        let oid = store.write(&obj).map_err(CommandError::from)?;
        writeln!(out, "{oid}").map_err(|e| CommandError::fatal(e.to_string()))?;
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
        let dir = std::env::temp_dir().join(format!("git-commit-tree-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn init_repo(base: &std::path::Path) {
        let git = base.join(".git");
        std::fs::create_dir_all(git.join("objects")).unwrap();
        std::fs::create_dir_all(git.join("refs")).unwrap();
        std::fs::write(
            git.join("config"),
            "[user]\n\tname = Tester\n\temail = test@example.com\n",
        )
        .unwrap();
    }

    #[test]
    fn creates_and_writes_a_commit() {
        let dir = tempdir();
        init_repo(&dir);
        std::env::set_var("GIT_AUTHOR_DATE", "2020-02-18 11:11:14 +0000");
        std::env::set_var("GIT_COMMITTER_DATE", "2020-02-18 11:11:14 +0000");

        let tree_hex = format!("{}", git_hash::HashAlgorithm::Sha1.empty_tree());

        let mut out = Vec::new();
        let res = crate::tests::with_cwd(&dir, || {
            let ctx = RepoContext::new();
            CommitTree.run(
                &ctx,
                &[
                    "-m".to_string(),
                    "initial commit".to_string(),
                    tree_hex.clone(),
                ],
                &mut out,
            )
        });
        res.unwrap();

        let oid_str = String::from_utf8(out).unwrap().trim().to_string();
        let oid = Oid::from_hex(&oid_str, git_hash::HashAlgorithm::Sha1).unwrap();

        let store = LooseStore::new(dir.join(".git/objects"), git_hash::HashAlgorithm::Sha1);
        let commit = store.read(&oid).unwrap();
        assert_eq!(commit.kind, ObjectKind::Commit);

        let text = String::from_utf8(commit.data.clone()).unwrap();
        assert!(text.starts_with(&format!("tree {tree_hex}\n")), "got: {text}");
        assert!(text.contains("author Tester <test@example.com> 1582024274 +0000"));
        assert!(text.contains("committer Tester <test@example.com> 1582024274 +0000"));
        assert!(text.ends_with("\n\ninitial commit\n"));

        std::env::remove_var("GIT_AUTHOR_DATE");
        std::env::remove_var("GIT_COMMITTER_DATE");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn includes_parents() {
        let dir = tempdir();
        init_repo(&dir);

        // A pseudo parent oid (all zeros) is structurally fine for this test.
        let parent = format!("{}", git_hash::HashAlgorithm::Sha1.null_oid());
        let tree_hex = format!("{}", git_hash::HashAlgorithm::Sha1.empty_tree());

        let mut out = Vec::new();
        crate::tests::with_cwd(&dir, || {
            let ctx = RepoContext::new();
            CommitTree.run(
                &ctx,
                &[
                    "-p".to_string(),
                    parent.clone(),
                    "-m".to_string(),
                    "second".to_string(),
                    tree_hex,
                ],
                &mut out,
            )
        })
        .unwrap();

        let oid_str = String::from_utf8(out).unwrap().trim().to_string();
        let oid = Oid::from_hex(&oid_str, git_hash::HashAlgorithm::Sha1).unwrap();
        let store = LooseStore::new(dir.join(".git/objects"), git_hash::HashAlgorithm::Sha1);
        let commit = store.read(&oid).unwrap();
        let text = String::from_utf8(commit.data).unwrap();
        assert!(text.contains(&format!("parent {parent}\n")), "got: {text}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_tree_is_usage_error() {
        let mut out = Vec::new();
        let ctx = RepoContext::new();
        let err = CommitTree.run(&ctx, &["-m".to_string(), "msg".to_string()], &mut out).unwrap_err();
        assert_eq!(err.code, 129);
    }
}
