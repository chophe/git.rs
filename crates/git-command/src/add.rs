//! `git add`: stage worktree changes into the index.
//!
//! Port of `builtin/add.c` for the non-interactive cases: pathspecs, the
//! ignore engine, `-A`/`-u`/`-n`/`-v`/`-f`, and stat-accurate index entries
//! that are byte-identical to C git for the common cases.
//!
//! Deferred (documented in FOLLOWUPS): `-p`/`-i` interactive, `-N`
//! intent-to-add (needs index v3 extended flags), `--refresh`, `--chmod`,
//! pathspec magic (`:(...)`), negation inside an ignored directory, and
//! clean/smudge or CRLF filters.

use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use crate::ignore_util::build_engine;
use crate::{Command, CommandError, RepoContext};
use git_attributes::ignore::IgnoreEngine;
use git_attributes::wildmatch::{wildmatch, WM_MATCH};
use git_index::{Index, IndexEntry};
use git_object::{Object, ObjectKind};
use git_odb::LooseStore;

pub struct Add;

#[derive(PartialEq, Eq, Clone, Copy)]
enum Mode {
    Default,
    All,
    Update,
}

/// A resolved pathspec (repo-relative).
struct Spec {
    original: String,
    full: String,
    glob: bool,
}

impl Spec {
    fn matches(&self, path: &str) -> bool {
        if self.glob {
            // Pathspec globs let `*` cross `/` (git calls wildmatch without
            // WM_PATHNAME), unlike .gitignore patterns.
            wildmatch(&self.full, path, 0) == WM_MATCH
        } else if self.full.is_empty() {
            true
        } else {
            path == self.full || path.starts_with(&format!("{}/", self.full))
        }
    }
}

impl Command for Add {
    fn name(&self) -> &'static str {
        "add"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut mode = Mode::Default;
        let mut dry_run = false;
        let mut verbose = false;
        let mut force = false;
        let mut operands: Vec<String> = Vec::new();
        let mut after_dashdash = false;

