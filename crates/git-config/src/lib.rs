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
    /// An include chain deeper than [`MAX_INCLUDE_DEPTH`], with C's exact
    /// text (`config.c` `include_depth_advice`): `path` is the file being
    /// included, `from` the file containing the directive. Distinct from
    /// [`ConfigError::IncludeCycle`] (a canonical-path repeat on the current
    /// chain, e.g. a direct two-file cycle); C itself only has the depth
    /// die, but the plan keeps the two errors distinct.
    IncludeDepth { limit: usize, path: PathBuf, from: PathBuf },
    /// A `remote.*.url` entry inside a file reached through an
    /// `includeIf "hasconfig:remote.*.url:..."` edge (C `forbid_remote_url`).
    RemoteUrlForbidden,
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
            ConfigError::IncludeDepth { limit, path, from } => {
                write!(
                    f,
                    "exceeded maximum include depth ({limit}) while including\n\t{}\nfrom\n\t{}\nThis might be due to circular includes.",
                    path.display(),
                    from.display()
                )
            }
            ConfigError::RemoteUrlForbidden => write!(
                f,
                "remote URLs cannot be configured in file directly or indirectly included by includeIf.hasconfig:remote.*.url"
            ),
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

    /// Parse configuration from a file, resolving `[include]` and
    /// `[includeIf]` entries. Repo-dependent conditions (`gitdir:`,
    /// `onbranch:`, `worktree:`) evaluate false without repository context
    /// (see [`from_file_with`]); `hasconfig:remote.*.url:` works standalone.
    pub fn from_file(path: &Path) -> Result<ConfigSet, ConfigError> {
        Self::from_file_with(path, &IncludeContext::default())
    }

    /// Parse configuration from a file with repository context for
    /// conditional includes.
    pub fn from_file_with(path: &Path, ctx: &IncludeContext) -> Result<ConfigSet, ConfigError> {
        load_roots(&[path.to_path_buf()], ctx, true, &[])
    }

    /// Recursive include loader (phase B): appends `path`'s entries, then
    /// follows its include directives in order. `seen` is the current chain
    /// (pushed on entry, popped on exit) so diamonds re-process like C while
    /// true cycles die. Missing files are skipped unless `required` (C
    /// `access_or_die`); `depth` counts include edges from the root.
    fn load_file(
        &mut self,
        path: &Path,
        seen: &mut Vec<PathBuf>,
        depth: usize,
        ctx: &IncludeContext,
        remote_urls: &[String],
        under_hasconfig: bool,
        required: bool,
    ) -> Result<(), ConfigError> {
        if !path.exists() {
            if required {
                return Err(ConfigError::Io(
                    std::io::Error::new(std::io::ErrorKind::NotFound, format!("{}", path.display()))
                        .to_string(),
                ));
            }
            return Ok(());
        }
        let (entries, directives) = read_parsed(path, seen)?;
        if under_hasconfig && has_remote_url(&entries) {
            return Err(ConfigError::RemoteUrlForbidden);
        }
        self.entries.extend(entries);
        for directive in &directives {
            let inc = resolve_include_path(&directive.value, path.parent());
            let is_hasconfig = directive.condition.as_deref().is_some_and(is_hasconfig_condition);
            if let Some(cond) = &directive.condition {
                if !include_condition_true(cond, path.parent(), ctx, remote_urls) {
                    continue;
                }
            }
            if depth + 1 > MAX_INCLUDE_DEPTH {
                return Err(ConfigError::IncludeDepth {
                    limit: MAX_INCLUDE_DEPTH,
                    path: inc,
                    from: path.to_path_buf(),
                });
            }
            self.load_file(
                &inc,
                seen,
                depth + 1,
                ctx,
                remote_urls,
                under_hasconfig || is_hasconfig,
                false,
            )?;
        }
        seen.pop();
        Ok(())
    }

    fn parse_into(&mut self, data: &[u8], origin: Option<PathBuf>) -> Result<Vec<PendingInclude>, ConfigError> {
        let text = std::str::from_utf8(data).unwrap_or("").to_string();
        let start = self.entries.len();
        let mut section: String = String::new();
        let mut subsection: Option<String> = None;
        let mut last_value_index: Option<usize> = None;
        let mut continuation = false;
        let mut includes: Vec<PendingInclude> = Vec::new();

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
                // C also accepts `key = value` on the same line after `]`
                // (used by `t/t1305` conditional-include setups); a trailing
                // comment alone is ignored.
                let rest = trimmed[end + 1..].trim();
                if rest.is_empty() || rest.starts_with('#') || rest.starts_with(';') {
                    continue;
                }
                let (key, raw_value) = split_key_value(rest);
                let (stripped_value, cont) = strip_continuation(raw_value.trim());
                let value = unquote_value(&stripped_value).map_err(|_| ConfigError::BadLine {
                    line: line_no,
                    file: origin.clone(),
                })?;
                self.entries.push(ConfigEntry {
                    section: section.clone(),
                    subsection: subsection.clone(),
                    key,
                    value,
                    origin: origin.clone(),
                });
                last_value_index = Some(self.entries.len() - 1);
                continuation = cont;
                continue;
            }

            // key [=] value
            let (key, raw_value) = split_key_value(trimmed);
            let (stripped_value, cont) = strip_continuation(raw_value.trim());
            // C reports quote errors as `bad config line N`, like any other
            // malformed line (probed on the tree binary).
            let value = unquote_value(&stripped_value).map_err(|_| ConfigError::BadLine {
                line: line_no,
                file: origin.clone(),
            })?;
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

        // Collect `[include] path` and `[includeIf "<cond>"] path` entries
        // from this file to resolve after it, in file order.
        for entry in &self.entries[start..] {
            if entry.key == "path" {
                if entry.section == "include" {
                    includes.push(PendingInclude { condition: None, value: entry.value.clone() });
                } else if entry.section == "includeif" {
                    if let Some(cond) = &entry.subsection {
                        includes.push(PendingInclude {
                            condition: Some(cond.clone()),
                            value: entry.value.clone(),
                        });
                    }
                    // A bare `[includeIf]` without a condition never matches
                    // (C `parse_config_key` yields no condition).
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
        let mut paths = Vec::new();
        if env_allows_system() {
            paths.push(system_config_path());
        }
        let (user, xdg) = global_config_paths();
        if let Some(x) = xdg {
            paths.push(x);
        }
        if let Some(u) = user {
            paths.push(u);
        }
        paths.push(scopes.commondir.join("config"));

        let ctx = IncludeContext {
            git_dir: Some(scopes.git_dir.clone()),
            git_dir_fallback: scopes.git_dir_verbatim.clone(),
            worktree: scopes.worktree.clone(),
            head_branch: resolve_head_branch(&scopes.git_dir),
        };
        let mut set = load_roots(&paths, &ctx, false, &[])?;
        if worktree_config_enabled(&set) {
            // The worktree scope's `hasconfig:` conditions see the main
            // scopes' remotes too (C collects across the whole sequence).
            let wt_path = scopes.git_dir.join("config.worktree");
            let wt = load_roots(std::slice::from_ref(&wt_path), &ctx, false, &paths)?;
            set.append(wt);
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
    /// Unresolved absolute `.git` directory (C retries `gitdir:` matches
    /// against the non-realpath form so symlinked patterns keep working).
    pub git_dir_verbatim: Option<PathBuf>,
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

/// C `MAX_INCLUDE_DEPTH` (`config.c`): include chains deeper than this die
/// instead of recursing forever.
pub const MAX_INCLUDE_DEPTH: usize = 10;

/// Repository context for evaluating conditional includes
/// (`include_condition_is_true` in `config.c`).
#[derive(Debug, Clone, Default)]
pub struct IncludeContext {
    /// Canonical `.git` directory for `gitdir:` conditions.
    pub git_dir: Option<PathBuf>,
    /// Unresolved absolute `.git` directory (C retries the match against the
    /// non-realpath form so symlinked patterns keep working).
    pub git_dir_fallback: Option<PathBuf>,
    /// Worktree root for `worktree:` conditions (`None` in bare repos).
    pub worktree: Option<PathBuf>,
    /// Current branch short name for `onbranch:` (from `HEAD`'s symref target,
    /// even when unborn; `None` when detached or unreadable).
    pub head_branch: Option<String>,
}

/// One `[include] path` / `[includeIf "<cond>"] path` directive.
#[derive(Debug, Clone)]
struct PendingInclude {
    condition: Option<String>,
    value: String,
}

/// Read `path`, check the chain for a canonical-path repeat, and split it
/// into entries plus include directives. Pushes onto `seen`; the caller pops
/// on success (path-based cycle detection, so diamond includes re-process
/// exactly like C).
fn read_parsed(
    path: &Path,
    seen: &mut Vec<PathBuf>,
) -> Result<(Vec<ConfigEntry>, Vec<PendingInclude>), ConfigError> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if seen.contains(&canonical) {
        return Err(ConfigError::IncludeCycle(canonical));
    }
    seen.push(canonical);
    let data = std::fs::read(path).map_err(|e| ConfigError::Io(e.to_string()))?;
    let mut tmp = ConfigSet::new();
    let directives = tmp.parse_into(&data, Some(path.to_path_buf()))?;
    Ok((tmp.entries, directives))
}

fn has_remote_url(entries: &[ConfigEntry]) -> bool {
    entries
        .iter()
        .any(|e| e.section == "remote" && e.key == "url" && e.subsection.is_some())
}

fn is_hasconfig_condition(cond: &str) -> bool {
    cond.starts_with("hasconfig:remote.*.url:")
}

/// Evaluate one `includeIf "<cond>"` condition (C
/// `include_condition_is_true`). Unknown condition keywords are silently
/// false, like C.
fn include_condition_true(
    cond: &str,
    including_dir: Option<&Path>,
    ctx: &IncludeContext,
    remote_urls: &[String],
) -> bool {
    if let Some(pat) = cond.strip_prefix("gitdir/i:") {
        return match_path_condition(pat, including_dir, ctx.git_dir.as_deref(), ctx.git_dir_fallback.as_deref(), true);
    }
    if let Some(pat) = cond.strip_prefix("gitdir:") {
        return match_path_condition(pat, including_dir, ctx.git_dir.as_deref(), ctx.git_dir_fallback.as_deref(), false);
    }
    if let Some(pat) = cond.strip_prefix("worktree/i:") {
        return match_path_condition(pat, including_dir, ctx.worktree.as_deref(), None, true);
    }
    if let Some(pat) = cond.strip_prefix("worktree:") {
        return match_path_condition(pat, including_dir, ctx.worktree.as_deref(), None, false);
    }
    if let Some(pat) = cond.strip_prefix("onbranch:") {
        return match_onbranch(pat, ctx.head_branch.as_deref());
    }
    if let Some(glob) = cond.strip_prefix("hasconfig:remote.*.url:") {
        return remote_urls.iter().any(|url| wildmatch(glob, url, false));
    }
    false
}

/// `gitdir:` / `worktree:` matching (C `include_by_path`): the pattern is
/// prepared (tilde expansion, `./` resolved against the including file,
/// unanchored patterns gain a `**/` prefix, trailing slashes gain `**`) and
/// matched against the real path with `WM_PATHNAME` semantics, retrying
/// against the unresolved absolute form when the first match fails (C's
/// `strbuf_add_absolute_path` second try, for symlinked setups).
fn match_path_condition(
    cond: &str,
    including_dir: Option<&Path>,
    path: Option<&Path>,
    fallback: Option<&Path>,
    icase: bool,
) -> bool {
    let Some(path) = path else {
        return false;
    };
    let pattern = prepare_condition_pattern(cond, including_dir);
    for candidate in path_candidates(path, fallback) {
        if wildmatch(&pattern, &candidate, icase) {
            return true;
        }
    }
    false
}

fn path_candidates(path: &Path, fallback: Option<&Path>) -> Vec<String> {
    let mut out = Vec::new();
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    out.push(canonical.to_string_lossy().into_owned());
    // The unresolved absolute form (may still contain symlinks).
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(cwd) = std::env::current_dir().ok() {
        cwd.join(path)
    } else {
        path.to_path_buf()
    };
    let s = absolute.to_string_lossy().into_owned();
    if !out.contains(&s) {
        out.push(s);
    }
    if let Some(fb) = fallback {
        let s = fb.to_string_lossy().into_owned();
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

/// Prepare a `gitdir:`/`worktree:` condition pattern (C
/// `prepare_include_condition_pattern`): `~` expansion, a leading `./`
/// resolved against the including file's directory (realpath'd), `**/`
/// prepended when not absolute, `**` appended for a trailing slash.
fn prepare_condition_pattern(cond: &str, including_dir: Option<&Path>) -> String {
    let mut pat = tilde_expand(cond);
    if pat.starts_with("./") || pat == "." {
        if let Some(dir) = including_dir {
            let base = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
            let rest = pat.strip_prefix("./").unwrap_or("");
            pat = base.join(rest).to_string_lossy().into_owned();
        }
    } else if !is_absolute_pattern(&pat) {
        pat = format!("**/{pat}");
    }
    if pat.ends_with('/') {
        pat.push_str("**");
    }
    pat
}

fn is_absolute_pattern(pat: &str) -> bool {
    pat.starts_with('/') || (pat.len() >= 3 && pat.as_bytes()[1] == b':' && (pat.as_bytes()[2] == b'/' || pat.as_bytes()[2] == b'\\'))
}

/// `onbranch:` matching (C `include_by_branch`): the pattern matches the
/// `HEAD` symref's short branch name with `WM_PATHNAME` semantics; a trailing
/// slash gains an implicit `/**`. Detached or missing `HEAD` never matches.
fn match_onbranch(pattern: &str, branch: Option<&str>) -> bool {
    let Some(branch) = branch else {
        return false;
    };
    let mut pat = pattern.to_string();
    if pat.ends_with('/') {
        pat.push_str("**");
    }
    wildmatch(&pat, branch, false)
}

/// Read the `HEAD` symref target's short branch name (`refs/heads/<branch>`),
/// even when the branch is unborn (C `refs_resolve_ref_unsafe` reports the
/// symref target regardless of existence). Detached/missing `HEAD` → `None`.
fn resolve_head_branch(git_dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let target = content.strip_prefix("ref: ")?.trim();
    target.strip_prefix("refs/heads/").map(|s| s.to_string())
}

/// Resolve an `include.path` value: tilde expansion, then relative paths
/// resolve from the including file's directory (C `handle_path_include`).
fn resolve_include_path(value: &str, including_dir: Option<&Path>) -> PathBuf {
    expand_path(value, including_dir)
}

/// Expand a leading `~/` or bare `~` via `$HOME`, like C
/// `interpolate_path`. Other `~user` forms are left as-is.
fn tilde_expand(value: &str) -> String {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}/{}", PathBuf::from(home).to_string_lossy(), rest);
        }
    } else if value == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return home.to_string_lossy().into_owned();
        }
    }
    value.to_string()
}

