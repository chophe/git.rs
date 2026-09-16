//! `git rm`: remove files from the index and/or worktree.
//!
//! Port of `builtin/rm.c` (`cmd_rm`): `--cached`, `-r`, `-f`, `-n`, `-q`,
//! `--ignore-unmatch`, and the staged/local-modification safety checks
//! (including the "different from both the file and the HEAD" case).
//! Deferred: `--pathspec-from-file` (explicit error), submodules beyond
//! plain removal, `--sparse` (accepted as a no-op).

use std::io::Write;
use std::path::PathBuf;

use crate::checkout_core::{self, read_head, read_index_or_empty, write_index};
use crate::{Command, CommandError, RepoContext};
use git_object::ObjectKind;
use git_odb::Odb;

pub struct Rm;

const USAGE: &str = "usage: git rm [-f | --force] [-n] [-r] [--cached] [--ignore-unmatch]\n              [--quiet] [--pathspec-from-file=<file> [--pathspec-file-nul]]\n              [--] [<pathspec>...]\n\n    -n, --[no-]dry-run    dry run\n    -q, --[no-]quiet      do not list removed files\n    --[no-]cached         only remove from the index\n    -f, --[no-]force      override the up-to-date check\n    -r                    allow recursive removal\n    --[no-]ignore-unmatch exit with a zero status even if nothing matched\n    --[no-]sparse         allow updating entries outside of the sparse-checkout cone\n    --[no-]pathspec-from-file <file>\n                          read pathspec from file\n    --[no-]pathspec-file-nul\n                          with --pathspec-from-file, pathspec elements are separated with NUL character\n";

fn usage_error(first: String) -> CommandError {
    CommandError::usage(format!("{first}\n{USAGE}"))
}

impl Command for Rm {
    fn name(&self) -> &'static str {
        "rm"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut cached = false;
        let mut recursive = false;
        let mut force = false;
        let mut dry_run = false;
        let mut quiet = false;
        let mut ignore_unmatch = false;
        let mut operands: Vec<String> = Vec::new();

        let mut i = 0usize;
        while i < args.len() {
            let a = &args[i];
            match a.as_str() {
                "--cached" => cached = true,
                "--no-cached" => cached = false,
                "-r" => recursive = true,
                "-f" | "--force" => force = true,
                "--no-force" => force = false,
                "-n" | "--dry-run" => dry_run = true,
                "--no-dry-run" => dry_run = false,
                "-q" | "--quiet" => quiet = true,
                "--no-quiet" => quiet = false,
                "--ignore-unmatch" => ignore_unmatch = true,
                "--no-ignore-unmatch" => ignore_unmatch = false,
                "--sparse" | "--no-sparse" => {}
                "--pathspec-from-file" | "--pathspec-file-nul" => {
                    return Err(CommandError::fatal(format!(
                        "fatal: rm: option '{a}' is not supported yet"
                    )));
                }
                s if s.starts_with("--pathspec-from-file=") => {
                    let _ = s;
                    return Err(CommandError::fatal(
                        "fatal: rm: option '--pathspec-from-file' is not supported yet",
                    ));
                }
                "--" => {
                    operands.extend(args[i + 1..].iter().cloned());
                    break;
                }
                s if s.starts_with("--") => {
                    let name = s[2..].split('=').next().unwrap_or("");
                    return Err(usage_error(format!("error: unknown option `{name}'")));
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    // Bundled shorts (-rf, -nq, ...).
                    let mut ok = true;
                    for c in s[1..].chars() {
                        match c {
                            'r' => recursive = true,
                            'f' => force = true,
                            'n' => dry_run = true,
                            'q' => quiet = true,
                            _ => {
                                ok = false;
                                return Err(usage_error(format!(
                                    "error: unknown switch `{c}'"
                                )));
                            }
                        }
                    }
                    let _ = ok;
                }
                s => operands.push(s.to_string()),
            }
            i += 1;
        }

        if operands.is_empty() {
            return Err(CommandError::fatal(
                "fatal: No pathspec was given. Which files should I remove?",
            ));
        }

        let repo = ctx.repository()?;
        let algo = repo.hash_algo;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let work_tree = repo
            .work_tree
            .clone()
            .ok_or_else(|| CommandError::fatal("fatal: this operation must be run in a work tree"))?;
        let filemode = repo.config.get_bool("core", "filemode").unwrap_or(true);

