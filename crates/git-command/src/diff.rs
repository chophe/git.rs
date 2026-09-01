//! `git diff`: unified diffs between worktree, index, and trees.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::patch::{self, BlobSource};
use crate::{Command, CommandError, RepoContext};
use git_hash::{HashAlgorithm, Oid};
use git_index::Index;
use git_object::{parse_tree, Object, ObjectKind};
use git_odb::Odb;

pub struct Diff;

/// Output format selected by flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Output {
    Patch,
    Stat,
    ShortStat,
    NumStat,
    NameOnly,
    NameStatus,
    Raw,
    Summary,
    None,
}

struct Options {
    output: Output,
    patch_with_stat: bool,
    context: usize,
    exit_code: bool,
    quiet: bool,
    cached: bool,
    no_index: bool,
    find_renames: bool,
    rename_threshold: u32,
    diff_filter: Option<(HashSet<char>, bool)>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            output: Output::Patch,
            patch_with_stat: false,
            context: 3,
            exit_code: false,
            quiet: false,
            cached: false,
            no_index: false,
            find_renames: true,
            rename_threshold: git_diff::MAX_SCORE / 2,
            diff_filter: None,
        }
    }
}

impl Command for Diff {
    fn name(&self) -> &'static str {
        "diff"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut opts = Options::default();
        let mut revs: Vec<String> = Vec::new();
        let mut paths: Vec<String> = Vec::new();
        let mut after_dashdash = false;
        for a in args {
            if after_dashdash {
                paths.push(a.clone());
                continue;
            }
            match a.as_str() {
                "--no-index" => opts.no_index = true,
                "--exit-code" => opts.exit_code = true,
                "--quiet" => {
                    // C git: quick implies exit_with_status.
                    opts.quiet = true;
                    opts.exit_code = true;
                }
                "--stat" => opts.output = Output::Stat,
                "--shortstat" => opts.output = Output::ShortStat,
                "--numstat" => opts.output = Output::NumStat,
                "--name-only" => opts.output = Output::NameOnly,
                "--name-status" => opts.output = Output::NameStatus,
                "--raw" => opts.output = Output::Raw,
                "--summary" => opts.output = Output::Summary,
                "-s" | "--no-patch" => opts.output = Output::None,
                "--patch" => opts.output = Output::Patch,
                "--patch-with-stat" => {
                    opts.output = Output::Patch;
                    opts.patch_with_stat = true;
                }
                "--cached" | "--staged" => opts.cached = true,
                "--no-renames" => opts.find_renames = false,
                "-M" | "--find-renames" => opts.find_renames = true,
                s if s.starts_with("--find-renames=") => {
                    opts.find_renames = true;
                    opts.rename_threshold = parse_percent(&s["--find-renames=".len()..]);
                }
                s if s.starts_with("-U") && s.len() > 2 => {
                    opts.context = s[2..].parse().unwrap_or(3);
                }
                s if s.starts_with("--unified=") => {
                    opts.context = s["--unified=".len()..].parse().unwrap_or(3);
                }
                s if s.starts_with("--diff-filter=") => {
                    let spec = &s["--diff-filter=".len()..];
                    let lower = spec.chars().all(|c| c.is_ascii_lowercase());
                    let set: HashSet<char> =
                        spec.chars().map(|c| c.to_ascii_uppercase()).collect();
                    opts.diff_filter = Some((set, lower));
                }
                "--" => after_dashdash = true,
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("diff: option '{s}' not supported")));
                }
                s => {
                    if revs.len() < 2 && !s.contains('/') && !s.contains('.') {
                        revs.push(s.to_string());
                    } else {
                        paths.push(s.to_string());
                    }
                }
            }
        }

        if opts.no_index {
            if paths.len() != 2 {
                return Err(CommandError::usage("diff --no-index: requires two paths"));
            }
            return no_index_diff(&paths[0], &paths[1], out);
        }

        let repo = ctx.repository()?;
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let algo = repo.hash_algo;
        let mut extra: HashMap<Oid, Vec<u8>> = HashMap::new();
        let index = match Index::read(&repo.index_file(), algo) {
            Ok(ix) => ix,
            Err(_) => Index::default(),
        };

        if revs.len() == 2 {
            let t1 = resolve_tree(&repo, &odb, &revs[0])?;
            let t2 = resolve_tree(&repo, &odb, &revs[1])?;
            let has = run_diff(&odb, &extra, Some(t1), t2, &paths, &opts, out, &HashSet::new(), repo.work_tree.as_deref())?;
            return finish(has, opts.exit_code);
        }

        let index_tree = build_index_tree(&index, &mut extra, algo);
        let (work_tree, dirty) = build_worktree_tree(&repo, &index, &mut extra, algo)?;

        let (old_tree, new_tree) = if revs.len() == 1 {
            let rev_tree = resolve_tree(&repo, &odb, &revs[0])?;
            if opts.cached {
                (Some(rev_tree), index_tree)
            } else {
                (Some(rev_tree), work_tree)
            }
        } else {
            // `git diff` with no revision: index vs worktree.
            (Some(index_tree), work_tree)
        };

        let has = run_diff(&odb, &extra, old_tree, new_tree, &paths, &opts, out, &dirty, repo.work_tree.as_deref())?;
        finish(has, opts.exit_code)
    }
}

