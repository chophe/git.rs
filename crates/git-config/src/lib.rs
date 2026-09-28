//! Git configuration parsing.
//!
//! A git-compatible subset of `config.c`. The parser handles section headers
//! (with subsections), `key = value` entries, multi-line values, quotes and
//! escapes, inline comments, and `[include]` resolution relative to the
//! including file.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

/// A single configuration entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEntry {
    pub section: String,
    pub subsection: Option<String>,
    pub key: String,
    pub value: String,
    /// The file this entry came from, if any.
    pub origin: Option<PathBuf>,
}

impl ConfigEntry {
    /// The fully qualified name, e.g. `core.filemode` or `remote "origin".url`.
    pub fn name(&self) -> String {
        match &self.subsection {
            Some(sub) => format!("{}.{}.{}", self.section, quote_subsection(sub), self.key),
            None => format!("{}.{}", self.section, self.key),
        }
    }
}

fn quote_subsection(sub: &str) -> String {
    format!("\"{}\"", sub.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Errors returned while parsing configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Io(String),
    IncludeCycle(PathBuf),
    UnterminatedQuote,
    /// A malformed line (e.g. a section header without a closing bracket),
    /// mirroring C git's `fatal: bad config line N [in file F]`.
    BadLine { line: usize, file: Option<PathBuf> },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "could not read config: {e}"),
            ConfigError::IncludeCycle(p) => write!(f, "include cycle detected: {}", p.display()),
            ConfigError::UnterminatedQuote => write!(f, "unterminated quote in config value"),
            ConfigError::BadLine { line, file } => {
                write!(f, "bad config line {line}")?;
                if let Some(p) = file {
                    write!(f, " in file {}", p.display())?;
                }
                Ok(())
            }
        }
    }
}

impl Error for ConfigError {}

impl ConfigError {
    /// Attach the source file to a [`ConfigError::BadLine`] that was parsed
    /// without origin context (e.g. repository config via [`ConfigSet::parse`]).
    pub fn with_file(self, file: PathBuf) -> ConfigError {
        match self {
            ConfigError::BadLine { line, file: None } => ConfigError::BadLine { line, file: Some(file) },
            other => other,
        }
    }
}

/// An ordered set of configuration entries (last occurrence wins on lookup).
#[derive(Debug, Clone, Default)]
pub struct ConfigSet {
    entries: Vec<ConfigEntry>,
}

impl ConfigSet {
    pub fn new() -> ConfigSet {
        ConfigSet::default()
    }

    /// Parse configuration bytes (no include resolution).
    pub fn parse(data: &[u8]) -> Result<ConfigSet, ConfigError> {
        let mut set = ConfigSet::new();
        set.parse_into(data, None)?;
        Ok(set)
    }

    /// Parse configuration from a file, resolving `[include] path` entries
    /// relative to the file's directory.
    pub fn from_file(path: &Path) -> Result<ConfigSet, ConfigError> {
        let mut set = ConfigSet::new();
        let mut seen = Vec::new();
        set.load_file(path, &mut seen)?;
        Ok(set)
    }