        let mut specs: Vec<String> = Vec::new();
        for o in &operands {
            specs.push(checkout_core::resolve_inside(ctx, &repo, &work_tree, o, true)?);
        }
        let matched = |path: &str| specs.iter().any(|s| checkout_core::spec_matches_glob(s, path));

        let index = read_index_or_empty(&repo)?;
        // The set of index entries the pathspecs hit (stage-0 and unmerged).
        let mut hit_any = vec![false; specs.len()];
        for e in &index.entries {
            for (si, s) in specs.iter().enumerate() {
                if checkout_core::spec_matches_glob(s, &e.name) {
                    hit_any[si] = true;
                }
            }
        }
        // A directory pathspec also "matches" when it names a worktree dir?
        // No: rm only operates on tracked (index) paths.
        if !ignore_unmatch {
            for (si, s) in specs.iter().enumerate() {
                if !hit_any[si] {
                    return Err(CommandError::fatal(format!(
                        "fatal: pathspec '{}' did not match any files",
                        operands[si]
                    )));
                }
            }
        }

        // Directory pathspecs require -r.
        for (si, s) in specs.iter().enumerate() {
            if !hit_any[si] {
                continue;
            }
            // Does the spec name a directory (i.e. it matches paths below
            // itself rather than only itself)? Glob specs skip this check
            // (C resolves them through the pathspec machinery instead).
            if s.contains(['*', '?', '[']) {
                continue;
            }
            let is_dir_spec = index
                .entries
                .iter()
                .any(|e| e.name.starts_with(&format!("{s}/")));
            if is_dir_spec && !recursive {
                return Err(CommandError::fatal(format!(
                    "fatal: not removing '{}' recursively without -r",
                    operands[si]
                )));
            }
        }

        // HEAD tree for the staged check (None when unborn).
        let head = read_head(&repo);
        let head_map: std::collections::BTreeMap<String, (u32, git_hash::Oid)> =
            match head.oid {
                Some(oid) => match odb.read(&oid) {
                    Ok(o) if o.kind == ObjectKind::Commit => {
                        match git_object::parse_commit(&o.data, algo) {
                            Ok(c) => head_tree_view(&odb, algo, &c.tree),
                            Err(_) => Default::default(),
                        }
                    }
                    _ => Default::default(),
                },
                None => Default::default(),
            };
        let no_head = head.oid.is_none();

        // Safety checks (C collects all three lists, then aborts on any).
        let mut files_staged: Vec<String> = Vec::new();
        let mut files_cached: Vec<String> = Vec::new();
        let mut files_local: Vec<String> = Vec::new();
        if !force {
            // Sorted for deterministic output.
            let mut names: Vec<&str> = index
                .entries
                .iter()
                .filter(|e| e.stage == 0 && matched(&e.name))
                .map(|e| e.name.as_str())
                .collect();
            names.sort();
            names.dedup();
            for name in names {
                // Missing worktree files are skipped (nothing to lose).
                let wt_missing =
                    std::fs::symlink_metadata(work_tree.join(name)).is_err();
                if wt_missing {
                    continue;
                }
                let entry = index
                    .entries
                    .iter()
                    .find(|e| e.stage == 0 && e.name == name)
                    .unwrap();
                let local_changes = match crate::worktree::worktree_blob(
                    &work_tree,
                    name,
                    algo,
                    filemode,
                ) {
                    Some((oid, mode)) => oid != entry.oid || mode != entry.mode,
                    None => true,
                };
                let staged_changes = no_head
                    || match head_map.get(name) {
                        None => true,
                        Some((mode, oid)) => {
                            *mode != entry.mode || *oid != entry.oid
                        }
                    };
                if local_changes && staged_changes {
                    if !cached {
                        files_staged.push(name.to_string());
                    } else {
                        files_staged.push(name.to_string());
                    }
                } else if !cached {
                    if staged_changes {
                        files_cached.push(name.to_string());
                    }
                    if local_changes {
                        files_local.push(name.to_string());
                    }
                }
            }
        }
        if !files_staged.is_empty() || !files_cached.is_empty() || !files_local.is_empty() {
            let mut msg = String::new();
            if !files_staged.is_empty() {
                msg.push_str(&error_block(
                    &files_staged,
                    "the following file has staged content different from both the\nfile and the HEAD:",
                    "the following files have staged content different from both the\nfile and the HEAD:",
                    "(use -f to force removal)",
                ));
            }
            if !files_cached.is_empty() {
                msg.push_str(&error_block(
                    &files_cached,
                    "the following file has changes staged in the index:",
                    "the following files have changes staged in the index:",
                    "(use --cached to keep the file, or -f to force removal)",
                ));
            }
            if !files_local.is_empty() {
                msg.push_str(&error_block(
                    &files_local,
                    "the following file has local modifications:",
                    "the following files have local modifications:",
                    "(use --cached to keep the file, or -f to force removal)",
                ));
            }
            // eprintln like C (each block ends with \n already).
            eprint!("{msg}");
            return Err(CommandError::silent(1));
        }