/// A byte index into a (possibly non-UTF8) byte string is a character
/// boundary unless it points at a UTF-8 continuation byte.
fn is_boundary(t: &[u8], i: usize) -> bool {
    i >= t.len() || (t[i] & 0xC0) != 0x80
}

/// Git `wildmatch` with `WM_PATHNAME` semantics (`*`/`?`/`[...]` never cross
/// `/`; `**` does), plus `WM_CASEFOLD` when `casefold` is set. Supports
/// literals, `?`, `*`, `**`, `[...]` classes (ranges, `!`/`^` negation) and
/// backslash escapes — enough for every `t/t1305` + `t/t1300` pattern.
fn wildmatch(pattern: &str, text: &str, casefold: bool) -> bool {
    wm(pattern.as_bytes(), text.as_bytes(), casefold)
}

fn wm(p: &[u8], t: &[u8], cf: bool) -> bool {
    if p.is_empty() {
        return t.is_empty();
    }
    if p[0] == b'*' {
        let mut i = 1;
        while p.get(i) == Some(&b'*') {
            i += 1;
        }
        let double = i >= 2;
        let rest = &p[i..];
        // `*` stops at `/` (pathname); `**` crosses it.
        for k in 0..=t.len() {
            if !double && t[..k].contains(&b'/') {
                break;
            }
            if wm(rest, &t[k..], cf) {
                return true;
            }
            // Never split a multi-byte char when advancing.
            if k < t.len() && !is_boundary(t, k + 1) {
                continue;
            }
        }
        return false;
    }
    if t.is_empty() {
        return false;
    }
    match p[0] {
        b'?' => {
            if t[0] == b'/' {
                return false;
            }
            wm(&p[1..], &t[1..], cf)
        }
        b'\\' if p.len() > 1 => eq_byte(p[1], t[0], cf) && wm(&p[2..], &t[1..], cf),
        b'[' => match_class(p, t, cf).map(|(plen, tlen)| wm(&p[plen..], &t[tlen..], cf)).unwrap_or(false),
        c => eq_byte(c, t[0], cf) && wm(&p[1..], &t[1..], cf),
    }
}

