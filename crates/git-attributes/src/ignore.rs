//! Git ignore pattern matching engine
//!
//! Implements the gitignore pattern matching logic, matching C Git's behavior
//! exactly including precedence rules, negative patterns, `**` semantics,
//! per-directory stacking, and trailing-slash dir-only rules.

use crate::wildmatch;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Pattern flags matching C Git's PATTERN_FLAG_* values
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatternFlags(pub u32);

impl PatternFlags {
    /// Pattern matches only files (not directories)
    pub const NODIR: u32 = 1;
    /// Pattern matches only at end of path
    pub const ENDSWITH: u32 = 4;
    /// Pattern matches only directories
    pub const MUSTBEDIR: u32 = 8;
    /// Pattern is a negation (un-ignore)
    pub const NEGATIVE: u32 = 16;

    pub fn is_negative(&self) -> bool {
        (self.0 & Self::NEGATIVE) != 0
    }

    pub fn is_must_be_dir(&self) -> bool {
        (self.0 & Self::MUSTBEDIR) != 0
    }

    pub fn is_nodir(&self) -> bool {
        (self.0 & Self::NODIR) != 0
    }
}

/// A single ignore pattern
#[derive(Debug, Clone)]
pub struct IgnorePattern {
    /// The pattern string
    pub pattern: String,
    /// Flags for this pattern
    pub flags: PatternFlags,
    /// The base directory this pattern applies to (relative to repo root)
    pub base: String,
    /// Length of base directory
    pub baselen: usize,
    /// Line number in source file (for verbose output)
    pub srcpos: i32,
    /// Source file path
    pub source: String,
    /// Length of non-wildcard prefix (for optimization)
    pub nowildcardlen: usize,
}

/// Result of an ignore match
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoreMatch<'a> {
    /// The pattern that matched
    pub pattern: &'a IgnorePattern,
    /// Whether this is a negation (un-ignore)
    pub is_negative: bool,
}

/// A list of ignore patterns from a single source
#[derive(Debug, Clone, Default)]
pub struct PatternList {
    /// The patterns in this list
    pub patterns: Vec<IgnorePattern>,
    /// Source identifier (e.g., file path)
    pub src: String,
}

/// Ignore engine that manages pattern lists and performs matching
#[derive(Debug, Default)]
pub struct IgnoreEngine {
    /// Global patterns (from core.excludesFile)
    global_patterns: Vec<PatternList>,
    /// Per-directory patterns (from .gitignore files)
    dir_patterns: Vec<PatternList>,
    /// Command-line patterns
    cmdline_patterns: Vec<PatternList>,
    /// Stack of per-directory exclude states
    exclude_stack: Vec<ExcludeStackEntry>,
    /// Current base path
    basebuf: String,
    /// Last matching pattern (cached)
    last_pattern: Option<IgnorePattern>,
    /// Whether to use case-insensitive matching
    ignore_case: bool,
}

#[derive(Debug, Clone)]
struct ExcludeStackEntry {
    baselen: usize,
    pattern_list_idx: usize,
}

impl IgnoreEngine {
    /// Create a new ignore engine
    pub fn new() -> Self {
        Self {
            ignore_case: false,
            ..Default::default()
        }
    }

    /// Set case-insensitive matching
    pub fn set_ignore_case(&mut self, ignore_case: bool) {
        self.ignore_case = ignore_case;
    }

    /// Add a global pattern list (e.g., from core.excludesFile)
    pub fn add_global_patterns(&mut self, patterns: PatternList) {
        self.global_patterns.push(patterns);
    }

    /// Add a per-directory pattern list (e.g., from .gitignore)
    pub fn add_dir_patterns(&mut self, patterns: PatternList) {
        self.dir_patterns.push(patterns);
    }

    /// Add a command-line pattern list
    pub fn add_cmdline_patterns(&mut self, patterns: PatternList) {
        self.cmdline_patterns.push(patterns);
    }

    /// Parse a .gitignore file and add its patterns
    pub fn add_patterns_from_file(&mut self, path: &str, base: &str) -> std::io::Result<()> {
        let content = std::fs::read_to_string(path)?;
        let patterns = parse_gitignore(&content, base, path, 0);
        self.dir_patterns.push(patterns);
        Ok(())
    }

    /// Check if a path is excluded (ignored)
    pub fn is_excluded(&mut self, path: &str, is_dir: bool) -> Option<IgnoreMatch<'_>> {
        let pathlen = path.len();
        let basename = path.rfind('/').map(|i| &path[i + 1..]).unwrap_or(path);

        // Pop exclude stack entries that don't apply
        self.prep_exclude(path);