        let mut i = 0usize;
        while i < args.len() {
            let a = &args[i];
            if after_dashdash {
                operands.push(a.clone());
                i += 1;
                continue;
            }
            match a.as_str() {
                "-A" | "--all" | "--no-ignore-removal" => mode = Mode::All,
                "-u" | "--update" | "--no-all" => mode = Mode::Update,
                "-n" | "--dry-run" => dry_run = true,
                "-v" | "--verbose" => verbose = true,
                "-f" | "--force" => force = true,
                "--" => after_dashdash = true,
                "--ignore-errors" | "--ignore-missing" | "--no-warn-embedded-repo" | "--sparse" => {}
                "-p" | "--patch" | "-i" | "--interactive" => {
                    return Err(CommandError::fatal("fatal: interactive add is not supported yet"));
                }
                "-N" | "--intent-to-add" => {
                    return Err(CommandError::fatal("fatal: --intent-to-add is not supported yet"));
                }
                "--refresh" => {
                    return Err(CommandError::fatal("fatal: --refresh is not supported yet"));
                }
                s if s.starts_with("--chmod=") => {
                    return Err(CommandError::fatal("fatal: --chmod is not supported yet"));
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!(
                        "error: unknown option `{s}'\nusage: git add [<options>] [--] <pathspec>...\n"
                    )));
                }
                s => operands.push(s.to_string()),
            }
            i += 1;
        }

        let repo = ctx.repository()?;
        let algo = repo.hash_algo;
        let work_tree = repo
            .work_tree
            .clone()
            .ok_or_else(|| CommandError::fatal("fatal: this operation must be run in a work tree"))?;

        if mode == Mode::Default && operands.is_empty() {
            eprintln!("Nothing specified, nothing added.");
            eprintln!("hint: Maybe you wanted to say 'git add .'?");
            eprintln!("hint: Disable this message with \"git config set advice.addEmptyPathspec false\"");
            return Ok(());
        }

        let index_path = repo.index_file();
        let mut index = match Index::read(&index_path, algo) {
            Ok(ix) => ix,
            Err(git_index::IndexError::Io(_)) => Index::default(),
            Err(e) => return Err(CommandError::fatal(format!("fatal: {e}"))),
        };
        let tracked: std::collections::HashSet<String> =
            index.entries.iter().map(|e| e.name.clone()).collect();

        let cwd_rel = ctx
            .cwd
            .canonicalize()
            .ok()
            .and_then(|c| c.strip_prefix(&work_tree).ok().map(|p| p.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let mut specs: Vec<Spec> = Vec::new();
        if operands.is_empty() {
            specs.push(Spec { original: String::new(), full: String::new(), glob: false });
        } else {
            for op in &operands {
                specs.push(resolve_spec(op, &cwd_rel, &work_tree));
            }
        }

        let mut engine = build_engine(&repo);
        let mut walker = Walker {
            root: work_tree.clone(),
            engine: &mut engine,
            specs: &specs,
            tracked: &tracked,
            mode,
            force,
            adds: Vec::new(),
            ignored: Vec::new(),
            spec_hit: vec![false; specs.len()],
        };
        walker.walk(&work_tree, "", false);

        // Removals: tracked paths whose worktree file is gone.
        let mut removes: Vec<String> = Vec::new();
        for e in &index.entries {
            if specs.iter().any(|s| s.matches(&e.name)) && e.stage == 0
                && std::fs::symlink_metadata(work_tree.join(&e.name)).is_err()
            {
                removes.push(e.name.clone());
            }
        }
        for (si, s) in specs.iter().enumerate() {
            if !walker.spec_hit[si] && removes.iter().any(|r| s.matches(r)) {
                walker.spec_hit[si] = true;
            }
        }
        if let Some(si) = walker.spec_hit.iter().position(|h| !h) {
            return Err(CommandError::fatal(format!(
                "fatal: pathspec '{}' did not match any files",
                specs[si].original
            )));
        }

        let mut exit_status = 0;
        if !walker.ignored.is_empty() {
            eprintln!("The following paths are ignored by one of your .gitignore files:");
            for p in &walker.ignored {
                eprintln!("{p}");
            }
            eprintln!("hint: Use -f if you really want to add them.");
            eprintln!("hint: Disable this message with \"git config set advice.addIgnoredFile false\"");
            exit_status = 1;
        }

        // Build entries for candidate files, keeping only those that actually
        // change the index (C only reports/invalidates changed paths).
        let filemode = repo.config.get_bool("core", "filemode").unwrap_or(true);
        let mut changes: Vec<(String, IndexEntry, Vec<u8>)> = Vec::new();
        for rel in &walker.adds {
            let (entry, data) = stat_entry(&work_tree.join(rel), rel, algo, filemode)?;
            let same = index
                .entries
                .iter()
                .find(|e| e.name == *rel && e.stage == 0)
                .map(|e| *e == entry)
                .unwrap_or(false);
            if !same {
                changes.push((rel.clone(), entry, data));
            }
        }

        let mut events: Vec<(String, bool)> = Vec::new();
        for (p, _, _) in &changes {
            events.push((p.clone(), true));
        }
        for p in &removes {
            events.push((p.clone(), false));
        }
        events.sort();
        events.dedup();
        if dry_run || verbose {
            for (p, is_add) in &events {
                let verb = if *is_add { "add" } else { "remove" };
                writeln!(out, "{verb} '{p}'").map_err(|e| CommandError::fatal(e.to_string()))?;
                out.flush().map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }

        if dry_run {
            return if exit_status == 0 { Ok(()) } else { Err(CommandError::silent(1)) };
        }

        if !changes.is_empty() || !removes.is_empty() {
            let store = LooseStore::from_repo(&repo);
            let remove_set: std::collections::HashSet<&str> =
                removes.iter().map(String::as_str).collect();
            index.entries.retain(|e| !remove_set.contains(e.name.as_str()));
            for (rel, entry, data) in &changes {
                let obj = Object::from_data(ObjectKind::Blob, data.clone());
                store.write(&obj).map_err(CommandError::from)?;
                index.entries.retain(|x| x.name != *rel);
                index.entries.push(entry.clone());
            }
            index.entries.sort_by(|a, b| a.name.cmp(&b.name));
            // C invalidates only the cache-tree nodes covering the changed
            // paths (`cache_tree_invalidate_path`), keeping other subtrees
            // valid; replicate that so the index bytes match.
            if let Some(ct) = index.cache_tree.as_mut() {
                for (p, _, _) in &changes {
                    ct.invalidate_path(p);
                }
                for p in &removes {
                    ct.invalidate_path(p);
                }
            }
            index.write(&index_path, algo).map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
        }

        if exit_status == 0 { Ok(()) } else { Err(CommandError::silent(1)) }
    }
}

struct Walker<'a> {
    root: PathBuf,
    engine: &'a mut IgnoreEngine,
    specs: &'a [Spec],
    tracked: &'a std::collections::HashSet<String>,
    mode: Mode,
    force: bool,
    adds: Vec<String>,
    ignored: Vec<String>,
    spec_hit: Vec<bool>,
}

