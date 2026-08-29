//! `git diff`: unified diffs between trees or files.

use std::io::Write;

use crate::patch;
use crate::{Command, CommandError, RepoContext};
use git_hash::{HashAlgorithm, Oid};
use git_object::{parse_tree, Object, ObjectKind};
use git_odb::Odb;

pub struct Diff;

impl Command for Diff {
    fn name(&self) -> &'static str {
        "diff"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut no_index = false;
        let mut exit_code = false;
        let mut numstat = false;
        let mut rest: Vec<String> = Vec::new();
        for a in args {
            match a.as_str() {
                "--no-index" => no_index = true,
                "--exit-code" => exit_code = true,
                "--numstat" => numstat = true,
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("diff: option '{s}' not supported")));
                }
                s => rest.push(s.to_string()),
            }
        }

        if no_index {
            if rest.len() != 2 {
                return Err(CommandError::usage("diff --no-index: requires two paths"));
            }
            return no_index_diff(&rest[0], &rest[1], out);
        }

        if rest.len() != 2 {
            return Err(CommandError::usage("diff: requires two <tree-ish> arguments"));
        }
        let repo = ctx.repository()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let algo = repo.hash_algo;

        let t1 = Oid::from_hex(&rest[0], algo)
            .map_err(|_| CommandError::error(format!("Not a valid object name '{}'", rest[0])))?;
        let t2 = Oid::from_hex(&rest[1], algo)
            .map_err(|_| CommandError::error(format!("Not a valid object name '{}'", rest[1])))?;
        let obj1 = odb.read(&t1).map_err(|e| CommandError::error(e.to_string()))?;
        let obj2 = odb.read(&t2).map_err(|e| CommandError::error(e.to_string()))?;
        let e1 = parse_tree(&obj1.data, algo).map_err(|e| CommandError::error(e.to_string()))?;
        let e2 = parse_tree(&obj2.data, algo).map_err(|e| CommandError::error(e.to_string()))?;

        let mut loader = |oid: &Oid| odb.read(oid).ok();
        let changes = git_diff::compare_trees(&e1, &e2, "", true, &mut loader);
        if changes.is_empty() {
            return Ok(());
        }
        for c in &changes {
            if numstat {
                patch::render_numstat(c, &odb, out)?;
            } else {
                let p = patch::render_change_patch(c, &odb)?;
                out.write_all(&p).map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }
        // `git diff` between two trees exits 0 unless --exit-code is given.
        if exit_code {
            Err(CommandError::silent(1))
        } else {
            Ok(())
        }
    }
}

fn no_index_diff(a_path_raw: &str, b_path_raw: &str, out: &mut dyn Write) -> Result<(), CommandError> {
    let a = std::fs::read(a_path_raw).map_err(|e| CommandError::error(format!("{a_path_raw}: {e}")))?;
    let b = std::fs::read(b_path_raw).map_err(|e| CommandError::error(format!("{b_path_raw}: {e}")))?;
    // git renders absolute paths without their leading slash in the header.
    let a_path = a_path_raw.trim_start_matches('/');
    let b_path = b_path_raw.trim_start_matches('/');
    let algo = HashAlgorithm::Sha1;
    let old_oid = Object::from_data(ObjectKind::Blob, a.clone()).compute_id(algo);
    let new_oid = Object::from_data(ObjectKind::Blob, b.clone()).compute_id(algo);

    writeln!(out, "diff --git a/{a_path} b/{b_path}").map_err(|e| CommandError::fatal(e.to_string()))?;
    writeln!(
        out,
        "index {}..{} 100644",
        &old_oid.to_string()[..7],
        &new_oid.to_string()[..7]
    )
    .map_err(|e| CommandError::fatal(e.to_string()))?;
    writeln!(out, "--- a/{a_path}").map_err(|e| CommandError::fatal(e.to_string()))?;
    writeln!(out, "+++ b/{b_path}").map_err(|e| CommandError::fatal(e.to_string()))?;
    out.write_all(&git_diff::diff_blobs(&a, &b))
        .map_err(|e| CommandError::fatal(e.to_string()))?;
    if a == b {
        Ok(())
    } else {
        Err(CommandError::silent(1))
    }
}