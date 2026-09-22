//! `git clean`: remove untracked files/directories from the worktree.
//!
//! Port of `builtin/clean.c` (`cmd_clean`): `-n`, `-f`, `-d`, `-x`/`-X`,
//! `-q`, `-e` excludes, pathspec limiting, and the `clean.requireForce`
//! guard. Deferred: `-i`/`--interactive` (explicit error).

use std::collections::HashSet;
use std::io::Write;

use crate::checkout_core::{self, read_index_or_empty};
use crate::ignore_util::build_engine;
use crate::{Command, CommandError, RepoContext};
use git_attributes::ignore::parse_gitignore;

pub struct Clean;

const USAGE: &str = "usage: git clean [-d] [-f] [-i] [-n] [-q] [-e <pattern>] [-x | -X] [--] [<pathspec>...]\n\n    -q, --[no-]quiet      do not print names of files removed\n    -n, --[no-]dry-run    dry run\n    -f, --[no-]force      force\n    -i, --[no-]interactive\n                          interactive cleaning\n    -d                    remove whole directories\n    -e, --exclude <pattern>\n                          add <pattern> to ignore rules\n    -x                    remove ignored files, too\n    -X                    remove only ignored files\n";

fn usage_error(first: String) -> CommandError {
    CommandError::usage(format!("{first}\n{USAGE}"))
}

impl Command for Clean {
    fn name(&self) -> &'static str {
        "clean"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut dry_run = false;
        let mut force_count = 0u32;
        let mut interactive = false;
        let mut quiet = false;
        let mut dirs = false;
        let mut excludes: Vec<String> = Vec::new();
        let mut rm_ignored = false; // -x
        let mut only_ignored = false; // -X
        let mut operands: Vec<String> = Vec::new();

        let mut i = 0usize;
        while i < args.len() {
            let a = &args[i];
            match a.as_str() {
                "-n" | "--dry-run" => dry_run = true,
                "--no-dry-run" => dry_run = false,
                "-f" | "--force" => force_count += 1,
                "--no-force" => force_count = 0,
                "-i" | "--interactive" => interactive = true,
                "--no-interactive" => interactive = false,
                "-q" | "--quiet" => quiet = true,
                "--no-quiet" => quiet = false,
                "-d" => dirs = true,
                "--no-d" => dirs = false,
                "-x" => rm_ignored = true,
                "-X" => only_ignored = true,
                "-e" | "--exclude" => {
                    i += 1;
                    excludes.push(
                        args.get(i)
                            .ok_or_else(|| {
                                usage_error("error: option `exclude' requires a value".to_string())
                            })?
                            .clone(),
                    );
                }
                s if s.starts_with("--exclude=") => {
                    excludes.push(s["--exclude=".len()..].to_string())
                }
                s if s.starts_with("-e") && s.len() > 2 => {
                    excludes.push(s[2..].to_string())
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
                    // Bundled shorts (-fdn, -ndx, ...). Only single-char
                    // flags bundle; -e takes the rest as its value.
                    let chars: Vec<char> = s[1..].chars().collect();
                    let mut j = 0usize;
                    while j < chars.len() {
                        match chars[j] {
                            'n' => dry_run = true,
                            'f' => force_count += 1,
                            'q' => quiet = true,
                            'd' => dirs = true,
                            'x' => rm_ignored = true,
                            'X' => only_ignored = true,
                            'i' => interactive = true,
                            'e' => {
                                let rest: String = chars[j + 1..].iter().collect();
                                if !rest.is_empty() {
                                    excludes.push(rest);
                                } else {
                                    i += 1;
                                    excludes.push(
                                        args.get(i).ok_or_else(|| {
                                            usage_error(
                                                "error: option `exclude' requires a value"
                                                    .to_string(),
                                            )
                                        })?
                                        .clone(),
                                    );
                                }
                                break;
                            }
                            c => {
                                return Err(usage_error(format!(
                                    "error: unknown switch `{c}'"
                                )));
                            }
                        }
                        j += 1;
                    }
                }
                s => operands.push(s.to_string()),
            }
            i += 1;
        }

