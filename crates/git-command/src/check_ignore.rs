//! `git check-ignore`: report whether paths are ignored.
//!
//! Port of `builtin/check-ignore.c`: loads the standard exclude lists
//! (`.git/info/exclude`, `core.excludesFile`, and per-directory `.gitignore`
//! files) and reports which paths are ignored. Supports the `-v` (verbose),
//! `-q` (quiet), `-n` (non-matching), `--stdin`, and `-z` (NUL) options.
//! Exit status is `!num_ignored` (0 when any path is ignored, 1 when none is).

use std::io::{BufRead, Write};
use std::path::PathBuf;

use git_attributes::ignore::{parse_gitignore, IgnoreEngine, PatternList};
use git_core::Repository;

use crate::{Command, CommandError, RepoContext};

/// Load `$GIT_DIR/info/exclude` into a pattern list.
fn info_exclude(repo: &Repository) -> PatternList {
    let path = repo.common_dir.join("info").join("exclude");
    let path_str = path.to_string_lossy().into_owned();
    match std::fs::read_to_string(&path) {
        Ok(content) => parse_gitignore(&content, "", &path_str, 0),
        Err(_) => PatternList {
            patterns: Vec::new(),
            src: path_str,
        },
    }
}

/// Load `core.excludesFile` (if configured) into a pattern list.
fn global_excludes(repo: &Repository) -> PatternList {
    let configured = repo
        .config
        .get("core", "excludesfile")
        .map(|v| v.to_string())
        .unwrap_or_default();
    if configured.is_empty() {
        return PatternList::default();
    }
    match std::fs::read_to_string(&configured) {
        Ok(content) => parse_gitignore(&content, "", &configured, 0),
        Err(_) => PatternList::default(),
    }
}

/// Collect every `.gitignore` file under the work tree (recursively), each
/// scoped to its directory. Patterns are later matched with their `base`, so
/// a `deep/.gitignore` only applies to paths under `deep/`; loading them all
/// up front replicates C git's on-demand per-directory loading.
fn collect_gitignores(repo: &Repository) -> Vec<PatternList> {
    let work_tree = match &repo.work_tree {
        Some(wt) => wt.clone(),
        None => return Vec::new(),
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| work_tree.clone());
    let mut lists = Vec::new();
    let mut todo: Vec<PathBuf> = vec![work_tree.clone()];
    let mut visited = std::collections::HashSet::new();
    while let Some(dir) = todo.pop() {
        if !visited.insert(dir.clone()) {
            continue;
        }
        let gitignore = dir.join(".gitignore");
        if let Ok(content) = std::fs::read_to_string(&gitignore) {
            let base = dir
                .strip_prefix(&work_tree)
                .unwrap_or(&dir)
                .to_string_lossy()
                .into_owned();
            let src = gitignore
                .strip_prefix(&cwd)
                .unwrap_or(&gitignore)
                .to_string_lossy()
                .into_owned();
            lists.push(parse_gitignore(&content, &base, &src, 0));
        }
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() && p.file_name().map(|n| n != ".git").unwrap_or(false) {
                    todo.push(p);
                }
            }
        }
    }
    lists
}

pub struct CheckIgnore;