    fn load_file(&mut self, path: &Path, seen: &mut Vec<PathBuf>) -> Result<(), ConfigError> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if seen.contains(&canonical) {
            return Err(ConfigError::IncludeCycle(canonical));
        }
        seen.push(canonical);
        let data = std::fs::read(path).map_err(|e| ConfigError::Io(e.to_string()))?;
        let origin = Some(path.to_path_buf());
        let includes = self.parse_into(&data, origin)?;
        for inc in includes {
            self.load_file(&inc, seen)?;
        }
        Ok(())
    }

    fn parse_into(&mut self, data: &[u8], origin: Option<PathBuf>) -> Result<Vec<PathBuf>, ConfigError> {
        let text = std::str::from_utf8(data).unwrap_or("").to_string();
        let start = self.entries.len();
        let mut section: String = String::new();
        let mut subsection: Option<String> = None;
        let mut last_value_index: Option<usize> = None;
        let mut continuation = false;
        let mut includes = Vec::new();

        for (n, line) in text.lines().enumerate() {
            let line_no = n + 1;
            let line = line.trim_end_matches('\r');

            // A value continues onto the next line only when the previous
            // value line ended with an (unescaped) backslash. The continuation
            // line's content is appended verbatim, matching git.
            if continuation {
                if line.trim().is_empty() {
                    continuation = false;
                    continue;
                }
                if let Some(i) = last_value_index {
                    let entry = self.entries.get_mut(i).expect("continuation target");
                    let (text, cont) = strip_continuation(line);
                    entry.value.push_str(&text);
                    continuation = cont;
                }
                continue;
            }

            let trimmed = line.trim_start();

            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }
            if trimmed.starts_with('[') {
                // Section header; a missing closing bracket is fatal, like C
                // git (`fatal: bad config line N`), not silently skipped.
                let end = match trimmed.find(']') {
                    Some(e) => e,
                    None => {
                        return Err(ConfigError::BadLine { line: line_no, file: origin.clone() });
                    }
                };
                let inner = trimmed[1..end].trim();
                let (s, sub) = split_section(inner);
                section = s;
                subsection = sub;
                continue;
            }

            // key [=] value
            let (key, raw_value) = split_key_value(trimmed);
            let (stripped_value, cont) = strip_continuation(raw_value.trim());
            let value = unquote_value(&stripped_value)?;
            self.entries.push(ConfigEntry {
                section: section.clone(),
                subsection: subsection.clone(),
                key,
                value,
                origin: origin.clone(),
            });
            last_value_index = Some(self.entries.len() - 1);
            continuation = cont;
        }

        // Collect `[include] path` entries from this file to resolve after it.
        for entry in &self.entries[start..] {
            if entry.section == "include" && entry.key == "path" {
                if let Some(base) = &entry.origin {
                    let p = expand_path(&entry.value, base.parent());
                    includes.push(p);
                }
            }
        }
        Ok(includes)
    }

    /// The last value for `section.key`, if any.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.get_in(section, None, key)
    }

    /// The last value for `section.subsection.key`, if any.
    pub fn get_in(&self, section: &str, subsection: Option<&str>, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find(|e| {
                e.section == section && e.subsection.as_deref() == subsection && e.key == key
            })
            .map(|e| e.value.as_str())
    }

    /// All values for `section.key` in file order.
    pub fn get_all(&self, section: &str, key: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| e.section == section && e.subsection.is_none() && e.key == key)
            .map(|e| e.value.as_str())
            .collect()
    }

    /// The last value for `section.key` parsed as a bool.
    pub fn get_bool(&self, section: &str, key: &str) -> Option<bool> {
        self.get(section, key).and_then(parse_bool)
    }

    /// All entries (used by `--list` style outputs).
    pub fn entries(&self) -> &[ConfigEntry] {
        &self.entries
    }

    /// Append another set's entries (later files win on lookup).
    pub fn append(&mut self, other: ConfigSet) {
        self.entries.extend(other.entries);
    }

    /// Set a `section.key` value, appending (so it wins on lookup).
    pub fn set(&mut self, section: &str, key: &str, value: &str) {
        self.set_in(section, None, key, value);
    }

    /// Set a `section.subsection.key` value, appending (so it wins on lookup).
    pub fn set_in(
        &mut self,
        section: &str,
        subsection: Option<&str>,
        key: &str,
        value: &str,
    ) {
        self.entries.push(ConfigEntry {
            section: section.to_ascii_lowercase(),
            subsection: subsection.map(|s| s.to_string()),
            key: key.to_ascii_lowercase(),
            value: value.to_string(),
            origin: None,
        });
    }

    /// Apply a `git -c name=value` command-line override.
    ///
    /// `name` may contain a subsection: `remote.origin.url`. A missing `=`
    /// means a boolean `true` (matching C git).
    pub fn set_cli(&mut self, name: &str, value: Option<&str>) {
        let value = value.unwrap_or("true");
        // The first dot separates section from the rest; the last dot in the
        // remainder separates subsection from key.
        let Some(first_dot) = name.find('.') else {
            return;
        };
        let section = name[..first_dot].to_lowercase();
        let rest = &name[first_dot + 1..];
        let (subsection, key) = match rest.rfind('.') {
            Some(i) => (Some(rest[..i].to_string()), rest[i + 1..].to_lowercase()),
            None => (None, rest.to_lowercase()),
        };
        if section.is_empty() || key.is_empty() {
            return;
        }
        self.set_in(&section, subsection.as_deref(), &key, value);
    }

    /// Load the layered repository scopes in C precedence order
    /// (`do_git_config_sequence` in `config.c`):
    ///
    /// system, XDG user, global user, local (`$COMMONDIR/config`), then the
    /// worktree file (`$GIT_DIR/config.worktree`, gated — see
    /// [`worktree_config_enabled`]).
    ///
    /// Later scopes win on lookup (via [`ConfigSet::append`]); missing scope
    /// files mean empty, while corrupt ones return [`ConfigError::BadLine`]
    /// (fatal 128 at the surface, like C). Each scope file resolves its own
    /// `[include]` entries; CLI overlays (`GIT_CONFIG_COUNT` pairs, `-c`)
    /// are applied by the caller afterwards so they always win.
    pub fn load_repo_scopes(scopes: &RepoScopes) -> Result<ConfigSet, ConfigError> {
        let mut set = ConfigSet::new();
        if env_allows_system() {
            load_scope_into(&mut set, &system_config_path())?;
        }
        let (user, xdg) = global_config_paths();
        if let Some(x) = xdg {
            load_scope_into(&mut set, &x)?;
        }
        if let Some(u) = user {
            load_scope_into(&mut set, &u)?;
        }
        load_scope_into(&mut set, &scopes.commondir.join("config"))?;
        if worktree_config_enabled(&set) {
            load_scope_into(&mut set, &scopes.git_dir.join("config.worktree"))?;
        }
        Ok(set)
    }
}