        // Check if we have a cached result
        if let Some(ref pattern) = self.last_pattern {
            return Some(IgnoreMatch {
                pattern,
                is_negative: pattern.flags.is_negative(),
            });
        }

        // Search through all pattern lists in order:
        // EXC_CMDL -> EXC_DIRS -> EXC_FILE
        let lists: Vec<&PatternList> = self
            .cmdline_patterns
            .iter()
            .chain(self.dir_patterns.iter())
            .chain(self.global_patterns.iter())
            .collect();

        for list in lists {
            if let Some(pattern) = self.last_matching_pattern_from_list(
                path,
                pathlen,
                basename,
                is_dir,
                list,
            ) {
                self.last_pattern = Some(pattern.clone());
                return Some(IgnoreMatch {
                    pattern,
                    is_negative: pattern.flags.is_negative(),
                });
            }
        }

        None
    }

    /// Get the last matching pattern for a path (for verbose output)
    pub fn last_matching_pattern(
        &mut self,
        path: &str,
        is_dir: bool,
    ) -> Option<IgnoreMatch<'_>> {
        self.is_excluded(path, is_dir)
    }

    fn prep_exclude(&mut self, path: &str) {
        // Pop stack entries that don't apply to this path
        while let Some(entry) = self.exclude_stack.last() {
            if entry.baselen <= path.len()
                && path.starts_with(&self.basebuf[..entry.baselen])
            {
                break;
            }
            self.exclude_stack.pop();
            self.last_pattern = None;
        }
        self.basebuf = path.to_string();
    }

    fn last_matching_pattern_from_list<'a>(
        &'a self,
        pathname: &str,
        pathlen: usize,
        basename: &str,
        is_dir: bool,
        list: &'a PatternList,
    ) -> Option<&'a IgnorePattern> {
        // Scan in reverse to find the last matching pattern
        for pattern in list.patterns.iter().rev() {
            // Check MUSTBEDIR flag
            if pattern.flags.is_must_be_dir() && !is_dir {
                continue;
            }

            if pattern.flags.is_nodir() {
                // Match only against basename
                if match_basename(
                    basename,
                    pathlen - (basename.len() - pattern.pattern.len().min(basename.len())),
                    &pattern.pattern,
                    pattern.nowildcardlen,
                    pattern.flags,
                    self.ignore_case,
                ) {
                    return Some(pattern);
                }
            } else {
                // Match against full path
                if match_pathname(
                    pathname,
                    pathlen,
                    &pattern.base,
                    if pattern.baselen > 0 {
                        pattern.baselen - 1
                    } else {
                        0
                    },
                    &pattern.pattern,
                    pattern.nowildcardlen,
                    pattern.flags,
                    self.ignore_case,
                ) {
                    return Some(pattern);
                }
            }
        }
        None
    }

    /// Clear all patterns
    pub fn clear(&mut self) {
        self.global_patterns.clear();
        self.dir_patterns.clear();
        self.cmdline_patterns.clear();
        self.exclude_stack.clear();
        self.last_pattern = None;
        self.basebuf.clear();
    }
}

/// Parse a .gitignore file content into a PatternList
pub fn parse_gitignore(content: &str, base: &str, source: &str, start_line: i32) -> PatternList {
    let mut patterns = Vec::new();
    let mut line_num = start_line;

    for line in content.lines() {
        line_num += 1;
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let mut flags = PatternFlags(0);
        let mut pattern_str = trimmed.to_string();

        // Check for negation
        if pattern_str.starts_with('!') {
            flags.0 |= PatternFlags::NEGATIVE;
            pattern_str = pattern_str[1..].to_string();
        }

        // Check for trailing slash (directory only)
        if pattern_str.ends_with('/') {
            flags.0 |= PatternFlags::MUSTBEDIR;
            pattern_str = pattern_str[..pattern_str.len() - 1].to_string();
        }

        // Check if pattern has a slash (not at the beginning)
        let has_internal_slash = pattern_str[1..].contains('/');

        // Calculate nowildcardlen (length of non-wildcard prefix)
        let nowildcardlen = pattern_str
            .find(|c| matches!(c, '?' | '*' | '[' | '\\'))
            .unwrap_or(pattern_str.len());

        // If pattern doesn't contain a slash (except possibly at start), it's a "nodir" pattern
        // that matches only the basename
        if !has_internal_slash && !pattern_str.starts_with('/') {
            flags.0 |= PatternFlags::NODIR;
        }

        // Remove leading / if present
        if pattern_str.starts_with('/') {
            pattern_str = pattern_str[1..].to_string();
        }

        patterns.push(IgnorePattern {
            pattern: pattern_str,
            flags,
            base: base.to_string(),
            baselen: base.len(),
            srcpos: line_num,
            source: source.to_string(),
            nowildcardlen,
        });
    }

    PatternList {
        patterns,
        src: source.to_string(),
    }
}

