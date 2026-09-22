//! Path filtering for Git commands (FR-011): magic, globs, exclusions, and
//! NUL-separated input — the sole glob authority. Per-command ad-hoc glob
//! code must not exist; every path-taking command consumes this component.
//!
//! Supported magic (matching C git): `!` (exclusion), `/` (rooted),
//! `:(icase)`, `:(glob)`, `:(literal)`, `:(attr:...)` (parsed, attribute
//! lookup itself stays in `git-attributes`). Anything else under `:()` is
//! rejected explicitly rather than silently misread.

use std::error::Error;
use std::fmt;

/// Errors from pathspec parsing/matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathspecError {
    /// Unknown `:(...)` magic.
    UnknownMagic(String),
    /// Malformed pattern (e.g. unterminated magic).
    Malformed(String),
}

impl fmt::Display for PathspecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathspecError::UnknownMagic(m) => write!(f, "unknown pathspec magic: {m}"),
            PathspecError::Malformed(p) => write!(f, "malformed pathspec: {p}"),
        }
    }
}

impl Error for PathspecError {}

/// Case sensitivity for one pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseMode {
    Sensitive,
    Insensitive,
}

/// One compiled pathspec pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    /// Glob body after magic is stripped.
    pub glob: String,
    /// `!` exclusion (negates the match).
    pub exclude: bool,
    /// Anchored to the prefix root (`/` or `:/`).
    pub anchored: bool,
    /// Directory-only (`trailing /`).
    pub dir_only: bool,
    pub case: CaseMode,
    /// Literal match: no glob metacharacters interpreted.
    pub literal: bool,
}

impl Pattern {
    /// Parse one pathspec item (magic + glob). Never panics.
    pub fn parse(raw: &str) -> Result<Pattern, PathspecError> {
        let mut rest = raw;
        let mut exclude = false;
        let mut case = CaseMode::Sensitive;
        let mut literal = false;
        let mut anchored = false;

        if let Some(stripped) = rest.strip_prefix('!') {
            exclude = true;
            rest = stripped;
        }
        // `:(magic,...)` long form.
        while let Some(stripped) = rest.strip_prefix(":(") {
            let end = stripped.find(')').ok_or_else(|| PathspecError::Malformed(raw.to_string()))?;
            for magic in stripped[..end].split(',') {
                match magic.trim().to_ascii_lowercase().as_str() {
                    "icase" => case = CaseMode::Insensitive,
                    "glob" => literal = false,
                    "literal" => literal = true,
                    "top" | "" => anchored = true,
                    m if m.starts_with("attr:") => {}
                    other => return Err(PathspecError::UnknownMagic(other.to_string())),
                }
            }
            rest = &stripped[end + 1..];
        }
        // `:/` short form (rooted).
        if let Some(stripped) = rest.strip_prefix(":/") {
            anchored = true;
            rest = stripped;
        } else if let Some(stripped) = rest.strip_prefix('/') {
            // Leading `/` anchors and is then dropped (C git strips it).
            anchored = true;
            rest = stripped;
        }
        let mut glob = rest.to_string();
        let mut dir_only = false;
        if glob.ends_with('/') && glob.len() > 1 {
            dir_only = true;
            glob.pop();
        }
        Ok(Pattern { glob, exclude, anchored, dir_only, case, literal })
    }

    /// Test one repository-relative path (forward slashes) against this
    /// pattern. `is_dir` gates directory-only patterns.
    pub fn matches(&self, path: &str, is_dir: bool) -> bool {
        if self.dir_only && !is_dir {
            return false;
        }
        let (pat, text) = match self.case {
            CaseMode::Sensitive => (self.glob.clone(), path.to_string()),
            CaseMode::Insensitive => (self.glob.to_lowercase(), path.to_lowercase()),
        };
        if self.literal {
            return if self.anchored { text == pat } else { text == pat || text.ends_with(&format!("/{pat}")) };
        }
        match_path(&pat, &text, self.anchored)
    }
}

/// Glob match with `*` (no slash crossing), `?`, and `**` (slash crossing),
/// like C git's wildmatch subset used for pathspecs.
pub fn match_path(pattern: &str, text: &str, anchored: bool) -> bool {
    if anchored {
        return wildmatch(pattern.as_bytes(), text.as_bytes());
    }
    // Unanchored: match against the full path and every trailing suffix.
    if wildmatch(pattern.as_bytes(), text.as_bytes()) {
        return true;
    }
    for (i, b) in text.bytes().enumerate() {
        if b == b'/' && wildmatch(pattern.as_bytes(), &text.as_bytes()[i + 1..]) {
            return true;
        }
    }
    false
}