/// Which on-disk scopes to layer for one repository.
#[derive(Debug, Clone)]
pub struct RepoScopes {
    /// The `.git` directory (locates `config.worktree`).
    pub git_dir: PathBuf,
    /// The shared directory (locates `config`).
    pub commondir: PathBuf,
    /// Resolved worktree hint for `includeIf "worktree:"` conditions
    /// (explicit `GIT_WORK_TREE` or the default parent directory; `None`
    /// when bare). Evaluated by conditional includes (task 2).
    pub worktree: Option<PathBuf>,
}

fn env_bool(name: &str, def: bool) -> bool {
    match std::env::var(name) {
        Err(_) => def,
        Ok(v) => parse_bool(&v).unwrap_or(def),
    }
}

/// C `git_config_system()`: `GIT_CONFIG_NOSYSTEM` skips the system file.
fn env_allows_system() -> bool {
    !env_bool("GIT_CONFIG_NOSYSTEM", false)
}

/// C `git_system_config()`: `GIT_CONFIG_SYSTEM` overrides the built-in path.
fn system_config_path() -> PathBuf {
    std::env::var_os("GIT_CONFIG_SYSTEM")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/gitconfig"))
}

/// C `git_global_config_paths()`: `GIT_CONFIG_GLOBAL` overrides the user file
/// (and disables the XDG file); otherwise `~/.gitconfig` plus
/// `$XDG_CONFIG_HOME/git/config` (or `~/.config/git/config`).
/// Returns `(user, xdg)` in load order (xdg first, user wins).
fn global_config_paths() -> (Option<PathBuf>, Option<PathBuf>) {
    if let Some(g) = std::env::var_os("GIT_CONFIG_GLOBAL") {
        return (Some(PathBuf::from(g)), None);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let user = home.as_ref().map(|h| h.join(".gitconfig"));
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| home.as_ref().map(|h| h.join(".config")))
        .map(|base| base.join("git/config"));
    (user, xdg)
}

/// Whether `$GIT_DIR/config.worktree` applies.
///
/// Probed against the tree binary (2.55.0.552): the file is read only when
/// `extensions.worktreeConfig` parses true **and** `core.repositoryformatversion`
/// is explicitly present (a config with the flag but no version key ignores
/// the worktree file). `setup.c` handles `worktreeconfig` even for v0, so no
/// version-magnitude check — presence is the gate.
fn worktree_config_enabled(set: &ConfigSet) -> bool {
    set.get("core", "repositoryformatversion").is_some()
        && set.get_bool("extensions", "worktreeconfig") == Some(true)
}