        // Perform the removal (sorted for deterministic `rm` output).
        let mut removed: Vec<String> = index
            .entries
            .iter()
            .filter(|e| matched(&e.name))
            .map(|e| e.name.clone())
            .collect();
        removed.sort();
        removed.dedup();
        if !dry_run && !removed.is_empty() {
            // Worktree removals first (unless --cached): a failure dies
            // immediately with the index untouched, like C (`fatal: git rm:
            // '<path>': <strerror>`, exit 128). The `rm` line already
            // printed above, matching C's output on failure.
            if !quiet {
                for p in &removed {
                    writeln!(out, "rm '{p}'").map_err(|e| CommandError::fatal(e.to_string()))?;
                }
            }
            if !cached {
                for p in &removed {
                    if let Err(e) = remove_path_result(&work_tree.join(p)) {
                        return Err(CommandError::fatal(format!(
                            "fatal: git rm: '{p}': {}",
                            io_strerror(&e)
                        )));
                    }
                }
                // Prune directories left empty by the removal.
                checkout_core::prune_empty_dirs(&work_tree);
            }
            let mut new_index = read_index_or_empty(&repo)?;
            new_index.entries.retain(|e| !matched(&e.name));
            if let Some(ct) = new_index.cache_tree.as_mut() {
                for s in &specs {
                    ct.invalidate_path(s);
                }
            }
            write_index(&repo, &new_index)?;
        } else if !quiet {
            for p in &removed {
                writeln!(out, "rm '{p}'").map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }
        Ok(())
    }
}

fn error_block(files: &[String], singular: &str, plural: &str, hint: &str) -> String {
    let mut s = String::new();
    if files.len() == 1 {
        s.push_str(&format!("error: {singular}\n"));
    } else {
        s.push_str(&format!("error: {plural}\n"));
    }
    for f in files {
        s.push_str(&format!("    {f}\n"));
    }
    s.push_str(hint);
    s.push('\n');
    s
}

/// HEAD tree as path -> (mode, oid), without blob payloads.
fn head_tree_view(
    odb: &Odb,
    algo: git_hash::HashAlgorithm,
    tree_oid: &git_hash::Oid,
) -> std::collections::BTreeMap<String, (u32, git_hash::Oid)> {
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![(tree_oid.clone(), String::new())];
    let mut seen = std::collections::HashSet::new();
    while let Some((oid, prefix)) = stack.pop() {
        if !seen.insert(oid.clone()) {
            continue;
        }
        let Ok(obj) = odb.read(&oid) else { continue };
        if obj.kind != ObjectKind::Tree {
            continue;
        }
        let Ok(entries) = git_object::parse_tree(&obj.data, algo) else { continue };
        for e in entries {
            let name = String::from_utf8_lossy(&e.name).into_owned();
            let path =
                if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            let mode = u32::from_str_radix(&e.mode, 8).unwrap_or(0);
            if e.is_dir() {
                stack.push((e.oid.clone(), path));
            } else {
                out.insert(path, (mode, e.oid.clone()));
            }
        }
    }
    out
}

fn remove_path_result(full: &PathBuf) -> std::io::Result<()> {
    match std::fs::symlink_metadata(full) {
        Ok(md) if md.file_type().is_dir() && !md.file_type().is_symlink() => {
            std::fs::remove_dir_all(full).map(|_| ())
        }
        Ok(_) => std::fs::remove_file(full).map(|_| ()),
        Err(e) => Err(e),
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