fn finish(has_changes: bool, exit_code: bool) -> Result<(), CommandError> {
    if exit_code && has_changes {
        Err(CommandError::silent(1))
    } else {
        Ok(())
    }
}

fn parse_percent(s: &str) -> u32 {
    let n: u32 = s.trim_end_matches('%').parse().unwrap_or(50);
    (n.min(100) * git_diff::MAX_SCORE) / 100
}

pub(crate) fn resolve_tree(repo: &git_core::Repository, odb: &Odb, rev: &str) -> Result<Oid, CommandError> {
    let oid = crate::resolve_arg(repo, rev)?;
    let obj = odb.read(&oid).map_err(|e| CommandError::error(e.to_string()))?;
    if obj.kind == ObjectKind::Tree {
        return Ok(oid);
    }
    if obj.kind == ObjectKind::Commit {
        let commit = git_object::parse_commit(&obj.data, repo.hash_algo)
            .map_err(|e| CommandError::error(e.to_string()))?;
        return Ok(commit.tree);
    }
    Err(CommandError::error(format!("fatal: not a tree object: {rev}")))
}

#[allow(clippy::too_many_arguments)]
fn run_diff(
    odb: &Odb,
    extra: &HashMap<Oid, Vec<u8>>,
    old_tree: Option<Oid>,
    new_tree: Oid,
    paths: &[String],
    opts: &Options,
    out: &mut dyn Write,
    dirty: &HashSet<Oid>,
    worktree: Option<&std::path::Path>,
) -> Result<bool, CommandError> {
    let algo = odb.algorithm();
    let tree_entries = |oid: Option<Oid>| -> Vec<git_object::TreeEntry> {
        let Some(oid) = oid else { return Vec::new() };
        let data = if let Some(d) = extra.get(&oid) {
            d.clone()
        } else {
            match odb.read(&oid) {
                Ok(o) => o.data,
                Err(_) => return Vec::new(),
            }
        };
        parse_tree(&data, algo).unwrap_or_default()
    };
    let e1 = tree_entries(old_tree);
    let e2 = tree_entries(Some(new_tree));

    let mut loader = |oid: &Oid| -> Option<Object> {
        if let Some(data) = extra.get(oid) {
            return Some(Object::from_data(ObjectKind::Blob, data.clone()));
        }
        odb.read(oid).ok()
    };
    let mut changes = git_diff::compare_trees(&e1, &e2, "", true, &mut loader);

    if !paths.is_empty() {
        let in_scope = |p: &str| {
            paths.iter().any(|path| {
                let path = path.trim_end_matches('/');
                p == path || p.starts_with(&format!("{path}/"))
            })
        };
        changes.retain(|c| in_scope(&c.path));
    }

    if opts.find_renames {
        detect_renames(&mut changes, extra, odb, opts.rename_threshold);
    }

    // `--raw` for worktree-side blobs: dirty files have no known oid.
    let mut raw_changes: Vec<git_diff::Change> = Vec::new();
    if opts.output == Output::Raw {
        for c in &changes {
            let mut c = c.clone();
            if dirty.contains(c.new_oid.as_ref().unwrap_or(&git_hash::HashAlgorithm::Sha1.null_oid())) {
                c.new_oid = None;
            }
            raw_changes.push(c);
        }
    }

    // Read .gitattributes from worktree if available
    let gitattributes = worktree.and_then(|wt| {
        let path = wt.join(".gitattributes");
        std::fs::read_to_string(path).ok()
    });

    if let Some((set, exclude)) = &opts.diff_filter {
        changes.retain(|c| {
            let keep = set.contains(&c.status);
            if *exclude {
                !keep
            } else {
                keep
            }
        });
    }

    if changes.is_empty() {
        return Ok(false);
    }

    // C git's `--quiet` (DIFF_FORMAT_NO_OUTPUT): suppress all output but
    // still report the exit status.
    if opts.quiet {
        return Ok(true);
    }

    let src = BlobSource { odb, extra };
    if opts.patch_with_stat || opts.output == Output::Stat {
        patch::render_stat(&changes, &src, out)?;
        if opts.patch_with_stat {
            writeln!(out).map_err(|e| CommandError::fatal(e.to_string()))?;
        }
    }
    match opts.output {
        Output::Patch => {
            for c in &changes {
                let driver = git_diff::resolve_driver(&c.path, gitattributes.as_deref());
                let p = patch::render_change_patch_ctx(c, &src, opts.context, driver.as_ref())?;
                out.write_all(&p).map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }
        Output::ShortStat => patch::render_shortstat(&changes, &src, out)?,
        Output::NumStat => {
            for c in &changes {
                patch::render_numstat(c, &src, out)?;
            }
        }
        Output::NameOnly => {
            for c in &changes {
                patch::render_name_line(c, false, out)?;
            }
        }
        Output::NameStatus => {
            for c in &changes {
                patch::render_name_line(c, true, out)?;
            }
        }
        Output::Raw => {
            for c in &raw_changes {
                patch::render_raw(c, out)?;
            }
        }
        Output::Summary => {
            for c in &changes {
                patch::render_summary(c, out)?;
            }
        }
        Output::Stat | Output::None => {}
    }
    Ok(true)
}

/// Exact-match then similarity-based rename detection over A/D pairs.
fn detect_renames(
    changes: &mut Vec<git_diff::Change>,
    extra: &HashMap<Oid, Vec<u8>>,
    odb: &Odb,
    threshold: u32,
) {
    let read = |oid: &Oid| -> Option<Vec<u8>> {
        if let Some(d) = extra.get(oid) {
            return Some(d.clone());
        }
        odb.read(oid).ok().map(|o| o.data)
    };
    // Exact pass: pair deletes with adds whose blob ids match.
    let mut used_dst: HashSet<usize> = HashSet::new();
    for d in changes.iter() {
        if d.status != 'D' {
            continue;
        }
        let Some(doid) = d.old_oid else { continue };
        for (i, a) in changes.iter().enumerate() {
            if a.status != 'A' || used_dst.contains(&i) {
                continue;
            }
            if a.new_oid == Some(doid) {
                used_dst.insert(i);
                break;
            }
        }
    }
    // Apply exact pairs: for each delete index, find its paired add index.
    {
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        let mut seen_dst: HashSet<usize> = HashSet::new();
        for (di, d) in changes.iter().enumerate() {
            if d.status != 'D' {
                continue;
            }
            let Some(doid) = d.old_oid else { continue };
            for (i, a) in changes.iter().enumerate() {
                if a.status != 'A' || seen_dst.contains(&i) {
                    continue;
                }
                if a.new_oid == Some(doid) {
                    seen_dst.insert(i);
                    pairs.push((di, i));
                    break;
                }
            }
        }
        for (di, ai) in pairs.iter() {
            let (dpath, dmode, doid) = {
                let d = &changes[*di];
                (d.path.clone(), d.old_mode.clone(), d.old_oid)
            };
            let a = &mut changes[*ai];
            a.status = 'R';
            a.score = Some(git_diff::MAX_SCORE);
            a.old_path = Some(dpath);
            a.new_path = Some(a.path.clone());
            a.path = a.new_path.clone().unwrap_or_default();
            a.old_mode = dmode;
            a.old_oid = doid;
        }
        // Drop the paired deletes (higher index first so indices stay valid).
        let mut dels_to_remove: Vec<usize> = pairs.iter().map(|(di, _)| *di).collect();
        dels_to_remove.sort_unstable();
        for di in dels_to_remove.into_iter().rev() {
            changes.remove(di);
        }
    }
    // Similarity pass over remaining D/A pairs.
    let deletes: Vec<(usize, Oid)> = changes
        .iter()
        .enumerate()
        .filter(|(_, c)| c.status == 'D')
        .filter_map(|(i, c)| c.old_oid.map(|o| (i, o)))
        .collect();
    let adds: Vec<(usize, Oid)> = changes
        .iter()
        .enumerate()
        .filter(|(_, c)| c.status == 'A')
        .filter_map(|(i, c)| c.new_oid.map(|o| (i, o)))
        .collect();
    let mut paired: HashSet<usize> = HashSet::new();
    let mut renames: Vec<(usize, usize, u32)> = Vec::new();
    for (di, doid) in &deletes {
        let Some(src) = read(doid) else { continue };
        let src_lines = git_diff::split_lines(&src);
        let mut best: Option<(usize, u32)> = None;
        for (ai, aoid) in &adds {
            if paired.contains(ai) {
                continue;
            }
            let Some(dst) = read(aoid) else { continue };
            let dst_lines = git_diff::split_lines(&dst);
            let ops = git_diff::diff_lines(&src_lines, &dst_lines);
            let common = ops
                .iter()
                .filter(|op| matches!(op, git_diff::Op::Keep))
                .count();
            let max_lines = src_lines.len().max(dst_lines.len());
            if max_lines == 0 {
                continue;
            }
            let score = ((common * git_diff::MAX_SCORE as usize) / max_lines) as u32;
            if score >= threshold && best.map(|(_, bs)| score > bs).unwrap_or(true) {
                best = Some((*ai, score));
            }
        }
        if let Some((ai, score)) = best {
            paired.insert(ai);
            renames.push((*di, ai, score));
        }
    }
    renames.sort_by_key(|(di, _, _)| usize::MAX - *di);
    for (di, ai, score) in renames {
        let (dpath, dmode, doid) = {
            let d = &changes[di];
            (d.path.clone(), d.old_mode.clone(), d.old_oid)
        };
        let a = &mut changes[ai];
        a.status = 'R';
        a.score = Some(score);
        a.old_path = Some(dpath);
        a.new_path = Some(a.path.clone());
        a.old_mode = dmode;
        a.old_oid = doid;
        changes.remove(di);
    }
}

fn oid_to_bytes(oid: &Oid) -> Vec<u8> {
    let hex = format!("{oid}");
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0))
        .collect()
}