        if rm_ignored && only_ignored {
            return Err(CommandError::fatal(
                "fatal: options '-x' and '-X' cannot be used together",
            ));
        }
        if interactive {
            return Err(CommandError::fatal(
                "fatal: clean: --interactive is not supported yet",
            ));
        }

        let repo = ctx.repository()?;
        let work_tree = repo
            .work_tree
            .clone()
            .ok_or_else(|| CommandError::fatal("fatal: this operation must be run in a work tree"))?;

        // Force requirement (C checks config + -f/-n/-i).
        let force = force_count > 0;
        // A doubled -f overrides the nested-repository protection, like C.
        let keep_nested = force_count < 2;
        if !force && !dry_run {
            let require = repo
                .config
                .get_bool("clean", "requireforce")
                .unwrap_or(true);
            if require {
                return Err(CommandError::fatal(
                    "fatal: clean.requireForce is true and -f not given: refusing to clean",
                ));
            }
        }

        // Ignore engine: -x drops the file-based rules, but -e patterns
        // always participate (they form EXC_CMDL in every mode, so -e
        // matches are segregated as ignored even under -x and surface
        // only under -X).
        let mut engine = if rm_ignored {
            git_attributes::ignore::IgnoreEngine::new()
        } else {
            build_engine(&repo, &ctx.cwd)
        };
        if !excludes.is_empty() {
            let joined = excludes.join("\n");
            let list = parse_gitignore(&joined, "", "<cmdline>", 0);
            engine.add_cmdline_patterns(list);
        }
        let ignored_of = |engine: &mut git_attributes::ignore::IgnoreEngine,
                          rel: &str,
                          is_dir: bool|
         -> bool {
            engine.is_excluded(rel, is_dir).is_some_and(|m| !m.is_negative)
        };

        let index = read_index_or_empty(&repo)?;
        let tracked: HashSet<&str> =
            index.entries.iter().map(|e| e.name.as_str()).collect();