fn wildmatch(pat: &[u8], text: &[u8]) -> bool {
    if pat.is_empty() {
        return text.is_empty();
    }
    match pat[0] {
        b'*' => {
            // `**/` crosses directories; lone `*` does not cross `/`.
            if pat.get(1) == Some(&b'*') {
                let mut rest = &pat[2..];
                if rest.first() == Some(&b'/') {
                    rest = &rest[1..];
                }
                // `**` matches zero or more path components.
                let mut i = 0;
                loop {
                    if wildmatch(rest, &text[i..]) {
                        return true;
                    }
                    match text[i..].iter().position(|&b| b == b'/') {
                        Some(j) => i += j + 1,
                        None => return wildmatch(rest, b""),
                    }
                    if i > text.len() {
                        return false;
                    }
                }
            }
            let mut i = 0;
            loop {
                if wildmatch(&pat[1..], &text[i..]) {
                    return true;
                }
                if i >= text.len() || text[i] == b'/' {
                    return false;
                }
                i += 1;
            }
        }
        b'?' => {
            if text.is_empty() || text[0] == b'/' {
                return false;
            }
            wildmatch(&pat[1..], &text[1..])
        }
        b'\\' if pat.len() > 1 => {
            if text.is_empty() || text[0] != pat[1] {
                return false;
            }
            wildmatch(&pat[2..], &text[1..])
        }
        c => {
            if text.is_empty() || text[0] != c {
                return false;
            }
            wildmatch(&pat[1..], &text[1..])
        }
    }
}

/// A compiled pathspec set: ordered patterns with exclusion semantics (later
/// patterns override — an excluded path re-included by a later positive
/// pattern matches, like C git).
#[derive(Debug, Clone, Default)]
pub struct Pathspec {
    patterns: Vec<Pattern>,
}

impl Pathspec {
    /// Compile many items; any malformed item fails the whole set.
    pub fn compile(items: &[&str]) -> Result<Pathspec, PathspecError> {
        let mut patterns = Vec::with_capacity(items.len());
        for item in items {
            patterns.push(Pattern::parse(item)?);
        }
        Ok(Pathspec { patterns })
    }

    /// Split NUL-separated input (e.g. `--pathspec-from-file --pathspec-file-nul`).
    pub fn split_nul(input: &[u8]) -> Vec<String> {
        input
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect()
    }

    /// True when `path` is selected (last matching pattern decides; default
    /// selected when no pattern matches, mirroring "no pathspec = everything").
    pub fn matches(&self, path: &str, is_dir: bool) -> bool {
        if self.patterns.is_empty() {
            return true;
        }
        let mut selected = false;
        for p in &self.patterns {
            if p.matches(path, is_dir) {
                selected = !p.exclude;
            }
        }
        selected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_basics() {
        let p = Pattern::parse("*.rs").unwrap();
        assert!(p.matches("src/main.rs", false));
        assert!(!p.matches("src/main.txt", false));
        // `*` does not cross `/` when anchored.
        let a = Pattern::parse("/*.rs").unwrap();
        assert!(!a.matches("src/main.rs", false));
        assert!(a.matches("main.rs", false));
    }

    #[test]
    fn doublestar_crosses() {
        let p = Pattern::parse("**/generated/**").unwrap();
        assert!(p.matches("a/b/generated/c/out.txt", false));
        assert!(!p.matches("a/b/clean/c/out.txt", false));
    }

    #[test]
    fn exclusions_override_in_order() {
        let ps = Pathspec::compile(&["*.txt", "!secret.txt", "secret.txt"]).unwrap();
        assert!(ps.matches("a.txt", false));
        assert!(ps.matches("secret.txt", false));
        let ps = Pathspec::compile(&["*.txt", "!secret.txt"]).unwrap();
        assert!(!ps.matches("secret.txt", false));
    }

    #[test]
    fn magic_icase_literal_top() {
        let p = Pattern::parse(":(icase)README.*").unwrap();
        assert!(p.matches("readme.md", false));
        let p = Pattern::parse(":(literal)a*b").unwrap();
        assert!(!p.matches("axxb", false));
        assert!(p.matches("a*b", false));
        let p = Pattern::parse(":/build").unwrap();
        assert!(p.matches("build", true));
        assert!(!p.matches("src/build", true));
    }

    #[test]
    fn rejects_unknown_magic_and_bad_syntax() {
        assert!(matches!(Pattern::parse(":(frobnicate)x"), Err(PathspecError::UnknownMagic(_))));
        assert!(matches!(Pattern::parse(":("), Err(PathspecError::Malformed(_))));
    }

    #[test]
    fn splits_nul_input() {
        assert_eq!(Pathspec::split_nul(b"a\0b\0\0c\0"), vec!["a", "b", "c"]);
    }

    #[test]
    fn empty_set_selects_everything() {
        assert!(Pathspec::default().matches("anything/at/all", false));
    }
}

#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Matching never panics on arbitrary patterns and paths.
        #[test]
        fn match_never_panics(pat in ".*", path in ".*") {
            if let Ok(p) = Pattern::parse(&pat) {
                let _ = p.matches(&path, false);
                let _ = p.matches(&path, true);
            }
        }

        /// Literal anchored patterns agree with plain equality
        /// (dir-only patterns excluded: they only match directories).
        #[test]
        fn literal_means_equal(text in "[a-z/]{0,24}") {
            // Repository-relative paths never start with `/`, and trailing
            // `/` marks dir-only patterns (which need is_dir=true).
            prop_assume!(!text.ends_with('/') && !text.starts_with('/'));
            let p = Pattern::parse(&format!(":(literal){text}")).unwrap();
            prop_assert!(p.matches(&text, false));
            let longer = format!("{text}x");
            prop_assert!(!p.matches(&longer, false));
        }
    }
}