/// Build a tree hierarchy from flat index-like entries, storing all
/// intermediate trees in `extra` (synthetic, not written to the odb).
fn build_tree_from_flat(
    entries: Vec<(String, u32, Oid)>,
    extra: &mut HashMap<Oid, Vec<u8>>,
    algo: HashAlgorithm,
) -> Oid {
    fn write_tree(
        entries: &[(String, u32, Oid)],
        extra: &mut HashMap<Oid, Vec<u8>>,
        algo: HashAlgorithm,
    ) -> Oid {
        let mut files: Vec<(String, u32, Oid)> = Vec::new();
        let mut dirs: std::collections::BTreeMap<String, Vec<(String, u32, Oid)>> =
            std::collections::BTreeMap::new();
        for (path, mode, oid) in entries {
            match path.split_once('/') {
                Some((dir, rest)) => {
                    dirs.entry(dir.to_string())
                        .or_default()
                        .push((rest.to_string(), *mode, *oid));
                }
                None => files.push((path.clone(), *mode, *oid)),
            }
        }
        // Git sorts tree entries as if directory names had a trailing '/'.
        let mut list: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for (name, mode, oid) in files {
            let mut raw = format!("{mode:o} {name}").into_bytes();
            raw.push(0);
            raw.extend_from_slice(&oid_to_bytes(&oid));
            list.push((name.into_bytes(), raw));
        }
        for (name, sub) in dirs {
            let sub_oid = write_tree(&sub, extra, algo);
            let sort_name = format!("{name}/");
            let mut raw = format!("40000 {name}").into_bytes();
            raw.push(0);
            raw.extend_from_slice(&oid_to_bytes(&sub_oid));
            list.push((sort_name.into_bytes(), raw));
        }
        list.sort_by(|a, b| a.0.cmp(&b.0));
        let mut data = Vec::new();
        for (_, raw) in list {
            data.extend_from_slice(&raw);
        }
        let obj = Object::from_data(ObjectKind::Tree, data);
        let oid = obj.compute_id(algo);
        extra.insert(oid.clone(), obj.data);
        oid
    }
    write_tree(&entries, extra, algo)
}