        // Repo-relative cwd ("", at the root). With no pathspec, C limits
        // the clean to the current directory and prints paths relative to
        // it; an explicit pathspec is resolved against the cwd instead.
        let cwd_rel = ctx
            .cwd
            .canonicalize()
            .ok()
            .and_then(|c| {
                c.strip_prefix(&work_tree).ok().map(|p| p.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        let mut specs: Vec<String> = Vec::new();
        if operands.is_empty() && !cwd_rel.is_empty() {
            specs.push(cwd_rel.clone());
        } else {
            for o in &operands {
                specs.push(checkout_core::resolve_inside(ctx, &repo, &work_tree, o, true)?);
            }
        }
        let in_scope =
            |rel: &str| specs.is_empty() || specs.iter().any(|s| checkout_core::spec_matches_glob(s, rel));
        // Display paths relative to the invoking directory, like C
        // (`clean -n ../src` from docs/ prints `../src/part3.c`). When a
        // subdirectory run names "." itself, C refuses the cwd but prints
        // matches with a "./" prefix.
        let dot_spec = !cwd_rel.is_empty() && operands.iter().any(|o| o == "." || o == "./");
        let display = |rel: &str| -> String {
            if dot_spec {
                return format!("./{rel}");
            }
            if rel == cwd_rel {
                return ".".to_string();
            }
            if !cwd_rel.is_empty() {
                if let Some(rest) = rel.strip_prefix(&format!("{cwd_rel}/")) {
                    return rest.to_string();
                }
                // Outside the cwd: walk up with "..".
                let cwd_comps: Vec<&str> = cwd_rel.split('/').collect();
                let rel_comps: Vec<&str> = rel.split('/').collect();
                let common = cwd_comps
                    .iter()
                    .zip(rel_comps.iter())
                    .take_while(|(a, b)| a == b)
                    .count();
                let mut out = vec![".."; cwd_comps.len() - common].join("/");
                for c in &rel_comps[common..] {
                    if !out.is_empty() {
                        out.push('/');
                    }
                    out.push_str(c);
                }
                return out;
            }
            rel.to_string()
        };
        if dot_spec {
            if dry_run {
                writeln!(out, "Would refuse to remove current working directory")
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
            } else {
                writeln!(out, "Refusing to remove current working directory")
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }

        // Walk the worktree in two phases. Phase 1 descends everywhere
        // except `.git` and nested repositories, recording every file with
        // its tracked/ignored/scope status. Phase 2 collapses directories
        // (with -d) whose entire subtree is removal-eligible — C removes
        // such dirs as a unit ("dir/") but descends into dirs holding kept
        // (e.g. ignored) files, listing survivors individually.
        struct FileInfo {
            rel: String,
            tracked: bool,
            eligible: bool,
        }
        let mut file_infos: Vec<FileInfo> = Vec::new();
        // All directories seen (relative, "" = root), for collapse checks.
        // Nested repositories (skipped above) block collapsing.
        let mut all_dirs: Vec<String> = Vec::new();
        let mut nested_dirs: Vec<String> = Vec::new();
        let mut stack = vec![work_tree.clone()];
        // Unreadable directories produce C's traversal warning (no exit-code
        // effect on their own).
        while let Some(dir) = stack.pop() {
            let rel_dir = dir
                .strip_prefix(&work_tree)
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !rel_dir.is_empty() {
                all_dirs.push(rel_dir.clone());
            }
            let rd = match std::fs::read_dir(&dir) {
                Ok(rd) => rd,
                Err(e) => {
                    if !rel_dir.is_empty() && rel_dir != ".git" {
                        eprintln!(
                            "warning: could not open directory '{rel_dir}/': {}",
                            io_strerror(&e)
                        );
                    }
                    continue;
                }
            };
            let mut entries: Vec<_> = rd.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let name = e.file_name().to_string_lossy().into_owned();
                if name == ".git" {
                    continue;
                }
                let rel = if rel_dir.is_empty() {
                    name.clone()
                } else {
                    format!("{rel_dir}/{name}")
                };
                let is_dir = e
                    .file_type()
                    .is_ok_and(|t| t.is_dir() && !t.is_symlink());
                // Nested repositories are skipped silently (never descended,
                // even for explicit pathspecs) and block collapsing (their
                // contents must survive a parent-dir removal). Only real
                // repositories count: look-alikes (garbage HEAD/gitfile)
                // are cleaned like ordinary directories. A doubled -f
                // overrides the protection entirely.
                if is_dir && keep_nested && is_nested_repo(&e.path()) {
                    nested_dirs.push(rel.clone());
                    continue;
                }
                if is_dir {
                    // An ignored directory need not be traversed in normal
                    // mode (its contents are implicitly ignored). Under -X
                    // it collapses as a unit (recorded for phase 2 via
                    // all_dirs); with -e excludes present, descend instead
                    // so file-level excludes still apply.
                    let dir_ignored =
                        !only_ignored && !rm_ignored && ignored_of(&mut engine, &rel, true);
                    if dir_ignored {
                        continue;
                    }
                    stack.push(e.path());
                    continue;
                }
                let is_tracked = tracked.contains(rel.as_str());
                if is_tracked {
                    file_infos.push(FileInfo { rel, tracked: true, eligible: false });
                    continue;
                }
                if !in_scope(&rel) {
                    file_infos.push(FileInfo { rel, tracked: false, eligible: false });
                    continue;
                }
                let ignored = ignored_of(&mut engine, &rel, false);
                // -x only drops the file-based rules from the engine; an
                // -e match still segregates the path as ignored (removed
                // solely under -X).
                let eligible = if only_ignored {
                    ignored
                } else {
                    !ignored
                };
                file_infos.push(FileInfo { rel, tracked: false, eligible });
            }
        }

        // Phase 2: collapse fully-eligible untracked directories. With -d,
        // any fully-eligible dir collapses; without -d only -X collapses,
        // and only ignored dirs (an empty non-ignored dir is not ignored).
        // A dir collapses when its subtree holds no tracked files and no
        // kept (surviving) untracked files. Empty dirs collapse too.
        let mut removals: Vec<String> = file_infos
            .iter()
            .filter(|f| f.eligible)
            .map(|f| f.rel.clone())
            .collect();
        if dirs || only_ignored {
            // Deepest first so nested collapses subsume correctly.
            let mut dirs_sorted = all_dirs.clone();
            dirs_sorted.sort_by_key(|d| std::cmp::Reverse(d.len()));
            let mut collapsed: HashSet<String> = HashSet::new();
            for dir in &dirs_sorted {
                if collapsed.iter().any(|c| dir.starts_with(&format!("{c}/"))) {
                    continue; // already subsumed.
                }
                let under: Vec<&FileInfo> = file_infos
                    .iter()
                    .filter(|f| f.rel.starts_with(&format!("{dir}/")))
                    .collect();
                // Any tracked content, any surviving untracked file, or
                // any nested repository beneath blocks collapsing.
                if under.iter().any(|f| f.tracked || !f.eligible) {
                    continue;
                }
                if nested_dirs.iter().any(|n| {
                    *n == *dir || n.starts_with(&format!("{dir}/"))
                }) {
                    continue;
                }
                // The dir itself must be in scope. A literal *directory*
                // spec collapses (via in_scope); a literal *file* spec never
                // collapses its parent (C lists the file); a glob collapses
                // when it matches the dir itself ("newdir/*" vs "newdir/").
                let dir_scoped = in_scope(dir)
                    || specs.iter().any(|s| {
                        s.contains(['*', '?', '[']) && spec_dir_matches_glob(s, dir)
                    });
                if !dir_scoped {
                    continue;
                }
                // Ignored dirs collapse only under -X. Without -d, -X
                // additionally requires ignored content beneath (an empty
                // non-ignored dir is kept; a dir whose files are all
                // ignored collapses as a unit).
                if !only_ignored && ignored_of(&mut engine, dir, true) {
                    continue;
                }
                if only_ignored && !dirs {
                    let any_file = under.iter().any(|f| !f.tracked);
                    if !any_file {
                        continue;
                    }
                    // (All files beneath are eligible⟺ignored here —
                    // anything kept would have blocked above.)
                }
                collapsed.insert(dir.clone());
                let prefix = format!("{dir}/");
                removals.retain(|f| !f.starts_with(&prefix));
                removals.push(format!("{dir}/"));
            }
        }
        if !dirs && !only_ignored {
            // Without -d (and not -X, which lists eligible files
            // individually wherever they are), files beneath
            // fully-untracked directories are not listed (C skips such dirs
            // wholesale) — unless a pathspec points beneath the directory
            // (explicit paths still apply).
            removals.retain(|f| {
                if !f.contains('/') {
                    return true;
                }
                // Walk ancestors from the top; the first fully-untracked
                // one swallows the file.
                let mut prefix = String::new();
                for comp in f.split('/').take(f.split('/').count() - 1) {
                    if !prefix.is_empty() {
                        prefix.push('/');
                    }
                    prefix.push_str(comp);
                    let has_tracked = file_infos.iter().any(|fi| {
                        fi.tracked
                            && (fi.rel == prefix
                                || fi.rel.starts_with(&format!("{prefix}/")))
                    });
                    if has_tracked {
                        continue;
                    }
                    let spec_beneath =
                        specs.iter().any(|s| spec_may_match_beneath(s, &prefix));
                    if !spec_beneath {
                        return false;
                    }
                }
                true
            });
        }
        removals.sort();

        if dry_run {
            if !quiet {
                for p in &removals {
                    writeln!(out, "Would remove {}", display(p))
                        .map_err(|e| CommandError::fatal(e.to_string()))?;
                }
            }
            return Ok(());
        }
        let mut failed = false;
        for p in &removals {
            let full = work_tree.join(p.trim_end_matches('/'));
            let res = match std::fs::symlink_metadata(&full) {
                Ok(md) if md.file_type().is_dir() && !md.file_type().is_symlink() => {
                    // Like C, fall back to a plain rmdir when the recursive
                    // removal cannot even read the directory (an unreadable
                    // but empty dir is still removable).
                    std::fs::remove_dir_all(&full)
                        .or_else(|_| std::fs::remove_dir(&full))
                        .map(|_| ())
                }
                Ok(_) => std::fs::remove_file(&full).map(|_| ()),
                Err(e) => Err(e),
            };
            if let Err(e) = res {
                // C: `warning: failed to remove <path>: <strerror>`, and the
                // command exits nonzero when anything failed.
                eprintln!("warning: failed to remove {p}: {}", io_strerror(&e));
                failed = true;
                continue;
            }
            if !quiet {
                writeln!(out, "Removing {}", display(p))
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }
        if failed {
            return Err(CommandError::silent(1));
        }
        Ok(())
    }
}

/// Whether `dir` is a nested repository (a valid git dir or a valid gitfile
/// link), mirroring C's `is_nonbare_repository_dir`: look-alikes with a
/// garbage HEAD or gitfile are ordinary directories and get cleaned.
fn is_nested_repo(dir: &std::path::Path) -> bool {
    let dotgit = dir.join(".git");
    match std::fs::symlink_metadata(&dotgit) {
        Err(_) => false,
        Ok(md) => {
            if md.file_type().is_symlink() {
                // A HEAD symlink counts when it points into refs/.
                return std::fs::read_link(&dotgit)
                    .is_ok_and(|t| t.to_string_lossy().starts_with("refs/"));
            }
            if md.is_dir() {
                return is_git_directory(&dotgit);
            }
            // Plain file: a valid `gitdir: <path>` link (unreadable files
            // count too, conservatively).
            match std::fs::read_to_string(&dotgit) {
                Ok(content) => content
                    .strip_prefix("gitdir:")
                    .is_some_and(|r| !r.trim().is_empty()),
                Err(_) => true,
            }
        }
    }
}

/// Whether `gitdir` looks like a git directory: a valid HEAD (symref,
/// detached hex, or refs/ symlink) plus `objects/` and `refs/`.
fn is_git_directory(gitdir: &std::path::Path) -> bool {
    if !headref_valid(&gitdir.join("HEAD")) {
        return false;
    }
    gitdir.join("objects").is_dir() && gitdir.join("refs").is_dir()
}

/// Whether a HEAD file/symlink is well-formed (C's `validate_headref`).
fn headref_valid(head: &std::path::Path) -> bool {
    let Ok(md) = std::fs::symlink_metadata(head) else {
        return false;
    };
    if md.file_type().is_symlink() {
        return std::fs::read_link(head)
            .is_ok_and(|t| t.to_string_lossy().starts_with("refs/"));
    }
    match std::fs::read_to_string(head) {
        Ok(c) => {
            let t = c.trim();
            if let Some(rest) = t.strip_prefix("ref:") {
                rest.trim_start().starts_with("refs/")
            } else {
                (t.len() == 40 || t.len() == 64)
                    && t.bytes().all(|b| b.is_ascii_hexdigit())
            }
        }
        Err(_) => false,
    }
}

fn io_strerror(e: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::NotFound => "No such file or directory".to_string(),
        ErrorKind::PermissionDenied => "Permission denied".to_string(),
        ErrorKind::AlreadyExists => "File exists".to_string(),
        ErrorKind::DirectoryNotEmpty => "Directory not empty".to_string(),
        _ => {
            let s = e.to_string();
            match s.rfind(" (os error ") {
                Some(i) => s[..i].to_string(),
                None => s,
            }
        }
    }
}

/// Does glob pathspec `spec` match directory `dir` itself (tested with a
/// trailing slash, so `newdir/*` matches `newdir/`)?
fn spec_dir_matches_glob(spec: &str, dir: &str) -> bool {
    git_attributes::wildmatch(spec, &format!("{dir}/"), 0) == git_attributes::WM_MATCH
}

/// Could pathspec `spec` match anything beneath directory `dir` (for
/// deciding descent / swallow rules)? Literals use prefix comparison; globs
/// use the literal text before the first glob character.
fn spec_may_match_beneath(spec: &str, dir: &str) -> bool {
    if !spec.contains(['*', '?', '[']) {
        return spec.starts_with(&format!("{dir}/"));
    }
    let lit_end = spec.find(['*', '?', '[']).unwrap_or(spec.len());
    let lit = spec[..lit_end].trim_end_matches('/');
    lit.is_empty() || lit == dir || lit.starts_with(&format!("{dir}/")) || dir.starts_with(&format!("{lit}/"))
}