impl Command for CheckIgnore {
    fn name(&self) -> &'static str {
        "check-ignore"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut quiet = false;
        let mut verbose = false;
        let mut stdin_paths = false;
        let mut nul_term = false;
        let mut show_non_matching = false;
        let mut paths: Vec<String> = Vec::new();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-q" | "--quiet" => quiet = true,
                "-v" | "--verbose" => verbose = true,
                "--stdin" => stdin_paths = true,
                "-z" => nul_term = true,
                "-n" | "--non-matching" => show_non_matching = true,
                "--no-index" => { /* accepted: no index load needed for exclusions */ }
                "--" => {
                    i += 1;
                    paths.extend(args[i..].iter().cloned());
                    break;
                }
                s if s.starts_with('-') => {
                    return Err(CommandError::usage(
                        "usage: git check-ignore [<options>] <pathname>...\n\
                         \n    -q, --quiet             suppress progress reporting\n\
                         \n        --stdin             read file names from stdin\n\
                         -z                          terminate input and output records by a NUL character\n\
                         -n, --non-matching          show non-matching input paths\n\
                             --no-index              ignore index when checking\n\
                         -v, --verbose               be verbose\n",
                    ));
                }
                s => paths.push(s.to_string()),
            }
            i += 1;
        }

        // Argument validation paralleling C git (all `die()` -> "fatal: ...").
        if stdin_paths {
            if !paths.is_empty() {
                return Err(CommandError::fatal("fatal: cannot specify pathnames with --stdin"));
            }
        } else {
            if nul_term {
                return Err(CommandError::fatal("fatal: -z only makes sense with --stdin"));
            }
            if paths.is_empty() {
                return Err(CommandError::fatal("fatal: no path specified"));
            }
        }
        if quiet {
            if paths.len() > 1 {
                return Err(CommandError::fatal(
                    "--quiet is only valid with a single pathname",
                ));
            }
            if verbose {
                return Err(CommandError::fatal("fatal: cannot have both --quiet and --verbose"));
            }
        }
        if show_non_matching && !verbose {
            return Err(CommandError::fatal(
                "--non-matching is only valid with --verbose",
            ));
        }

        let repo = ctx.repository()?;
        if repo.work_tree.is_none() {
            return Err(CommandError::fatal("check-ignore needs a working tree"));
        }

        // Load standard excludes.
        let mut engine = IgnoreEngine::new();
        for pl in collect_gitignores(&repo) {
            engine.add_dir_patterns(pl);
        }
        engine.add_dir_patterns(info_exclude(&repo));
        let glob = global_excludes(&repo);
        if !glob.patterns.is_empty() {
            engine.add_global_patterns(glob);
        }

        let num_ignored = std::cell::Cell::new(0usize);

        let mut check_one = |path: &str, out: &mut dyn Write| {
            let is_dir = std::path::Path::new(path).is_dir();
            let matched = engine.last_matching_pattern(path, is_dir);

            // C git strips a NEGATIVE (re-inclusion) match in non-verbose
            // mode: it is neither displayed nor counted as ignored.
            let is_negative = matched.map(|p| p.flags.is_negative()).unwrap_or(false);
            let matched = if !verbose && is_negative { None } else { matched };
            let show = !quiet && (matched.is_some() || show_non_matching);
            if show {
                if verbose {
                    match matched {
                        Some(p) => {
                            let bang = if p.flags.is_negative() { "!" } else { "" };
                            let slash = if p.flags.is_must_be_dir() { "/" } else { "" };
                            if nul_term {
                                let _ = write!(
                                    out,
                                    "{source}\0{srcpos}\0{bang}{pat}{slash}\0{path}\0",
                                    source = p.source,
                                    srcpos = p.srcpos,
                                    pat = p.pattern,
                                );
                            } else {
                                let _ = write!(
                                    out,
                                    "{source}:{srcpos}:{bang}{pat}{slash}\t{path}\n",
                                    source = p.source,
                                    srcpos = p.srcpos,
                                    pat = p.pattern,
                                );
                            }
                        }
                        None => {
                            if nul_term {
                                let _ = write!(out, "\0\0\0{path}\0");
                            } else {
                                let _ = write!(out, "::\t{path}\n");
                            }
                        }
                    }
                } else if nul_term {
                    let _ = write!(out, "{path}\0");
                } else {
                    let _ = write!(out, "{path}\n");
                }
            }
            if matched.is_some() {
                num_ignored.set(num_ignored.get() + 1);
            }
        };

        if stdin_paths {
            let mut buf = String::new();
            let mut stdin_reader = std::io::stdin().lock();
            loop {
                buf.clear();
                match stdin_reader.read_line(&mut buf) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
                let path = if nul_term {
                    buf.trim_end_matches('\0').to_string()
                } else {
                    buf.trim_end_matches(&['\r', '\n'][..]).to_string()
                };
                if !path.is_empty() {
                    check_one(&path, out);
                }
            }
        } else {
            for path in &paths {
                check_one(path, out);
            }
        }

        let count = num_ignored.get();
        if count == 0 {
            Err(CommandError::silent(1))
        } else {
            Ok(())
        }
    }
}