/// Build the synthetic tree representing the index state.
fn build_index_tree(
    index: &Index,
    extra: &mut HashMap<Oid, Vec<u8>>,
    algo: HashAlgorithm,
) -> Oid {
    let entries: Vec<(String, u32, Oid)> = index
        .entries
        .iter()
        .map(|e| (e.name.clone(), e.mode, e.oid.clone()))
        .collect();
    build_tree_from_flat(entries, extra, algo)
}

/// Build the synthetic tree representing the worktree state: entries whose
/// stat matches the index reuse the index oid; changed files are re-hashed.
fn build_worktree_tree(
    repo: &git_core::Repository,
    index: &Index,
    extra: &mut HashMap<Oid, Vec<u8>>,
    algo: HashAlgorithm,
) -> Result<(Oid, HashSet<Oid>), CommandError> {
    let work_tree: &Path = repo.work_tree.as_deref().unwrap_or(Path::new("."));
    let mut entries: Vec<(String, u32, Oid)> = Vec::new();
    let mut dirty: HashSet<Oid> = HashSet::new();
    for e in &index.entries {
        let path: PathBuf = work_tree.join(&e.name);
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue, // deleted in the worktree
        };
        if !meta.is_file() {
            continue;
        }
        let stat_matches = {
            use std::os::unix::fs::MetadataExt;
            meta.size() as u32 == e.size
                && meta.mtime() as u32 == e.mtime_sec
                && meta.mtime_nsec() as u32 == e.mtime_nsec
        };
        let oid = if stat_matches {
            e.oid.clone()
        } else {
            let data = std::fs::read(&path)
                .map_err(|err| CommandError::error(format!("{}: {err}", e.name)))?;
            let obj = Object::from_data(ObjectKind::Blob, data);
            let oid = obj.compute_id(algo);
            extra.insert(oid.clone(), obj.data);
            dirty.insert(oid.clone());
            oid
        };
        entries.push((e.name.clone(), e.mode, oid));
    }
    Ok((build_tree_from_flat(entries, extra, algo), dirty))
}