fn eq_byte(a: u8, b: u8, cf: bool) -> bool {
    if cf {
        a.to_ascii_lowercase() == b.to_ascii_lowercase()
    } else {
        a == b
    }
}

/// Match a `[...]` class against the head of `t`. Returns the consumed
/// pattern/text lengths on success. A class never matches `/` (pathname).
fn match_class(p: &[u8], t: &[u8], cf: bool) -> Option<(usize, usize)> {
    let mut i = 1;
    let mut negated = false;
    if p.get(i) == Some(&b'!') || p.get(i) == Some(&b'^') {
        negated = true;
        i += 1;
    }
    if t[0] == b'/' {
        return None;
    }
    let mut matched = false;
    let mut first = true;
    while i < p.len() {
        if p[i] == b']' && !first {
            break;
        }
        if p[i] == b'\\' && i + 1 < p.len() {
            i += 1;
            if eq_byte(p[i], t[0], cf) {
                matched = true;
            }
        } else if i + 2 < p.len() && p[i + 1] == b'-' && p[i + 2] != b']' {
            let (lo, hi) = (p[i], p[i + 2]);
            let c = if cf { t[0].to_ascii_lowercase() } else { t[0] };
            let (lo, hi) = if cf { (lo.to_ascii_lowercase(), hi.to_ascii_lowercase()) } else { (lo, hi) };
            if lo <= c && c <= hi {
                matched = true;
            }
            i += 2;
        } else if eq_byte(p[i], t[0], cf) {
            matched = true;
        }
        i += 1;
        first = false;
    }
    if i >= p.len() {
        return None; // unterminated class: literal-match failure
    }
    if matched != negated {
        // Consume one full character of text (may be multi-byte).
        let mut tlen = 1;
        while tlen < t.len() && !is_boundary(t, tlen) {
            tlen += 1;
        }
        Some((i + 1, tlen))
    } else {
        None
    }
}

