//! `git mv`: move/rename tracked files in the index and worktree.
//!
//! Port of `builtin/mv.c` (`cmd_mv`): `<source>... <destination>` with `-f`,
//! `-k`, `-n`, `-v` (and `--sparse` accepted as a no-op). Submodules are
//! moved as plain paths (no `.gitmodules` rewriting).

use std::io::Write;
use std::path::PathBuf;

use crate::checkout_core::{self, read_index_or_empty, write_index};
use crate::{Command, CommandError, RepoContext};

pub struct Mv;

const USAGE: &str = "usage: git mv [-v] [-f] [-n] [-k] <source> <destination>\n   or: git mv [-v] [-f] [-n] [-k] <source>... <destination-directory>\n\n    -v, --[no-]verbose    be verbose\n    -n, --[no-]dry-run    dry run\n    -f, --[no-]force      force move/rename even if target exists\n    -k                    skip move/rename errors\n    --[no-]sparse         allow updating entries outside of the sparse-checkout cone\n";

fn usage_error(first: String) -> CommandError {
    CommandError::usage(format!("{first}\n{USAGE}"))
}

impl Command for Mv {
    fn name(&self) -> &'static str {
        "mv"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut verbose = false;
        let mut force = false;
        let mut dry_run = false;
        let mut skip_errors = false;
        let mut operands: Vec<String> = Vec::new();

        let mut i = 0usize;
        while i < args.len() {
            let a = &args[i];
            match a.as_str() {
                "-v" | "--verbose" => verbose = true,
                "--no-verbose" => verbose = false,
                "-f" | "--force" => force = true,
                "--no-force" => force = false,
                "-n" | "--dry-run" => dry_run = true,
                "--no-dry-run" => dry_run = false,
                "-k" => skip_errors = true,
                "--sparse" | "--no-sparse" => {}
                "--" => {
                    operands.extend(args[i + 1..].iter().cloned());
                    break;
                }
                s if s.starts_with("--") => {
                    let name = s[2..].split('=').next().unwrap_or("");
                    return Err(usage_error(format!("error: unknown option `{name}'")));
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    // Bundled shorts (-vf, -nf, ...).
                    for c in s[1..].chars() {
                        match c {
                            'v' => verbose = true,
                            'f' => force = true,
                            'n' => dry_run = true,
                            'k' => skip_errors = true,
                            _ => {
                                return Err(usage_error(format!(
                                    "error: unknown switch `{c}'"
                                )));
                            }
                        }
                    }
                }
                s => operands.push(s.to_string()),
            }
            i += 1;
        }

        if operands.len() < 2 {
            return Err(CommandError::usage(USAGE.to_string()));
        }

        let repo = ctx.repository()?;
        let work_tree = repo
            .work_tree
            .clone()
            .ok_or_else(|| CommandError::fatal("fatal: this operation must be run in a work tree"))?;

        // Resolve operands against the invoking cwd, like C's prefix handling.
        let mut resolved: Vec<String> = Vec::new();
        for o in &operands {
            resolved.push(checkout_core::resolve_inside(ctx, &repo, &work_tree, o, false)?);
        }
        let dst_raw = resolved.last().unwrap().clone();
        let srcs_raw = &resolved[..resolved.len() - 1];

        // Destination: existing dir (or trailing slash) means "into".
        let dst_exists_as_dir = std::fs::symlink_metadata(work_tree.join(&dst_raw))
            .is_ok_and(|m| m.file_type().is_dir() && !m.file_type().is_symlink());
        let dst_is_dirish = dst_raw.ends_with('/') || dst_exists_as_dir;
        if srcs_raw.len() > 1 && !dst_is_dirish {
            return Err(CommandError::fatal(format!(
                "fatal: destination '{dst_raw}' is not a directory"
            )));
        }

        let index = read_index_or_empty(&repo)?;
        let tracked: std::collections::HashSet<&str> =
            index.entries.iter().filter(|e| e.stage == 0).map(|e| e.name.as_str()).collect();

