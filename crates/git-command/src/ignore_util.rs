//! Shared construction of the worktree ignore engine used by `check-ignore`
//! and `add` (`.gitignore` files, `.git/info/exclude`, `core.excludesFile`).

use std::path::PathBuf;

use git_attributes::ignore::{parse_gitignore, IgnoreEngine, PatternList};
use git_core::Repository;

/// Load `$GIT_DIR/info/exclude` into a pattern list.
pub(crate) fn info_exclude(repo: &Repository) -> PatternList {
    let path = repo.common_dir.join("info").join("exclude");
    let path_str = path.to_string_lossy().into_owned();
    match std::fs::read_to_string(&path) {
        Ok(content) => parse_gitignore(&content, "", &path_str, 0),
        Err(_) => PatternList { patterns: Vec::new(), src: path_str },
    }
}

/// Load `core.excludesFile` (if configured) into a pattern list.
pub(crate) fn global_excludes(repo: &Repository) -> PatternList {
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
/// scoped to its directory.
pub(crate) fn collect_gitignores(repo: &Repository) -> Vec<PatternList> {
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
            let base = dir.strip_prefix(&work_tree).unwrap_or(&dir).to_string_lossy().into_owned();
            let src = gitignore.strip_prefix(&cwd).unwrap_or(&gitignore).to_string_lossy().into_owned();
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

/// Build a fully-populated ignore engine for the repository's worktree.
pub(crate) fn build_engine(repo: &Repository) -> IgnoreEngine {
    let mut engine = IgnoreEngine::new();
    for pl in collect_gitignores(repo) {
        engine.add_dir_patterns(pl);
    }
    engine.add_dir_patterns(info_exclude(repo));
    let glob = global_excludes(repo);
    if !glob.patterns.is_empty() {
        engine.add_global_patterns(glob);
    }
    engine
}