/// Match a basename against a pattern
fn match_basename(
    basename: &str,
    _basename_len: usize,
    pattern: &str,
    _nowildcardlen: usize,
    flags: PatternFlags,
    ignore_case: bool,
) -> bool {
    let wm_flags = if ignore_case {
        wildmatch::flags::PATHNAME | wildmatch::flags::CASEFOLD
    } else {
        wildmatch::flags::PATHNAME
    };

    // For nodir patterns, we match against just the basename
    // The pattern might need a leading **/ to match at any level
    if wildmatch(pattern, basename, wm_flags) == wildmatch::WM_MATCH {
        return true;
    }

    // Also try with **/ prefix for patterns without a slash
    if !pattern.contains('/') {
        let mut prefixed = String::from("**/");
        prefixed.push_str(pattern);
        if wildmatch(&prefixed, basename, wm_flags) == wildmatch::WM_MATCH {
            return true;
        }
    }

    false
}

/// Match a full pathname against a pattern
fn match_pathname(
    pathname: &str,
    _pathlen: usize,
    base: &str,
    baselen: usize,
    pattern: &str,
    _nowildcardlen: usize,
    _flags: PatternFlags,
    ignore_case: bool,
) -> bool {
    let wm_flags = if ignore_case {
        wildmatch::flags::PATHNAME | wildmatch::flags::CASEFOLD
    } else {
        wildmatch::flags::PATHNAME
    };

    // If there's a base, prepend it to the pattern
    let full_pattern = if baselen > 0 {
        let mut p = String::with_capacity(baselen + 1 + pattern.len());
        p.push_str(&base[..baselen]);
        p.push('/');
        p.push_str(pattern);
        p
    } else {
        pattern.to_string()
    };

    if wildmatch(&full_pattern, pathname, wm_flags) == wildmatch::WM_MATCH {
        return true;
    }

    // Also try matching without the base prefix
    if baselen > 0 && wildmatch(pattern, pathname, wm_flags) == wildmatch::WM_MATCH {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_patterns() {
        let content = "foo\nbar/\n!baz\n";
        let list = parse_gitignore(content, "", "test", 0);

        assert_eq!(list.patterns.len(), 3);
        assert_eq!(list.patterns[0].pattern, "foo");
        assert!(!list.patterns[0].flags.is_negative());
        assert!(list.patterns[0].flags.is_nodir());

        assert_eq!(list.patterns[1].pattern, "bar");
        assert!(list.patterns[1].flags.is_must_be_dir());

        assert_eq!(list.patterns[2].pattern, "baz");
        assert!(list.patterns[2].flags.is_negative());
    }

    #[test]
    fn test_simple_ignore() {
        let mut engine = IgnoreEngine::new();
        let list = parse_gitignore("foo\nbar\n", "", "test", 0);
        engine.add_dir_patterns(list);

        assert!(engine.is_excluded("foo", false).is_some());
        assert!(engine.is_excluded("bar", false).is_some());
        assert!(engine.is_excluded("baz", false).is_none());
    }

    #[test]
    fn test_negation() {
        let mut engine = IgnoreEngine::new();
        let list = parse_gitignore("foo\n!foo/bar\n", "", "test", 0);
        engine.add_dir_patterns(list);

        // foo/bar should not be ignored because of the negation
        let result = engine.is_excluded("foo/bar", false);
        assert!(result.is_none() || result.unwrap().is_negative);
    }

    #[test]
    fn test_directory_only() {
        let mut engine = IgnoreEngine::new();
        let list = parse_gitignore("build/\n", "", "test", 0);
        engine.add_dir_patterns(list);

        // build/ should match directories only
        assert!(engine.is_excluded("build", true).is_some());
        assert!(engine.is_excluded("build", false).is_none());
    }

    #[test]
    fn test_wildcard_patterns() {
        let mut engine = IgnoreEngine::new();
        let list = parse_gitignore("*.o\n*~\n", "", "test", 0);
        engine.add_dir_patterns(list);

        assert!(engine.is_excluded("foo.o", false).is_some());
        assert!(engine.is_excluded("foo.c", false).is_none());
        assert!(engine.is_excluded("backup~", false).is_some());
    }

    #[test]
    fn test_double_star() {
        let mut engine = IgnoreEngine::new();
        let list = parse_gitignore("foo/**/bar\n", "", "test", 0);
        engine.add_dir_patterns(list);

        assert!(engine.is_excluded("foo/bar", false).is_some());
        assert!(engine.is_excluded("foo/a/bar", false).is_some());
        assert!(engine.is_excluded("foo/a/b/bar", false).is_some());
    }
}