        // Plan every move first (validating), then execute.
        struct Move {
            src: String,
            dst: String,
        }
        let mut moves: Vec<Move> = Vec::new();
        for src in srcs_raw {
            // Destination for this source (PathBuf joining, so that a "."
            // or "" destination yields a relative path, never "/base").
            let join_into = |dir: &str, base: &str| -> String {
                std::path::Path::new(dir.trim_end_matches('/'))
                    .join(base)
                    .to_string_lossy()
                    .into_owned()
            };
            // The source must exist in the worktree first ("bad source"),
            // then be tracked ("not under version control").
            if std::fs::symlink_metadata(work_tree.join(src)).is_err() {
                // Destination for the message: dir-appended when dirish.
                let dst = if dst_is_dirish {
                    let base = src.rsplit('/').next().unwrap_or(src);
                    join_into(&dst_raw, base)
                } else {
                    dst_raw.clone()
                };
                let err = CommandError::fatal(format!(
                    "fatal: bad source, source={src}, destination={dst}"
                ));
                if skip_errors {
                    continue;
                }
                return Err(err);
            }
            if dst_raw.ends_with('/') && !dst_exists_as_dir {
                let err = CommandError::fatal(format!(
                    "fatal: destination directory does not exist, source={src}, destination={dst_raw}"
                ));
                if skip_errors {
                    continue;
                }
                return Err(err);
            }
            // Destination for this source.
            let dst = if dst_is_dirish {
                let base = src.rsplit('/').next().unwrap_or(src);
                join_into(&dst_raw, base)
            } else {
                dst_raw.clone()
            };
            // Same path?
            if src == &dst {
                let err = CommandError::fatal(format!(
                    "fatal: can not move directory into itself, source={src}, destination={dst}"
                ));
                if skip_errors {
                    continue;
                }
                return Err(err);
            }
            // Into-itself (dst strictly under src)?
            if dst.starts_with(&format!("{src}/")) {
                let err = CommandError::fatal(format!(
                    "fatal: can not move directory into itself, source={src}, destination={dst}"
                ));
                if skip_errors {
                    continue;
                }
                return Err(err);
            }
            // Source must be tracked (stage-0). Directories expand to the
            // tracked entries below them.
            let src_is_dir = index.entries.iter().any(|e| {
                e.stage == 0 && e.name.starts_with(&format!("{src}/"))
            });
            let src_tracked = tracked.contains(src.as_str()) || src_is_dir;
            if !src_tracked {
                let err = CommandError::fatal(format!(
                    "fatal: not under version control, source={src}, destination={dst}"
                ));
                if skip_errors {
                    continue;
                }
                return Err(err);
            }
            // Unmerged source?
            if index.entries.iter().any(|e| e.stage != 0 && (e.name == *src || e.name.starts_with(&format!("{src}/")))) {
                let err = CommandError::fatal(format!(
                    "fatal: bad source, source={src}, destination={dst}"
                ));
                if skip_errors {
                    continue;
                }
                return Err(err);
            }
            // Destination collision?
            let dst_full = work_tree.join(&dst);
            let dst_meta = std::fs::symlink_metadata(&dst_full).ok();
            let dst_is_real_dir = dst_meta
                .as_ref()
                .is_some_and(|m| m.file_type().is_dir() && !m.file_type().is_symlink());
            if dst_meta.is_some() && !force {
                // Directory sources merge into existing dirs (rename(2)
                // would fail, but C moves the tree inside... only when the
                // destination does not exist as an entry? Probed: dir onto
                // existing dir moves INSIDE it. A file colliding fails.)
                let src_is_dir_move = src_is_dir && !tracked.contains(src.as_str());
                if src_is_dir_move && dst_is_real_dir {
                    // Move inside: dst = dst/src-basename.
                    let base = src.rsplit('/').next().unwrap_or(src);
                    let inner = join_into(&dst, base);
                    moves.push(Move { src: src.clone(), dst: inner });
                    continue;
                }
                let word = if dst_is_real_dir { "already exists" } else { "exists" };
                let err = CommandError::fatal(format!(
                    "fatal: destination {word}, source={src}, destination={dst}"
                ));
                if skip_errors {
                    continue;
                }
                return Err(err);
            }
            moves.push(Move { src: src.clone(), dst });
        }

        if dry_run {
            for m in &moves {
                writeln!(out, "Checking rename of '{}' to '{}'", m.src, m.dst)
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
                writeln!(out, "Renaming {} to {}", m.src, m.dst)
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
            }
            return Ok(());
        }

        // Execute: worktree renames + index updates.
        let mut new_index = read_index_or_empty(&repo)?;
        for m in &moves {
            let src_full = work_tree.join(&m.src);
            let dst_full = work_tree.join(&m.dst);
            if force {
                // Clear the destination (file or dir) like C does.
                match std::fs::symlink_metadata(&dst_full) {
                    Ok(md) if md.file_type().is_dir() && !md.file_type().is_symlink() => {
                        let _ = std::fs::remove_dir_all(&dst_full);
                    }
                    Ok(_) => {
                        let _ = std::fs::remove_file(&dst_full);
                    }
                    Err(_) => {}
                }
                // Also drop any index entries at the destination.
                new_index.entries.retain(|e| {
                    !(e.name == m.dst || e.name.starts_with(&format!("{}/", m.dst)))
                });
            }
            if let Err(e) = std::fs::rename(&src_full, &dst_full) {
                let err = CommandError::fatal(format!(
                    "fatal: renaming '{}' failed: {}",
                    m.src,
                    io_strerror(&e)
                ));
                if skip_errors {
                    continue;
                }
                // Persist moves done so far, like C (no rollback).
                write_index(&repo, &new_index)?;
                return Err(err);
            }
            // Move index entries (the whole subtree for directories).
            let mut moved: Vec<(String, git_index::IndexEntry)> = Vec::new();
            new_index.entries.retain(|e| {
                if e.stage == 0 && (e.name == m.src || e.name.starts_with(&format!("{}/", m.src))) {
                    let rest = e.name[m.src.len()..].to_string();
                    let mut ne = e.clone();
                    ne.name = format!("{}{}", m.dst, rest);
                    moved.push((e.name.clone(), ne));
                    false
                } else {
                    true
                }
            });
            for (_, mut ne) in moved {
                // Re-stat at the new location.
                if let Ok(md) = std::fs::symlink_metadata(work_tree.join(&ne.name)) {
                    checkout_core::fill_stat(&mut ne, &md);
                }
                new_index.entries.push(ne);
            }
            if verbose {
                writeln!(out, "Renaming {} to {}", m.src, m.dst)
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }
        new_index.entries.sort_by(|a, b| {
            a.name.cmp(&b.name).then_with(|| a.stage.cmp(&b.stage))
        });
        if let Some(ct) = new_index.cache_tree.as_mut() {
            for m in &moves {
                ct.invalidate_path(&m.src);
                ct.invalidate_path(&m.dst);
            }
        }
        write_index(&repo, &new_index)?;
        Ok(())
    }
}

fn io_strerror(e: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::NotFound => "No such file or directory".to_string(),
        ErrorKind::PermissionDenied => "Permission denied".to_string(),
        ErrorKind::AlreadyExists => "File exists".to_string(),
        _ => {
            let s = e.to_string();
            match s.rfind(" (os error ") {
                Some(i) => s[..i].to_string(),
                None => s,
            }
        }
    }
}