/// Load one scope file: a missing file means empty (C `access_or_die` skip),
/// a corrupt file is fatal (C `die("bad config line ...")`).
fn load_scope_into(set: &mut ConfigSet, path: &Path) -> Result<(), ConfigError> {
    if !path.exists() {
        return Ok(());
    }
    let scope = ConfigSet::from_file(path)?;
    set.append(scope);
    Ok(())
}

/// Split a section header body into section and optional subsection.
fn split_section(inner: &str) -> (String, Option<String>) {
    // Section names are case-insensitive (C git lowercases them);
    // subsection names keep their case.
    match inner.find('"') {
        Some(i) => {
            let section = inner[..i].trim().to_ascii_lowercase();
            let rest = &inner[i + 1..];
            let sub = match rest.find('"') {
                Some(j) => Some(rest[..j].to_string()),
                None => Some(rest.trim().to_string()),
            };
            (section, sub)
        }
        None => (inner.to_ascii_lowercase(), None),
    }
}

/// Detect a value continuation: a single unescaped trailing backslash means the
/// value continues on the next line (git removes the backslash + newline).
fn strip_continuation(value: &str) -> (String, bool) {
    let bytes = value.as_bytes();
    let n = bytes.len();
    if n > 0 && bytes[n - 1] == b'\\' && (n < 2 || bytes[n - 2] != b'\\') {
        (value[..n - 1].to_string(), true)
    } else {
        (value.to_string(), false)
    }
}

/// Split a `key = value` (or `key value`) line, trimming comments.
fn split_key_value(trimmed: &str) -> (String, String) {
    // Key names are case-insensitive (C git lowercases them).
    let (key, value) = match trimmed.find('=') {
        Some(i) => (trimmed[..i].trim(), trimmed[i + 1..].trim()),
        None => match trimmed.split_once(char::is_whitespace) {
            Some((k, v)) => (k.trim(), v.trim()),
            None => (trimmed.trim(), ""),
        },
    };
    (key.to_ascii_lowercase(), strip_inline_comment(value).to_string())
}

/// Strip a trailing `#`/`;` comment that follows whitespace and is outside quotes.
fn strip_inline_comment(value: &str) -> &str {
    let bytes = value.as_bytes();
    let mut in_quotes = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => in_quotes = !in_quotes,
            b'#' | b';' if !in_quotes => {
                // Only a comment if preceded by whitespace or start.
                if i == 0 || bytes[i - 1].is_ascii_whitespace() {
                    return &value[..i].trim_end();
                }
            }
            _ => {}
        }
        i += 1;
    }
    value
}

/// Remove surrounding quotes and resolve escapes from a config value.
fn unquote_value(value: &str) -> Result<String, ConfigError> {
    if value.is_empty() || value.starts_with('"') == false {
        return Ok(value.to_string());
    }
    if value.len() < 2 {
        return Err(ConfigError::UnterminatedQuote);
    }
    let inner = &value[1..];
    let bytes = inner.as_bytes();
    let mut out = String::with_capacity(inner.len());
    let mut i = 0;
    let mut closed = false;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\\' => {
                if i + 1 >= bytes.len() {
                    return Err(ConfigError::UnterminatedQuote);
                }
                let esc = bytes[i + 1];
                match esc {
                    b'n' => out.push('\n'),
                    b't' => out.push('\t'),
                    b'b' => out.push('\u{0008}'),
                    c => out.push(c as char),
                }
                i += 2;
            }
            b'"' => {
                closed = true;
                break;
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    if !closed {
        return Err(ConfigError::UnterminatedQuote);
    }
    Ok(out)
}

/// Parse a boolean per git's rules (case-insensitive, like C
/// `git_parse_maybe_bool`: `true/yes/on/1` and the empty string are true,
/// `false/no/off/0` are false).
pub fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "" | "yes" | "on" | "true" | "1" => Some(true),
        "no" | "off" | "false" | "0" => Some(false),
        _ => None,
    }
}