/// Phase A: walk the include closure of `path` following every edge
/// (conditional edges count as taken, like C's `populate_remote_urls` with
/// `unconditional_remote_url`), collecting `remote.*.url` values for
/// `hasconfig:` matching and enforcing the remote-URL forbid inside
/// `hasconfig`-reachable subtrees. Missing files are skipped.
fn collect_includes(
    path: &Path,
    from: Option<&Path>,
    seen: &mut Vec<PathBuf>,
    depth: usize,
    remote_urls: &mut Vec<String>,
    under_hasconfig: bool,
) -> Result<(), ConfigError> {
    if !path.exists() {
        return Ok(());
    }
    if let Some(from) = from {
        if depth > MAX_INCLUDE_DEPTH {
            return Err(ConfigError::IncludeDepth {
                limit: MAX_INCLUDE_DEPTH,
                path: path.to_path_buf(),
                from: from.to_path_buf(),
            });
        }
    }
    let (entries, directives) = read_parsed(path, seen)?;
    if under_hasconfig && has_remote_url(&entries) {
        return Err(ConfigError::RemoteUrlForbidden);
    }
    for entry in &entries {
        if entry.section == "remote" && entry.key == "url" && entry.subsection.is_some() {
            remote_urls.push(entry.value.clone());
        }
    }
    for directive in &directives {
        let inc = resolve_include_path(&directive.value, path.parent());
        let is_hasconfig = directive.condition.as_deref().is_some_and(is_hasconfig_condition);
        collect_includes(&inc, Some(path), seen, depth + 1, remote_urls, under_hasconfig || is_hasconfig)?;
    }
    seen.pop();
    Ok(())
}