impl Walker<'_> {
    fn walk(&mut self, dir: &Path, prefix: &str, ancestor_ignored: bool) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == ".git" {
                continue;
            }
            let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            let Ok(md) = std::fs::symlink_metadata(e.path()) else { continue };
            let is_dir = md.is_dir();
            let ignored = !self.force
                && (ancestor_ignored || self.engine.is_excluded(&rel, is_dir).is_some());

            // Which specs does this path count as matching?
            for (si, s) in self.specs.iter().enumerate() {
                let counts = is_dir
                    || !ignored
                    || (!s.glob && s.full == rel);
                if counts && s.matches(&rel) {
                    self.spec_hit[si] = true;
                }
            }

            if is_dir {
                // Ignored directories are skipped silently (only explicitly
                // named ignored *files* produce the warning, like C).
                if !ignored {
                    self.walk(&e.path(), &rel, false);
                }
                continue;
            }

            if ignored {
                if !self.force && self.specs.iter().any(|s| !s.glob && s.full == rel) {
                    self.ignored.push(rel);
                }
                continue;
            }

            let is_tracked = self.tracked.contains(&rel);
            let wanted = match self.mode {
                Mode::Update => is_tracked,
                Mode::Default | Mode::All => true,
            };
            if wanted && self.specs.iter().any(|s| s.matches(&rel)) {
                self.adds.push(rel);
            }
        }
        let _ = &self.root;
    }
}

/// Resolve a command-line pathspec to a repo-relative literal or glob.
fn resolve_spec(op: &str, cwd_rel: &str, work_tree: &Path) -> Spec {
    let rel_to_root = if Path::new(op).is_absolute() {
        Path::new(op)
            .strip_prefix(work_tree)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| op.to_string())
    } else if cwd_rel.is_empty() {
        op.to_string()
    } else {
        format!("{cwd_rel}/{op}")
    };
    let normalized = normalize_path(&rel_to_root);
    let glob = normalized.contains(['*', '?', '[']);
    Spec { original: op.to_string(), full: normalized, glob }
}

/// Lexically normalize a repo-relative path (`a/./b`, `a/../b`, trailing `/`).
fn normalize_path(p: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for comp in Path::new(p).components() {
        match comp {
            Component::Normal(c) => parts.push(c.to_str().unwrap_or("")),
            Component::ParentDir => {
                parts.pop();
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    parts.join("/")
}

/// Build an index entry (with a computed, not written, blob id) plus the raw
/// blob bytes for the worktree file at `full`.
fn stat_entry(
    full: &Path,
    rel: &str,
    algo: git_hash::HashAlgorithm,
    filemode: bool,
) -> Result<(IndexEntry, Vec<u8>), CommandError> {
    let md = std::fs::symlink_metadata(full)
        .map_err(|e| CommandError::fatal(format!("fatal: unable to stat '{}': {e}", full.display())))?;
    let ft = md.file_type();
    let (data, mode) = if ft.is_symlink() {
        let target = std::fs::read_link(full)
            .map_err(|e| CommandError::fatal(format!("fatal: unable to read '{}': {e}", full.display())))?;
        (target.to_string_lossy().into_owned().into_bytes(), 0o120000u32)
    } else if ft.is_file() {
        let data = std::fs::read(full)
            .map_err(|e| CommandError::fatal(format!("fatal: unable to read '{}': {e}", full.display())))?;
        let mode = if filemode && (md.mode() & 0o111) != 0 { 0o100755 } else { 0o100644 };
        (data, mode)
    } else {
        (Vec::new(), 0o160000u32)
    };
    let oid = Object::from_data(ObjectKind::Blob, data.clone()).compute_id(algo);
    let entry = IndexEntry {
        ctime_sec: md.ctime() as u32,
        ctime_nsec: md.ctime_nsec() as u32,
        mtime_sec: md.mtime() as u32,
        mtime_nsec: md.mtime_nsec() as u32,
        dev: md.dev() as u32,
        ino: md.ino() as u32,
        mode,
        uid: md.uid(),
        gid: md.gid(),
        size: md.size() as u32,
        oid,
        assume_valid: false,
        stage: 0,
        name: rel.to_string(),
    };
    Ok((entry, data))
}