fn no_index_diff(a_path_raw: &str, b_path_raw: &str, out: &mut dyn Write) -> Result<(), CommandError> {
    let a = std::fs::read(a_path_raw).map_err(|e| CommandError::error(format!("{a_path_raw}: {e}")))?;
    let b = std::fs::read(b_path_raw).map_err(|e| CommandError::error(format!("{b_path_raw}: {e}")))?;
    let a_path = a_path_raw.trim_start_matches('/');
    let b_path = b_path_raw.trim_start_matches('/');
    let algo = HashAlgorithm::Sha1;
    let old_oid = Object::from_data(ObjectKind::Blob, a.clone()).compute_id(algo);
    let new_oid = Object::from_data(ObjectKind::Blob, b.clone()).compute_id(algo);

    writeln!(out, "diff --git a/{a_path} b/{b_path}")
        .map_err(|e| CommandError::fatal(e.to_string()))?;
    writeln!(
        out,
        "index {}..{} 100644",
        &old_oid.to_string()[..7],
        &new_oid.to_string()[..7]
    )
    .map_err(|e| CommandError::fatal(e.to_string()))?;
    if patch::is_binary(&a) || patch::is_binary(&b) {
        writeln!(out, "Binary files a/{a_path} and b/{b_path} differ")
            .map_err(|e| CommandError::fatal(e.to_string()))?;
    } else {
        writeln!(out, "--- a/{a_path}").map_err(|e| CommandError::fatal(e.to_string()))?;
        writeln!(out, "+++ b/{b_path}").map_err(|e| CommandError::fatal(e.to_string()))?;
        out.write_all(&git_diff::diff_blobs_ctx(&a, &b, 3, None))
            .map_err(|e| CommandError::fatal(e.to_string()))?;
    }
    if a == b {
        Ok(())
    } else {
        Err(CommandError::silent(1))
    }
}