/// Expand `~`/`~/...` and `$HOME`/`${HOME}` in a path, resolving relative to
/// `base` otherwise.
fn expand_path(value: &str, base: Option<&Path>) -> PathBuf {
    let expanded = if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            PathBuf::from(home).join(rest)
        } else {
            PathBuf::from(value)
        }
    } else if let Some(rest) = value.strip_prefix('~') {
        if rest.is_empty() {
            if let Some(home) = std::env::var_os("HOME") {
                PathBuf::from(home)
            } else {
                PathBuf::from(value)
            }
        } else {
            PathBuf::from(value)
        }
    } else {
        PathBuf::from(value)
    };

    if expanded.is_absolute() {
        expanded
    } else if let Some(base) = base {
        base.join(expanded)
    } else {
        expanded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_sections_and_keys() {
        let cfg = ConfigSet::parse(b"[core]\n\tfilemode = true\n[user]\n\tname = Alice\n").unwrap();
        assert_eq!(cfg.get("core", "filemode"), Some("true"));
        assert_eq!(cfg.get_bool("core", "filemode"), Some(true));
        assert_eq!(cfg.get("user", "name"), Some("Alice"));
        assert_eq!(cfg.get("user", "email"), None);
    }

    #[test]
    fn parses_subsection() {
        let cfg = ConfigSet::parse(b"[remote \"origin\"]\n\turl = https://example.com/git\n").unwrap();
        assert_eq!(cfg.get_in("remote", Some("origin"), "url"), Some("https://example.com/git"));
        assert_eq!(cfg.get("remote", "url"), None);
        assert_eq!(cfg.entries()[0].name(), "remote.\"origin\".url");
    }

    #[test]
    fn last_wins() {
        let cfg = ConfigSet::parse(b"[core]\na = 1\na = 2\n").unwrap();
        assert_eq!(cfg.get("core", "a"), Some("2"));
        assert_eq!(cfg.get_all("core", "a"), vec!["1", "2"]);
    }

    #[test]
    fn key_without_equals() {
        let cfg = ConfigSet::parse(b"[core]\n\tfilemode true\n").unwrap();
        assert_eq!(cfg.get("core", "filemode"), Some("true"));
    }

    #[test]
    fn inline_and_full_comments() {
        let cfg = ConfigSet::parse(b"[core]\n\t# full line comment\n\t; another\n\tfilemode = true # trailing\n").unwrap();
        assert_eq!(cfg.get("core", "filemode"), Some("true"));
    }

    #[test]
    fn quoted_values_and_escapes() {
        let cfg = ConfigSet::parse(b"[user]\n\tname = \"A\\nB\"\n").unwrap();
        assert_eq!(cfg.get("user", "name"), Some("A\nB"));
    }

    #[test]
    fn continuation_lines() {
        // Continuation is triggered by a trailing backslash, not by leading
        // whitespace. The continuation line's content is appended verbatim.
        let cfg = ConfigSet::parse(b"[user]\n\tname = Alice \\\n\tBob\n").unwrap();
        assert_eq!(cfg.get("user", "name"), Some("Alice \tBob"));
    }

    #[test]
    fn whitespace_line_without_backslash_is_new_key() {
        // A tab-prefixed `key = value` line is a new key, never a continuation.
        let cfg = ConfigSet::parse(b"[user]\n\tname = Alice\n\temail = alice@example.com\n").unwrap();
        assert_eq!(cfg.get("user", "name"), Some("Alice"));
        assert_eq!(cfg.get("user", "email"), Some("alice@example.com"));
    }

    #[test]
    fn bool_values() {
        assert_eq!(parse_bool(""), Some(true));
        assert_eq!(parse_bool("true"), Some(true));
        assert_eq!(parse_bool("yes"), Some(true));
        assert_eq!(parse_bool("on"), Some(true));
        assert_eq!(parse_bool("1"), Some(true));
        assert_eq!(parse_bool("false"), Some(false));
        assert_eq!(parse_bool("no"), Some(false));
        assert_eq!(parse_bool("off"), Some(false));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool("maybe"), None);
    }

    #[test]
    fn include_resolution() {
        let dir = std::env::temp_dir().join(format!("git-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let main = dir.join("config");
        let sub = dir.join("sub.conf");
        std::fs::write(&sub, "[core]\n\tfilemode = false\n").unwrap();
        std::fs::write(
            &main,
            format!("[include]\n\tpath = {}\n[user]\n\tname = Bob\n", sub.display()),
        )
        .unwrap();

        let cfg = ConfigSet::from_file(&main).unwrap();
        assert_eq!(cfg.get_bool("core", "filemode"), Some(false));
        assert_eq!(cfg.get("user", "name"), Some("Bob"));
        let from_sub = cfg
            .entries()
            .iter()
            .find(|e| e.section == "core" && e.key == "filemode")
            .unwrap();
        assert_eq!(from_sub.origin.as_deref(), Some(sub.as_path()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn include_cycle_detected() {
        let dir = std::env::temp_dir().join(format!("git-config-cycle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.conf");
        let b = dir.join("b.conf");
        std::fs::write(&a, format!("[include]\n\tpath = {}\n", b.display())).unwrap();
        std::fs::write(&b, format!("[include]\n\tpath = {}\n", a.display())).unwrap();

        let err = ConfigSet::from_file(&a).unwrap_err();
        assert!(matches!(err, ConfigError::IncludeCycle(_)));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unclosed_section_header_is_fatal_with_line_number() {
        // Mirrors C git: `fatal: bad config line 2 in file ...`.
        let err = ConfigSet::parse(b"[core]\n\tfilemode = true\n[[[oops\n").unwrap_err();
        assert!(
            matches!(err, ConfigError::BadLine { line: 3, file: None }),
            "unexpected error: {err:?}"
        );
        assert_eq!(format!("{err}"), "bad config line 3");
        let with_file = err.with_file(std::path::PathBuf::from(".git/config"));
        assert_eq!(format!("{with_file}"), "bad config line 3 in file .git/config");
    }

    // -- scope loader tests (C `do_git_config_sequence` order) --

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    struct ScopeLayout {
        _guard: std::sync::MutexGuard<'static, ()>,
        dir: PathBuf,
        git_dir: PathBuf,
        common_dir: PathBuf,
        saved: Vec<(String, Option<std::ffi::OsString>)>,
    }

    /// Build an isolated scope tree and point the scope env vars at it, so
    /// tests never read the developer's real `~/.gitconfig` or `/etc/gitconfig`.
    /// Returns the layout; the env guard is held for the test's lifetime.
    /// Saved vars are restored on drop via `ScopeLayout`'s fields.
    fn isolated_scopes(tag: &str) -> ScopeLayout {
        let guard = lock_env();
        let dir = std::env::temp_dir().join(format!(
            "git-config-scope-{}-{}-{}",
            std::process::id(),
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let git_dir = dir.join("repo.git");
        std::fs::create_dir_all(&git_dir).unwrap();
        // Isolate from ambient config: no system file, temp global/system.
        let mut saved = Vec::new();
        for (k, v) in [
            ("GIT_CONFIG_NOSYSTEM", Some("1")),
            ("GIT_CONFIG_GLOBAL", None),
            ("GIT_CONFIG_SYSTEM", None),
            ("HOME", None),
        ] {
            saved.push((k.to_string(), std::env::var_os(k)));
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
        // Point global/system at (initially missing) temp files, HOME at dir.
        std::env::set_var("GIT_CONFIG_GLOBAL", dir.join("global.conf"));
        std::env::set_var("GIT_CONFIG_SYSTEM", dir.join("system.conf"));
        std::env::set_var("HOME", &dir);
        saved.push(("XDG_CONFIG_HOME".to_string(), std::env::var_os("XDG_CONFIG_HOME")));
        std::env::remove_var("XDG_CONFIG_HOME");
        ScopeLayout { _guard: guard, dir, git_dir: git_dir.clone(), common_dir: git_dir, saved }
    }

    impl Drop for ScopeLayout {
        fn drop(&mut self) {
            for (k, v) in self.saved.drain(..) {
                match v {
                    Some(val) => std::env::set_var(&k, val),
                    None => std::env::remove_var(&k),
                }
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    impl ScopeLayout {
        fn scopes(&self) -> super::RepoScopes {
            super::RepoScopes {
                git_dir: self.git_dir.clone(),
                commondir: self.common_dir.clone(),
                worktree: None,
            }
        }
    }

    #[test]
    fn scope_precedence_local_beats_global_beats_system() {
        let layout = isolated_scopes("precedence");
        std::fs::write(layout.dir.join("system.conf"), "[user]\n\tname = sys\n").unwrap();
        std::fs::write(layout.dir.join("global.conf"), "[user]\n\tname = glob\n").unwrap();
        std::fs::write(
            layout.common_dir.join("config"),
            "[user]\n\tname = local\n",
        )
        .unwrap();

        let cfg = ConfigSet::load_repo_scopes(&layout.scopes()).unwrap();
        assert_eq!(cfg.get("user", "name"), Some("local"));

        // A `-c`-style overlay applied afterwards always wins.
        let mut with_cli = cfg;
        with_cli.set_cli("user.name", Some("cli"));
        assert_eq!(with_cli.get("user", "name"), Some("cli"));
    }

    #[test]
    fn scope_falls_back_through_missing_files() {
        let layout = isolated_scopes("fallback");
        std::fs::write(layout.dir.join("global.conf"), "[user]\n\tname = glob\n").unwrap();
        // No system file, no local file: global wins; nothing errors.
        let cfg = ConfigSet::load_repo_scopes(&layout.scopes()).unwrap();
        assert_eq!(cfg.get("user", "name"), Some("glob"));
    }

    #[test]
    fn worktree_file_gated_on_flag_and_version() {
        let layout = isolated_scopes("worktree");
        std::fs::write(
            layout.git_dir.join("config.worktree"),
            "[user]\n\temail = wt@example.com\n",
        )
        .unwrap();

        // No version key: worktree file invisible even with the flag.
        std::fs::write(
            layout.common_dir.join("config"),
            "[user]\n\tname = local\n[extensions]\n\tworktreeConfig = true\n",
        )
        .unwrap();
        let cfg = ConfigSet::load_repo_scopes(&layout.scopes()).unwrap();
        assert_eq!(cfg.get("user", "email"), None);

        // Flag without version... same file minus version covered above.
        // With both: visible (probed C behavior).
        std::fs::write(
            layout.common_dir.join("config"),
            "[core]\n\trepositoryformatversion = 0\n[user]\n\tname = local\n[extensions]\n\tworktreeConfig = true\n",
        )
        .unwrap();
        let cfg = ConfigSet::load_repo_scopes(&layout.scopes()).unwrap();
        assert_eq!(cfg.get("user", "email"), Some("wt@example.com"));

        // Version but flag off: invisible.
        std::fs::write(
            layout.common_dir.join("config"),
            "[core]\n\trepositoryformatversion = 0\n[user]\n\tname = local\n",
        )
        .unwrap();
        let cfg = ConfigSet::load_repo_scopes(&layout.scopes()).unwrap();
        assert_eq!(cfg.get("user", "email"), None);
    }

    #[test]
    fn corrupt_scope_file_is_badline() {
        let layout = isolated_scopes("corrupt");
        std::fs::write(layout.common_dir.join("config"), "[[[oops\n").unwrap();
        let err = ConfigSet::load_repo_scopes(&layout.scopes()).unwrap_err();
        assert!(
            matches!(err, ConfigError::BadLine { line: 1, file: _ }),
            "unexpected error: {err:?}"
        );
        // Renders like C's `fatal: bad config line 1 in file ...` once the
        // surface adds the `fatal: ` prefix.
        assert!(format!("{err}").starts_with("bad config line 1 in file "));
    }

    #[test]
    fn bool_parsing_is_case_insensitive_like_c() {
        assert_eq!(super::parse_bool("TRUE"), Some(true));
        assert_eq!(super::parse_bool("Yes"), Some(true));
        assert_eq!(super::parse_bool("OFF"), Some(false));
        assert_eq!(super::parse_bool("maybe"), None);
    }
}

#[cfg(test)]
mod props {
    use super::ConfigSet;
    use proptest::prelude::*;

    proptest! {
        /// Parsing arbitrary bytes must never panic (it either parses or
        /// returns an error).
        #[test]
        fn parse_never_panics(data: Vec<u8>) {
            let _ = ConfigSet::parse(&data);
        }

        /// A generated well-formed config round-trips through the parser.
        #[test]
        fn round_trips_generated_config(section in "[a-z]{1,8}", key in "[a-z]{1,8}", value in "[a-z0-9]{0,16}") {
            let text = format!("[{section}]\n\t{key} = {value}\n");
            let cfg = ConfigSet::parse(text.as_bytes()).unwrap();
            prop_assert_eq!(cfg.get(&section, &key), Some(value.as_str()));
        }
    }
}