/// Load several root files: first collect remote URLs across all of them
/// (C evaluates `hasconfig:` against the full config, even entries defined
/// after — or in later files than — the condition), then load in order.
/// `collect_extra` roots contribute remote URLs (and forbid checks) without
/// being loaded — used so the worktree scope sees the main scopes' remotes.
fn load_roots(
    paths: &[PathBuf],
    ctx: &IncludeContext,
    require_first: bool,
    collect_extra: &[PathBuf],
) -> Result<ConfigSet, ConfigError> {
    let mut remote_urls = Vec::new();
    {
        let mut seen = Vec::new();
        for path in paths.iter().chain(collect_extra.iter()) {
            collect_includes(path, None, &mut seen, 0, &mut remote_urls, false)?;
        }
    }
    let mut set = ConfigSet::new();
    for (i, path) in paths.iter().enumerate() {
        let mut seen = Vec::new();
        set.load_file(path, &mut seen, 0, ctx, &remote_urls, false, require_first && i == 0)?;
    }
    Ok(set)
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
                git_dir_verbatim: None,
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

    #[test]
    fn single_line_section_header_with_value() {
        // C parses `[section]key = value` on one line (t/t1305 writes
        // conditional includes this way).
        let cfg = ConfigSet::parse(b"[user]name = inline\n").unwrap();
        assert_eq!(cfg.get("user", "name"), Some("inline"));
        let cfg = ConfigSet::parse(b"[user] # comment\n\tname = alice\n").unwrap();
        assert_eq!(cfg.get("user", "name"), Some("alice"));
    }

    #[test]
    fn unterminated_quote_is_bad_line_like_c() {
        let err = ConfigSet::parse(b"[user]\n\tname = \"oops\n").unwrap_err();
        assert!(matches!(err, ConfigError::BadLine { line: 2, file: None }));
        assert_eq!(format!("{err}"), "bad config line 2");
    }

    fn include_ctx(git_dir: &Path, worktree: Option<&Path>, branch: Option<&str>) -> super::IncludeContext {
        super::IncludeContext {
            git_dir: Some(git_dir.to_path_buf()),
            git_dir_fallback: None,
            worktree: worktree.map(|p| p.to_path_buf()),
            head_branch: branch.map(|s| s.to_string()),
        }
    }

    fn write_include_repo(tag: &str) -> (std::sync::MutexGuard<'static, ()>, PathBuf, PathBuf) {
        // No env dependence (absolute paths only), but serialize against env
        // mutators anyway via the shared lock for determinism.
        let guard = lock_env();
        let dir = std::env::temp_dir().join(format!("git-config-inc-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let git_dir = dir.join("foo").join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        (guard, dir, git_dir)
    }

    #[test]
    fn include_gitdir_matches_inside_ignored_outside() {
        let (_guard, dir, git_dir) = write_include_repo("gitdir");
        std::fs::write(dir.join("yes.conf"), "[test]\n\tone = 1\n").unwrap();
        std::fs::write(dir.join("no.conf"), "[test]\n\ttwo = 2\n").unwrap();
        let main = dir.join("main.conf");
        std::fs::write(
            &main,
            format!(
                "[includeIf \"gitdir:foo/\"]\n\tpath = {}\n[includeIf \"gitdir:other/\"]\n\tpath = {}\n",
                dir.join("yes.conf").display(),
                dir.join("no.conf").display()
            ),
        )
        .unwrap();
        let cfg = ConfigSet::from_file_with(&main, &include_ctx(&git_dir, None, None)).unwrap();
        assert_eq!(cfg.get("test", "one"), Some("1"));
        assert_eq!(cfg.get("test", "two"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn include_gitdir_icase_and_double_star() {
        let (_guard, dir, git_dir) = write_include_repo("icase");
        // foo/.git nested deeper: ** patterns must cross slashes.
        let deep = dir.join("a").join("foo").join("x").join("bar").join(".git");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(dir.join("hit.conf"), "[test]\n\thit = yes\n").unwrap();
        let main = dir.join("main.conf");
        std::fs::write(
            &main,
            format!(
                "[includeIf \"gitdir/i:FOO/\"]\n\tpath = {}\n[includeIf \"gitdir:**/foo/**/bar/**\"]\n\tpath = {}\n",
                dir.join("hit.conf").display(),
                dir.join("hit.conf").display()
            ),
        )
        .unwrap();
        let cfg = ConfigSet::from_file_with(&main, &include_ctx(&deep, None, None)).unwrap();
        // Both conditions match the same file; absence of error + value is the signal.
        assert_eq!(cfg.get("test", "hit"), Some("yes"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn include_gitdir_tilde_and_dot_relative() {
        let layout = isolated_scopes("increl");
        // HOME == layout.dir here.
        let home_foo_git = layout.dir.join("foo").join(".git");
        std::fs::create_dir_all(&home_foo_git).unwrap();
        std::fs::write(layout.dir.join("t.conf"), "[test]\n\ttilde = 1\n").unwrap();
        std::fs::write(layout.dir.join("d.conf"), "[test]\n\tdot = 1\n").unwrap();
        let main = layout.dir.join(".gitconfig");
        std::fs::write(
            &main,
            format!(
                "[includeIf \"gitdir:~/foo/\"]\n\tpath = {}\n[includeIf \"gitdir:./foo/.git\"]\n\tpath = {}\n",
                layout.dir.join("t.conf").display(),
                layout.dir.join("d.conf").display()
            ),
        )
        .unwrap();
        let cfg = ConfigSet::from_file_with(&main, &include_ctx(&home_foo_git, None, None)).unwrap();
        assert_eq!(cfg.get("test", "tilde"), Some("1"));
        assert_eq!(cfg.get("test", "dot"), Some("1"));
    }

    #[test]
    fn include_onbranch_exact_wildcard_and_slash() {
        let (_guard, dir, git_dir) = write_include_repo("onbranch");
        std::fs::write(dir.join("b.conf"), "[test]\n\tb = 1\n").unwrap();
        let main = dir.join("main.conf");
        std::fs::write(
            &main,
            format!(
                "[includeIf \"onbranch:foo-branch\"]\n\tpath = {}\n[includeIf \"onbranch:?oo-*/**\"]\n\tpath = {}\n[includeIf \"onbranch:foo-dir/\"]\n\tpath = {}\n[includeIf \"onbranch:other\"]\n\tpath = {}\n",
                dir.join("b.conf").display(),
                dir.join("b.conf").display(),
                dir.join("b.conf").display(),
                dir.join("b.conf").display()
            ),
        )
        .unwrap();
        // Branch foo-branch/a/b/c: exact fails, wildcard + slash-prefix match.
        let cfg = ConfigSet::from_file_with(&main, &include_ctx(&git_dir, None, Some("foo-branch/a/b/c"))).unwrap();
        assert_eq!(cfg.get("test", "b"), Some("1"));
        // Detached HEAD: nothing matches.
        let cfg = ConfigSet::from_file_with(&main, &include_ctx(&git_dir, None, None)).unwrap();
        assert_eq!(cfg.get("test", "b"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_head_branch_unborn_detached_missing() {
        let (_guard, dir, _git) = write_include_repo("head");
        let gd = dir.join("repo.git");
        std::fs::create_dir_all(&gd).unwrap();
        // Unborn: symref target reports even though the branch is missing.
        std::fs::write(gd.join("HEAD"), "ref: refs/heads/master\n").unwrap();
        assert_eq!(super::resolve_head_branch(&gd).as_deref(), Some("master"));
        // Detached: no branch.
        std::fs::write(gd.join("HEAD"), "3b18e512dba79e4c8300dd08aeb37f8e1c3a69db\n").unwrap();
        assert_eq!(super::resolve_head_branch(&gd), None);
        std::fs::remove_file(gd.join("HEAD")).unwrap();
        assert_eq!(super::resolve_head_branch(&gd), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn include_hasconfig_sees_later_remote_in_same_file() {
        // t/t1300: the remote is defined AFTER the condition, yet matches —
        // the pre-pass collects remotes across the whole file first.
        let (_guard, dir, git_dir) = write_include_repo("hasconfig");
        std::fs::write(dir.join("inc.conf"), "[user]\n\tthis = yes\n").unwrap();
        let main = dir.join("main.conf");
        std::fs::write(
            &main,
            format!(
                "[includeIf \"hasconfig:remote.*.url:foourl\"]\n\tpath = {}\n[remote \"foo\"]\n\turl = foourl\n",
                dir.join("inc.conf").display()
            ),
        )
        .unwrap();
        let cfg = ConfigSet::from_file_with(&main, &include_ctx(&git_dir, None, None)).unwrap();
        assert_eq!(cfg.get("user", "this"), Some("yes"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn include_hasconfig_globs() {
        let (_guard, dir, git_dir) = write_include_repo("hasglob");
        for (name, key) in [("s1", "dss"), ("s2", "dse"), ("s3", "dsm"), ("s4", "ssm")] {
            std::fs::write(dir.join(name), format!("[user]\n\t{key} = yes\n")).unwrap();
        }
        std::fs::write(dir.join("no"), "[user]\n\tno = no\n").unwrap();
        let main = dir.join("main.conf");
        let p = |n: &str| dir.join(n).display().to_string();
        std::fs::write(
            &main,
            format!(
                "[remote \"foo\"]\n\turl = https://foo/bar/baz\n\
                 [includeIf \"hasconfig:remote.*.url:**/baz\"]\n\tpath = {}\n\
                 [includeIf \"hasconfig:remote.*.url:**/nomatch\"]\n\tpath = {}\n\
                 [includeIf \"hasconfig:remote.*.url:https:/**\"]\n\tpath = {}\n\
                 [includeIf \"hasconfig:remote.*.url:https:/**/baz\"]\n\tpath = {}\n\
                 [includeIf \"hasconfig:remote.*.url:https://*/bar/baz\"]\n\tpath = {}\n\
                 [includeIf \"hasconfig:remote.*.url:https://*/baz\"]\n\tpath = {}\n",
                p("s1"),
                p("no"),
                p("s2"),
                p("s3"),
                p("s4"),
                p("no"),
            ),
        )
        .unwrap();
        let cfg = ConfigSet::from_file_with(&main, &include_ctx(&git_dir, None, None)).unwrap();
        for key in ["dss", "dse", "dsm", "ssm"] {
            assert_eq!(cfg.get("user", key), Some("yes"), "key {key}");
        }
        assert_eq!(cfg.get("user", "no"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn include_hasconfig_forbids_remote_url_with_c_text() {
        let (_guard, dir, git_dir) = write_include_repo("forbid");
        std::fs::write(dir.join("evil.conf"), "[remote \"bar\"]\n\turl = barurl\n").unwrap();
        let main = dir.join("main.conf");
        std::fs::write(
            &main,
            format!(
                "[remote \"foo\"]\n\turl = foourl\n[includeIf \"hasconfig:remote.*.url:foourl\"]\n\tpath = {}\n",
                dir.join("evil.conf").display()
            ),
        )
        .unwrap();
        let err = ConfigSet::from_file_with(&main, &include_ctx(&git_dir, None, None)).unwrap_err();
        assert_eq!(err, ConfigError::RemoteUrlForbidden);
        assert_eq!(
            format!("{err}"),
            "remote URLs cannot be configured in file directly or indirectly included by includeIf.hasconfig:remote.*.url"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn include_unknown_condition_is_silently_false() {
        let (_guard, dir, git_dir) = write_include_repo("unknown");
        std::fs::write(dir.join("u.conf"), "[test]\n\tu = 1\n").unwrap();
        let main = dir.join("main.conf");
        std::fs::write(
            &main,
            format!("[includeIf \"frobnicate:everything\"]\n\tpath = {}\n", dir.join("u.conf").display()),
        )
        .unwrap();
        let cfg = ConfigSet::from_file_with(&main, &include_ctx(&git_dir, None, None)).unwrap();
        assert_eq!(cfg.get("test", "u"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn include_worktree_matches_and_bare_is_false() {
        let (_guard, dir, git_dir) = write_include_repo("worktree");
        std::fs::write(dir.join("w.conf"), "[test]\n\tw = 1\n").unwrap();
        let wt = dir.join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        let main = dir.join("main.conf");
        std::fs::write(
            &main,
            format!(
                "[includeIf \"worktree:{}\"]\n\tpath = {}\n[includeIf \"worktree:{}/\"]\n\tpath = {}\n",
                wt.display(),
                dir.join("w.conf").display(),
                wt.display(),
                dir.join("w.conf").display()
            ),
        )
        .unwrap();
        let cfg = ConfigSet::from_file_with(&main, &include_ctx(&git_dir, Some(&wt), None)).unwrap();
        assert_eq!(cfg.get("test", "w"), Some("1"));
        // Bare repos have no worktree: never matches (t/t1305 worktree-bare).
        let cfg = ConfigSet::from_file_with(&main, &include_ctx(&git_dir, None, None)).unwrap();
        assert_eq!(cfg.get("test", "w"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn include_depth_overflow_dies_c_exactly() {
        let (_guard, dir, _git) = write_include_repo("depth");
        for i in 0..12 {
            let next = if i < 11 {
                format!("[include]\n\tpath = {}\n", dir.join(format!("c{}.conf", i + 1)).display())
            } else {
                "[user]\n\tname = deep\n".to_string()
            };
            std::fs::write(dir.join(format!("c{i}.conf")), next).unwrap();
        }
        let err = ConfigSet::from_file(&dir.join("c0.conf")).unwrap_err();
        assert!(
            matches!(err, ConfigError::IncludeDepth { limit: 10, .. }),
            "unexpected error: {err:?}"
        );
        // Byte-shape of C's `include_depth_advice` die (fatal prefix added
        // by the surface).
        let text = format!("{err}");
        assert!(text.starts_with("exceeded maximum include depth (10) while including\n\t"));
        assert!(text.contains("\nfrom\n\t"));
        assert!(text.ends_with("\nThis might be due to circular includes."));
        assert!(text.contains("c11.conf"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn include_direct_cycle_reports_cycle_error() {
        let dir = std::env::temp_dir().join(format!("git-config-cycle2-{}", std::process::id()));
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
    fn include_missing_file_is_skipped_like_c() {
        let (_guard, dir, _git) = write_include_repo("missing");
        let main = dir.join("main.conf");
        std::fs::write(
            &main,
            "[include]\n\tpath = /nonexistent-xyz-git-rs.conf\n[user]\n\tname = bob\n",
        )
        .unwrap();
        let cfg = ConfigSet::from_file(&main).unwrap();
        assert_eq!(cfg.get("user", "name"), Some("bob"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn include_diamond_reprocesses_without_false_cycle() {
        // a → {b, c}, b → d, c → d: d is visited twice (like C), no error.
        let (_guard, dir, _git) = write_include_repo("diamond");
        let p = |n: &str| dir.join(n).display().to_string();
        std::fs::write(dir.join("d.conf"), "[test]\n\td = 1\n").unwrap();
        std::fs::write(dir.join("b.conf"), format!("[include]\n\tpath = {}\n", p("d.conf"))).unwrap();
        std::fs::write(dir.join("c.conf"), format!("[include]\n\tpath = {}\n", p("d.conf"))).unwrap();
        std::fs::write(
            dir.join("a.conf"),
            format!("[include]\n\tpath = {}\n[include]\n\tpath = {}\n", p("b.conf"), p("c.conf")),
        )
        .unwrap();
        let cfg = ConfigSet::from_file(&dir.join("a.conf")).unwrap();
        assert_eq!(cfg.get("test", "d"), Some("1"));
        // Processed twice, like C (no memoization across siblings).
        assert_eq!(cfg.get_all("test", "d"), vec!["1", "1"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wildmatch_unit_matrix() {
        let t = super::wildmatch;
        assert!(t("**/foo/**", "/x/foo/.git", false));
        assert!(!t("**/foo/**", "/x/foobar/.git", false));
        assert!(t("**/FOO/**", "/x/foo/", true));
        assert!(!t("**/FOO/**", "/x/foo/", false));
        assert!(t("?oo-*/**", "foo-branch/a/b/c", false));
        assert!(!t("?oo-*/**", "foo-branch", false)); // trailing `/**` needs the slash
        assert!(t("https://*/bar/baz", "https://foo/bar/baz", false));
        assert!(!t("https://*/bar/baz", "https://foo/a/bar/baz", false));
        assert!(t("https:/**", "https://foo/bar/baz", false));
        assert!(t("[a-z]oo", "foo", false));
        assert!(!t("[a-z]oo", "Foo", false));
        assert!(t("[!a]oo", "boo", false));
        assert!(!t("*.git", "a/b.git", false));
        assert!(t("*.git", "a.git", false));
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
