//! `git init`: create a new empty git repository.
//!
//! Port of `builtin/init-db.c` (`cmd_init_db`) and the `init_db()` /
//! `create_default_files()` machinery in `setup.c`: argument parsing with
//! C-parse-options-compatible diagnostics, `--bare` / `--separate-git-dir` /
//! `--template` / `--shared` / `-b` handling, template copying, config-file
//! surgery, filesystem probes (`filemode`, `ignorecase`, `precomposeunicode`,
//! `symlinks`), and `Initialized` / `Reinitialized` reporting.

use std::io::Write;
use std::path::{Component, Path, PathBuf};

use crate::{Command, CommandError, RepoContext};

pub struct Init;

impl Command for Init {
    fn name(&self) -> &'static str {
        "init"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let parsed = match parse_args(args) {
            Ok(p) => p,
            Err(ParseFail::Stderr(msg)) => return Err(CommandError::usage(msg)),
            Err(ParseFail::StdoutUsage { stderr_msg }) => {
                // C prints the usage block to stdout here (e.g. ambiguous
                // abbreviations) while the error itself goes to stderr.
                out.write_all(FULL_USAGE.as_bytes())
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
                out.write_all(b"\n")
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
                out.flush().map_err(|e| CommandError::fatal(e.to_string()))?;
                return Err(CommandError::usage(stderr_msg));
            }
        };
        if parsed.help {
            // Full usage on stdout with C's trailing blank line; exit 129.
            out.write_all(FULL_USAGE.as_bytes())
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            out.write_all(b"\n")
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            out.flush().map_err(|e| CommandError::fatal(e.to_string()))?;
            return Err(CommandError::silent(129));
        }
        if parsed.operands.len() > 1 {
            return Err(CommandError::usage(SHORT_USAGE.to_string()));
        }
        init_db(ctx, &parsed, out)
    }
}

/// Synopsis-only usage (extra operands, `takes no value`), stderr, exit 129.
const SHORT_USAGE: &str = "usage: git init [-q | --quiet] [--bare] [--template=<template-directory>]\n         [--separate-git-dir <git-dir>] [--object-format=<format>]\n         [--ref-format=<format>]\n         [-b <branch-name> | --initial-branch=<branch-name>]\n         [--shared[=<permissions>]] [<directory>]";

/// Full usage (`--help` on stdout; unknown/ambiguous options on stderr).
const FULL_USAGE: &str = "usage: git init [-q | --quiet] [--bare] [--template=<template-directory>]\n                [--separate-git-dir <git-dir>] [--object-format=<format>]\n                [--ref-format=<format>]\n                [-b <branch-name> | --initial-branch=<branch-name>]\n                [--shared[=<permissions>]] [<directory>]\n\n    --[no-]template <template-directory>\n                          directory from which templates will be used\n    --[no-]bare           create a bare repository\n    --shared[=<permissions>]\n                          specify that the git repository is to be shared amongst several users\n    -q, --[no-]quiet      be quiet\n    --[no-]separate-git-dir <gitdir>\n                          separate git dir from working tree\n    -b, --[no-]initial-branch <name>\n                          override the name of the initial branch\n    --[no-]object-format <hash>\n                          specify the hash algorithm to use\n    --[no-]ref-format <format>\n                          specify the reference format to use\n";

/// `error: ...` + full usage, for unknown options (exit 129, all stderr).
fn full_usage_error(first_line: String) -> ParseFail {
    // FULL_USAGE ends with a single '\n'; eprintln appends the final
    // newline, reproducing C's trailing blank line byte-for-byte.
    ParseFail::Stderr(format!("{first_line}\n{FULL_USAGE}"))
}

/// Ambiguous abbreviations: error on stderr, usage block on stdout (129).
fn stdout_usage_error(first_line: String) -> ParseFail {
    ParseFail::StdoutUsage { stderr_msg: first_line }
}

/// Parse failure routing: C prints some usage blocks on stdout.
#[derive(Debug)]
enum ParseFail {
    /// Message (possibly with usage) goes to stderr.
    Stderr(String),
    /// `stderr_msg` goes to stderr; the full usage block goes to stdout.
    StdoutUsage { stderr_msg: String },
}

impl From<ParseFail> for CommandError {
    fn from(f: ParseFail) -> CommandError {
        match f {
            ParseFail::Stderr(msg) => CommandError::usage(msg),
            ParseFail::StdoutUsage { stderr_msg } => CommandError::usage(stderr_msg),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SharedVal {
    Group,
    Everybody,
    /// A literal `0xxx` file mode (already masked to `0o666`).
    Mode(u32),
}

#[derive(Debug, Default)]
struct InitArgs {
    help: bool,
    template: TemplateOpt,
    bare: Option<bool>,
    shared: Option<SharedVal>,
    quiet: bool,
    separate_git_dir: Option<String>,
    initial_branch: Option<String>,
    object_format: Option<String>,
    ref_format: Option<String>,
    operands: Vec<String>,
}

#[derive(Debug, Default)]
enum TemplateOpt {
    /// No `--template` given: `$GIT_TEMPLATE_DIR`, `init.templatedir`, default dir.
    #[default]
    Default,
    /// `--template=` (empty): copy no templates at all.
    Skip,
    /// `--template=<dir>`: use this directory (made absolute).
    Dir(String),
}

/// A long option taking a required (`true`) or no (`false`) value.
struct LongOpt {
    name: &'static str,
    takes_value: OptValue,
    negatable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OptValue {
    Required,
    None,
    /// `--shared[=perm]`: attached `=perm` only, never a separate arg.
    Optional,
}

const LONG_OPTS: &[LongOpt] = &[
    LongOpt { name: "template", takes_value: OptValue::Required, negatable: true },
    LongOpt { name: "bare", takes_value: OptValue::None, negatable: true },
    LongOpt { name: "shared", takes_value: OptValue::Optional, negatable: false },
    LongOpt { name: "quiet", takes_value: OptValue::None, negatable: true },
    LongOpt { name: "separate-git-dir", takes_value: OptValue::Required, negatable: true },
    LongOpt { name: "initial-branch", takes_value: OptValue::Required, negatable: true },
    LongOpt { name: "object-format", takes_value: OptValue::Required, negatable: true },
    LongOpt { name: "ref-format", takes_value: OptValue::Required, negatable: true },
    LongOpt { name: "help", takes_value: OptValue::None, negatable: false },
];

fn find_long(name: &str) -> Result<&'static LongOpt, ParseFail> {
    if let Some(o) = LONG_OPTS.iter().find(|o| o.name == name) {
        return Ok(o);
    }
    let mut matches = LONG_OPTS.iter().filter(|o| o.name.starts_with(name));
    match (matches.next(), matches.next()) {
        (Some(o), None) => Ok(o),
        (Some(first), Some(_)) => {
            let mut rest = LONG_OPTS.iter().filter(|o| o.name.starts_with(name));
            let mut list: Vec<&str> = Vec::new();
            for o in rest.by_ref() {
                list.push(o.name);
            }
            let _ = first;
            let mut text = format!("--{}", list[0]);
            for c in &list[1..] {
                text.push_str(&format!(" or --{c}"));
            }
            Err(stdout_usage_error(format!("error: ambiguous option: {name} (could be {text})")))
        }
        (None, _) => Err(full_usage_error(format!("error: unknown option `{name}'"))),
    }
}

fn parse_args(args: &[String]) -> Result<InitArgs, ParseFail> {
    let mut out = InitArgs::default();
    let mut i = 0usize;
    let mut end_of_opts = false;
    while i < args.len() {
        let a = &args[i];
        if end_of_opts {
            out.operands.push(a.clone());
        } else if a == "--" {
            end_of_opts = true;
        } else if let Some(long) = a.strip_prefix("--") {
            let (mut name, inline) = match long.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (long.to_string(), None),
            };
            let mut negated = false;
            if let Some(rest) = name.strip_prefix("no-") {
                // Only negatable options accept `--no-`; otherwise C treats
                // `--no-x` as an (unknown) option name. Match by trying the
                // stripped name first.
                if LONG_OPTS.iter().any(|o| o.negatable && o.name == rest)
                    || LONG_OPTS.iter().filter(|o| o.negatable && o.name.starts_with(rest)).count() == 1
                {
                    negated = true;
                    name = rest.to_string();
                }
            }
            let opt = find_long(&name)?;
            if negated && !opt.negatable {
                return Err(full_usage_error(format!("error: unknown option `no-{name}'")));
            }
            if inline.is_some() && opt.takes_value == OptValue::None {
                return Err(ParseFail::Stderr(format!("error: option `{name}' takes no value")));
            }
            if negated && inline.is_some() {
                return Err(ParseFail::Stderr(format!("error: option `no-{name}' takes no value")));
            }
            match opt.name {
                "help" => out.help = true,
                "quiet" => out.quiet = !negated,
                "bare" => out.bare = Some(!negated),
                "template" => {
                    if negated {
                        out.template = TemplateOpt::Default;
                    } else if let Some(v) = inline {
                        out.template =
                            if v.is_empty() { TemplateOpt::Skip } else { TemplateOpt::Dir(v) };
                    } else {
                        i += 1;
                        let v = args.get(i).ok_or_else(|| {
                            ParseFail::Stderr("error: option `template' requires a value".to_string())
                        })?;
                        out.template = TemplateOpt::Dir(v.clone());
                    }
                }
                "separate-git-dir" => {
                    if negated {
                        out.separate_git_dir = None;
                    } else if let Some(v) = inline {
                        out.separate_git_dir = Some(v);
                    } else {
                        i += 1;
                        let v = args.get(i).ok_or_else(|| {
                            ParseFail::Stderr(
                                "error: option `separate-git-dir' requires a value".to_string(),
                            )
                        })?;
                        out.separate_git_dir = Some(v.clone());
                    }
                }
                "initial-branch" => {
                    if negated {
                        out.initial_branch = None;
                    } else if let Some(v) = inline {
                        out.initial_branch = Some(v);
                    } else {
                        i += 1;
                        let v = args.get(i).ok_or_else(|| {
                            ParseFail::Stderr(
                                "error: option `initial-branch' requires a value".to_string(),
                            )
                        })?;
                        out.initial_branch = Some(v.clone());
                    }
                }
                "object-format" => {
                    if negated {
                        out.object_format = None;
                    } else if let Some(v) = inline {
                        out.object_format = Some(v);
                    } else {
                        i += 1;
                        let v = args.get(i).ok_or_else(|| {
                            ParseFail::Stderr(
                                "error: option `object-format' requires a value".to_string(),
                            )
                        })?;
                        out.object_format = Some(v.clone());
                    }
                }
                "ref-format" => {
                    if negated {
                        out.ref_format = None;
                    } else if let Some(v) = inline {
                        out.ref_format = Some(v);
                    } else {
                        i += 1;
                        let v = args.get(i).ok_or_else(|| {
                            ParseFail::Stderr(
                                "error: option `ref-format' requires a value".to_string(),
                            )
                        })?;
                        out.ref_format = Some(v.clone());
                    }
                }
                "shared" => {
                    if negated {
                        out.shared = None;
                    } else {
                        let v = inline.as_deref().unwrap_or("");
                        out.shared = Some(parse_shared(v)?);
                    }
                }
                _ => unreachable!(),
            }
        } else if a.starts_with('-') && a.len() > 1 {
            // Bundled short options.
            let chars: Vec<char> = a[1..].chars().collect();
            let mut j = 0usize;
            while j < chars.len() {
                match chars[j] {
                    'q' => out.quiet = true,
                    'h' => out.help = true,
                    'b' => {
                        let rest: String = chars[j + 1..].iter().collect();
                        if !rest.is_empty() {
                            out.initial_branch = Some(rest);
                        } else {
                            i += 1;
                            let v = args.get(i).ok_or_else(|| {
                                ParseFail::Stderr(
                                    "error: switch `b' requires a value".to_string(),
                                )
                            })?;
                            out.initial_branch = Some(v.clone());
                        }
                        break;
                    }
                    c => {
                        return Err(full_usage_error(format!("error: unknown switch `{c}'")));
                    }
                }
                j += 1;
            }
        } else {
            out.operands.push(a.clone());
        }
        i += 1;
    }
    Ok(out)
}

/// Parse a `--shared[=value]` argument (C `git_config_perm` semantics).
fn parse_shared(value: &str) -> Result<SharedVal, CommandError> {
    if value.is_empty() || value == "true" || value == "group" {
        return Ok(SharedVal::Group);
    }
    if value == "umask" {
        return Ok(SharedVal::Mode(0));
    }
    if value == "all" || value == "world" || value == "everybody" {
        return Ok(SharedVal::Everybody);
    }
    // Octal file mode?
    if !value.is_empty() && value.chars().all(|c| ('0'..='7').contains(&c)) {
        let mode = u32::from_str_radix(value, 8).unwrap_or(0);
        match mode {
            0 => return Ok(SharedVal::Mode(0)),
            1 => return Ok(SharedVal::Group),
            2 => return Ok(SharedVal::Everybody),
            _ => {
                if mode & 0o600 != 0o600 {
                    return Err(CommandError::fatal(format!(
                        "fatal: problem with core.sharedRepository filemode value (0{mode:03o}).\nThe owner of files must always have read and write permissions."
                    )));
                }
                return Ok(SharedVal::Mode(mode & 0o666));
            }
        }
    }
    // Boolean fallback like C (`git_config_bool` dies on garbage).
    match git_config::parse_bool(value) {
        Some(true) => Ok(SharedVal::Group),
        Some(false) => Ok(SharedVal::Mode(0)),
        None => Err(CommandError::fatal(format!(
            "fatal: bad boolean config value '{value}' for 'arg'"
        ))),
    }
}

impl SharedVal {
    /// Whether this value counts as "shared" for messages/config.
    fn is_shared(&self) -> bool {
        !matches!(self, SharedVal::Mode(0))
    }

    /// The `core.sharedrepository` value C writes.
    fn config_value(&self) -> String {
        match *self {
            SharedVal::Mode(0) => "0".to_string(),
            SharedVal::Mode(m) => format!("0{m:o}"),
            SharedVal::Group => "1".to_string(),
            SharedVal::Everybody => "2".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Config layering: system + global + `-c` overrides (never the cwd repo).
// ---------------------------------------------------------------------------

/// Load the system + global config C consults for `init` defaults, then apply
/// `git -c` overrides (highest precedence).
fn base_config(ctx: &RepoContext) -> git_config::ConfigSet {
    use git_config::ConfigSet;
    let mut cfg = ConfigSet::new();
    // System config: explicit override, or the first existing candidate.
    // (`GIT_CONFIG_NOSYSTEM` suppresses it, like C.)
    if std::env::var_os("GIT_CONFIG_NOSYSTEM").is_none() {
        if let Some(p) = std::env::var_os("GIT_CONFIG_SYSTEM") {
            let p = PathBuf::from(p);
            // A set-but-not-regular path (e.g. `/dev/null`) falls back to
            // the compiled default, matching observed C behavior; a set but
            // missing path means "no system config".
            if p.is_file() {
                load_lenient(&mut cfg, &p);
            } else if p.symlink_metadata().is_err() {
                // Missing: no system config (upstream semantics).
            } else {
                for cand in system_config_candidates() {
                    if cand.is_file() {
                        load_lenient(&mut cfg, &cand);
                        break;
                    }
                }
            }
        } else {
            for cand in system_config_candidates() {
                if cand.is_file() {
                    load_lenient(&mut cfg, &cand);
                    break;
                }
            }
        }
    }
    // Global config: explicit override, else XDG + ~/.gitconfig.
    // (Missing/unreadable files are silently skipped, like C.)
    if let Some(p) = std::env::var_os("GIT_CONFIG_GLOBAL") {
        load_lenient(&mut cfg, &PathBuf::from(p));
    } else {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        if let Some(p) = xdg {
            load_lenient(&mut cfg, &p.join("git/config"));
        } else if let Some(h) = &home {
            load_lenient(&mut cfg, &h.join(".config/git/config"));
        }
        if let Some(h) = home {
            load_lenient(&mut cfg, &h.join(".gitconfig"));
        }
    }
    for (name, value) in &ctx.config_overrides {
        cfg.set_cli(name, value.as_deref());
    }
    cfg
}

fn load_lenient(cfg: &mut git_config::ConfigSet, path: &Path) {
    // Missing/unreadable system/global files are fine (like C); a present
    // file resolves its `[include]`s relative to itself via from_file.
    if let Ok(set) = git_config::ConfigSet::from_file(path) {
        cfg.append(set);
    }
}

/// System config candidates in precedence order (first existing file wins).
fn system_config_candidates() -> Vec<PathBuf> {
    let mut out = vec![PathBuf::from("/etc/gitconfig"), PathBuf::from("/usr/local/etc/gitconfig")];
    // Apple Xcode CLT git keeps its system config next to the templates.
    out.push(PathBuf::from(
        "/Library/Developer/CommandLineTools/usr/share/git-core/gitconfig",
    ));
    out
}

/// Expand a leading `~/` against `$HOME` (C `interpolate_path` subset used
/// for `init.templatedir` and include paths).
fn expand_tilde(value: &str, _relative_to: &Path) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(value)
}

// ---------------------------------------------------------------------------
// The init itself.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HashAlgo {
    Sha1,
    Sha256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefFormat {
    Files,
    Reftable,
}

fn init_db(ctx: &RepoContext, parsed: &InitArgs, out: &mut dyn Write) -> Result<(), CommandError> {
    // --bare / global --bare.
    let mut bare: i32 = if ctx.bare { 1 } else { -1 };
    if let Some(b) = parsed.bare {
        bare = if b { 1 } else { 0 };
    }

    let operand = parsed.operands.first().cloned();

    // The target directory (created when an operand is given).
    let work_base: PathBuf = match &operand {
        Some(dir) => {
            let target = ctx.cwd.join(dir);
            std::fs::create_dir_all(&target).map_err(|e| {
                CommandError::fatal(format!("fatal: cannot mkdir '{}': {}", dir, io_strerror(&e)))
            })?;
            canonicalize(&target).map_err(|e| {
                CommandError::fatal(format!("fatal: cannot mkdir '{}': {}", dir, io_strerror(&e)))
            })?
        }
        None => canonicalize(&ctx.cwd)
            .map_err(|e| CommandError::fatal(format!("fatal: {}", io_strerror(&e))))?,
    };

    // GIT_DIR resolution mirrors C: with an operand and bare==1 the git dir
    // is the target itself; otherwise GIT_DIR/--git-dir or ".git".
    let cli_git_dir = ctx.git_dir.clone();
    let env_git_dir = std::env::var_os("GIT_DIR").map(PathBuf::from);
    let mut git_dir_src: Option<PathBuf> = cli_git_dir.or(env_git_dir);
    // C sets GIT_DIR to the cwd (or the operand directory) whenever `--bare`
    // is in effect; without an operand it only does so when GIT_DIR is not
    // already set (`setenv(..., argc > 0)`).
    if bare == 1 && (operand.is_some() || git_dir_src.is_none()) {
        git_dir_src = Some(work_base.clone());
    }
    let git_dir_raw = git_dir_src
        .clone()
        .unwrap_or_else(|| PathBuf::from(".git"));

    // GIT_WORK_TREE is only valid together with an explicit git dir (and
    // never with a bare repository).
    let work_tree_env: Option<PathBuf> = ctx
        .work_tree
        .clone()
        .or_else(|| std::env::var_os("GIT_WORK_TREE").map(PathBuf::from));
    if work_tree_env.is_some() && (git_dir_src.is_none() || bare == 1) {
        return Err(CommandError::fatal(
            "fatal: GIT_WORK_TREE (or --work-tree=<directory>) not allowed without specifying GIT_DIR (or --git-dir=<directory>)".to_string(),
        ));
    }

    // --separate-git-dir is absolutized against the invoking directory
    // (C resolves it before chdir into the operand).
    let real_git_dir: Option<PathBuf> = parsed
        .separate_git_dir
        .as_ref()
        .map(|d| absolutize(&ctx.cwd, Path::new(d)));
    if real_git_dir.is_some() && bare == 1 {
        return Err(CommandError::fatal(
            "fatal: options '--separate-git-dir' and '--bare' cannot be used together".to_string(),
        ));
    }

    // Guess bare/non-bare from the git dir string when not specified.
    if bare < 0 {
        bare = if guess_bare(&git_dir_raw, &work_base) { 1 } else { 0 };
    }
    let bare = bare == 1;

    if bare && real_git_dir.is_some() {
        return Err(CommandError::fatal(
            "fatal: --separate-git-dir incompatible with bare repository".to_string(),
        ));
    }

    // Absolute git dir (before separation): relative paths resolve inside
    // the target directory, like C after chdir().
    let original_git_dir = absolutize(&work_base, &git_dir_raw);

    // The work tree: explicit override, else derived from the git dir.
    let work_tree: Option<PathBuf> = if bare {
        None
    } else if let Some(w) = work_tree_env {
        Some(absolutize(&work_base, &w))
    } else if let Some(parent) = original_git_dir.parent() {
        if parent.as_os_str().is_empty() {
            Some(work_base.clone())
        } else {
            Some(parent.to_path_buf())
        }
    } else {
        Some(work_base.clone())
    };

    // Resolve a `gitdir:` link file (re-init inside a separated work tree).
    let mut git_dir = original_git_dir.clone();
    if git_dir.is_file() {
        if let Some(linked) = read_gitfile(&git_dir) {
            git_dir = absolutize(&git_dir.parent().unwrap_or(&work_base).to_path_buf(), &linked);
        }
    }

    let reinit = git_dir.join("HEAD").is_file() || is_symlink(&git_dir.join("HEAD"));

    // Format + shared + template configuration from system/global/`-c`.
    let base = base_config(ctx);
    let existing: Option<ConfigFile> = std::fs::read(git_dir.join("config"))
        .ok()
        .map(|data| ConfigFile::parse(&data));

    let (hash_algo, ref_format) =
        resolve_formats(parsed, &base, existing.as_ref(), reinit)?;

    // Shared repository setting: `--shared` beats config. The config
    // fallback (including template-provided values) resolves after the
    // template copy below, like C which re-reads config at that point.
    let mut shared: Option<SharedVal> = match &parsed.shared {
        Some(v) => Some(*v),
        None => base.get("core", "sharedrepository").and_then(|v| config_perm(v).ok()),
    };

    // Template directory: --template > $GIT_TEMPLATE_DIR > init.templatedir
    // > compiled default. Empty means "no templates".
    let template_dir = resolve_template_dir(parsed, &base, &ctx.cwd, &work_base);

    // Create the git dir + object store layout.
    let object_dir: Option<PathBuf> = std::env::var_os("GIT_OBJECT_DIRECTORY")
        .map(|d| absolutize(&work_base, Path::new(&d)));
    let store_dir = object_dir.clone().unwrap_or_else(|| git_dir.join("objects"));
    for d in [&git_dir, &store_dir, &store_dir.join("pack"), &store_dir.join("info")] {
        std::fs::create_dir_all(d)
            .map_err(|e| CommandError::fatal(format!("fatal: {}", io_strerror(&e))))?;
    }

    // --separate-git-dir: move an existing git dir aside, then link it.
    if let Some(real) = &real_git_dir {
        separate_git_dir(&original_git_dir, real)?;
        git_dir = real.clone();
    }

    // Copy templates (never overwriting existing files).
    if let Some(tpl) = template_dir {
        copy_templates(&git_dir, &tpl)?;
    }

    // C re-reads the config after copying templates: a template-provided
    // `config` file participates from here on (fresh repos only; reinit
    // files are never overwritten by the copy above).
    let existing: Option<ConfigFile> = if reinit {
        existing
    } else {
        std::fs::read(git_dir.join("config")).ok().map(|data| ConfigFile::parse(&data))
    };
    // A template config can also carry the shared setting (like C, which
    // re-reads config after the copy).
    if shared.is_none() {
        if let Some(ex) = &existing {
            if let Some(v) = ex.get("core", "sharedrepository") {
                shared = match v.as_str() {
                    "1" => Some(SharedVal::Group),
                    "2" => Some(SharedVal::Everybody),
                    _ => config_perm(&v).ok(),
                };
            }
        }
    }

    // refs layout (the files ref-store creates these on demand; init
    // materializes them like C's ref_store_create_on_disk).
    for d in [&git_dir.join("refs"), &git_dir.join("refs").join("heads"), &git_dir.join("refs").join("tags")] {
        std::fs::create_dir_all(d)
            .map_err(|e| CommandError::fatal(format!("fatal: {}", io_strerror(&e))))?;
    }

    // Config file surgery.
    let template_config_has = |key: (&str, &str)| -> bool {
        std::fs::read(git_dir.join("config"))
            .ok()
            .map(|data| ConfigFile::parse(&data))
            .and_then(|c| c.get(key.0, key.1))
            .is_some()
    };
    write_config(
        &git_dir,
        existing.as_ref(),
        reinit,
        hash_algo,
        ref_format,
        bare,
        &work_tree,
        &original_git_dir,
        shared,
        template_config_has,
        base.get("init", "defaultsubmodulepathconfig").and_then(git_config::parse_bool).unwrap_or(false),
    )?;

    // Shared permissions fixup over the freshly created git dir.
    if let Some(s) = shared {
        if s.is_shared() {
            adjust_shared_perm(&git_dir, s);
        }
    }

    // HEAD + initial branch (fresh repos only).
    if !reinit {
        let branch = match &parsed.initial_branch {
            Some(b) => {
                let full = format!("refs/heads/{b}");
                git_refs::validate_refname(&full)
                    .map_err(|_| CommandError::fatal(format!("fatal: invalid initial branch name: '{b}'")))?;
                b.clone()
            }
            None => default_branch_name(&base, parsed.quiet)?,
        };
        std::fs::write(git_dir.join("HEAD"), format!("ref: refs/heads/{branch}\n"))
            .map_err(|e| CommandError::fatal(format!("fatal: {}", io_strerror(&e))))?;
    } else if let Some(b) = &parsed.initial_branch {
        eprintln!("warning: re-init: ignored --initial-branch={b}");
    }

    // Report (physical path like C's real_pathdup, with trailing slash).
    if let Ok(canon) = canonicalize(&git_dir) {
        git_dir = canon;
    }
    if !parsed.quiet {
        let shared_now = shared.map(|s| s.is_shared()).unwrap_or(false);
        let kind = if reinit { "Reinitialized existing" } else { "Initialized empty" };
        let shared_word = if shared_now { " shared" } else { "" };
        writeln!(out, "{kind}{shared_word} Git repository in {}/", git_dir.display())
            .map_err(|e| CommandError::fatal(format!("fatal: {}", io_strerror(&e))))?;
    }
    Ok(())
}

/// C `guess_repository_type()`: `GIT_DIR=.` (or `$PWD`) is always bare,
/// `.git` / `*/.git` usually not, everything else usually bare.
fn guess_bare(git_dir: &Path, cwd: &Path) -> bool {
    let s = git_dir.to_string_lossy();
    if s == "." {
        return true;
    }
    if git_dir == cwd {
        return true;
    }
    if s == ".git" {
        return false;
    }
    if s.ends_with("/.git") {
        return false;
    }
    true
}

/// Read a `gitdir: <path>` link file.
fn read_gitfile(path: &Path) -> Option<PathBuf> {
    let data = std::fs::read(path).ok()?;
    let text = std::str::from_utf8(&data).ok()?;
    let rest = text.strip_prefix("gitdir:")?.trim();
    if rest.is_empty() {
        return None;
    }
    Some(PathBuf::from(rest))
}

fn is_symlink(p: &Path) -> bool {
    std::fs::symlink_metadata(p).map(|m| m.file_type().is_symlink()).unwrap_or(false)
}

/// Resolve formats exactly like `repository_format_configure()`.
fn resolve_formats(
    parsed: &InitArgs,
    base: &git_config::ConfigSet,
    existing: Option<&ConfigFile>,
    reinit: bool,
) -> Result<(HashAlgo, RefFormat), CommandError> {
    // --object-format / GIT_DEFAULT_HASH / init.defaultObjectFormat.
    let mut cli_hash = None;
    if let Some(v) = &parsed.object_format {
        if !v.is_empty() {
            cli_hash = Some(hash_by_name(v).ok_or_else(|| {
                CommandError::fatal(format!("fatal: unknown hash algorithm '{v}'"))
            })?);
        }
    }
    let mut env_hash = None;
    if let Some(v) = std::env::var_os("GIT_DEFAULT_HASH") {
        let v = v.to_string_lossy().into_owned();
        if !v.is_empty() {
            env_hash = Some(hash_by_name(&v).ok_or_else(|| {
                CommandError::fatal(format!("fatal: unknown hash algorithm '{v}'"))
            })?);
        }
    }
    let mut cfg_hash = None;
    if let Some(v) = base.get("init", "defaultobjectformat") {
        if !v.is_empty() {
            match hash_by_name(v) {
                Some(h) => cfg_hash = Some(h),
                None => eprintln!("warning: unknown hash algorithm '{v}'"),
            }
        }
    }

    // --ref-format / GIT_DEFAULT_REF_FORMAT / init.defaultRefFormat /
    // feature.experimental.
    let mut cli_ref = None;
    if let Some(v) = &parsed.ref_format {
        if !v.is_empty() {
            cli_ref = Some(ref_by_name(v).ok_or_else(|| {
                CommandError::fatal(format!("fatal: unknown ref storage format '{v}'"))
            })?);
        }
    }
    let mut env_ref = None;
    if let Some(v) = std::env::var_os("GIT_DEFAULT_REF_FORMAT") {
        let v = v.to_string_lossy().into_owned();
        if !v.is_empty() {
            env_ref = Some(ref_by_name(&v).ok_or_else(|| {
                CommandError::fatal(format!("fatal: unknown ref storage format '{v}'"))
            })?);
        }
    }
    let mut cfg_ref = None;
    if let Some(v) = base.get("init", "defaultrefformat") {
        if !v.is_empty() {
            match ref_by_name(v) {
                Some(r) => cfg_ref = Some(r),
                None => eprintln!("warning: unknown ref storage format '{v}'"),
            }
        }
    } else if base.get_bool("feature", "experimental").unwrap_or(false) {
        cfg_ref = Some(RefFormat::Reftable);
    }

    // Existing repo format (reinit).
    let (mut algo, mut refmt) = (HashAlgo::Sha1, RefFormat::Files);
    let mut have_existing = false;
    if let Some(ex) = existing {
        if ex.get("core", "repositoryformatversion").is_some() {
            have_existing = true;
            if ex.get("extensions", "objectformat").map(|v| v == "sha256").unwrap_or(false) {
                algo = HashAlgo::Sha256;
            }
            if ex.get("extensions", "refstorage").map(|v| v == "reftable").unwrap_or(false) {
                refmt = RefFormat::Reftable;
            }
        }
    }

    if reinit && have_existing {
        if let Some(h) = cli_hash {
            if h != algo {
                return Err(CommandError::fatal(
                    "fatal: attempt to reinitialize repository with different hash".to_string(),
                ));
            }
        }
        if let Some(r) = cli_ref {
            if r != refmt {
                return Err(CommandError::fatal(
                    "fatal: attempt to reinitialize repository with different reference storage format"
                        .to_string(),
                ));
            }
        }
    } else {
        if let Some(h) = cli_hash {
            algo = h;
        } else if let Some(h) = env_hash {
            algo = h;
        } else if let Some(h) = cfg_hash {
            algo = h;
        }
        if let Some(r) = cli_ref {
            refmt = r;
        } else if let Some(r) = env_ref {
            refmt = r;
        } else if let Some(r) = cfg_ref {
            refmt = r;
        }
    }
    Ok((algo, refmt))
}

fn hash_by_name(name: &str) -> Option<HashAlgo> {
    match name {
        "sha1" => Some(HashAlgo::Sha1),
        "sha256" => Some(HashAlgo::Sha256),
        _ => None,
    }
}

fn ref_by_name(name: &str) -> Option<RefFormat> {
    match name {
        "files" => Some(RefFormat::Files),
        "reftable" => Some(RefFormat::Reftable),
        _ => None,
    }
}

/// C `git_config_perm()` for `core.sharedRepository` values from config.
fn config_perm(value: &str) -> Result<SharedVal, ()> {
    if value == "umask" {
        return Ok(SharedVal::Mode(0));
    }
    if value == "group" {
        return Ok(SharedVal::Group);
    }
    if value == "all" || value == "world" || value == "everybody" {
        return Ok(SharedVal::Everybody);
    }
    if !value.is_empty() && value.chars().all(|c| ('0'..='7').contains(&c)) {
        match u32::from_str_radix(value, 8).unwrap_or(0) {
            0 => return Ok(SharedVal::Mode(0)),
            1 => return Ok(SharedVal::Group),
            2 => return Ok(SharedVal::Everybody),
            m => return Ok(SharedVal::Mode(m & 0o666)),
        }
    }
    match git_config::parse_bool(value) {
        Some(true) => Ok(SharedVal::Group),
        Some(false) => Ok(SharedVal::Mode(0)),
        None => Err(()),
    }
}

/// Template dir: `--template` (against the invoking directory, like C
/// which absolutizes it before chdir) > `$GIT_TEMPLATE_DIR` >
/// `init.templatedir` (both used as-is, i.e. relative to the target
/// directory after the conceptual chdir) > compiled default.
/// `None` means "copy nothing".
fn resolve_template_dir(
    parsed: &InitArgs,
    base: &git_config::ConfigSet,
    ctx_cwd: &Path,
    work_base: &Path,
) -> Option<PathBuf> {
    match &parsed.template {
        TemplateOpt::Skip => None,
        TemplateOpt::Dir(d) => Some(absolutize(ctx_cwd, Path::new(d))),
        TemplateOpt::Default => {
            if let Some(env) = std::env::var_os("GIT_TEMPLATE_DIR") {
                let s = env.to_string_lossy().into_owned();
                if s.is_empty() {
                    return None;
                }
                return Some(absolutize(work_base, Path::new(&s)));
            }
            if let Some(v) = base.get("init", "templatedir") {
                if v.is_empty() {
                    return None;
                }
                return Some(absolutize(work_base, &expand_tilde(v, work_base)));
            }
            Some(default_template_dir())
        }
    }
}

fn default_template_dir() -> PathBuf {
    // Next to the installed git-core templates; mirrors C's compiled
    // `DEFAULT_GIT_TEMPLATE_DIR` with a few well-known fallbacks.
    if let Ok(exe) = std::env::current_exe() {
        // <prefix>/libexec/git-core/git -> <prefix>/share/git-core/templates
        if let Some(dir) = exe.parent() {
            let cand = dir.join("../share/git-core/templates");
            if cand.is_dir() {
                return cand;
            }
        }
    }
    for cand in [
        // Platform default first so dev builds agree with the system git
        // (Apple CLT git keeps its templates outside the exec prefix).
        "/Library/Developer/CommandLineTools/usr/share/git-core/templates",
        "/usr/share/git-core/templates",
        "/usr/local/share/git-core/templates",
        "/opt/homebrew/share/git-core/templates",
    ] {
        if Path::new(cand).is_dir() {
            return PathBuf::from(cand);
        }
    }
    PathBuf::from("/usr/share/git-core/templates")
}

/// Recursively copy templates (skip dotfiles, never overwrite), like
/// `copy_templates_1()`. Warns and continues when the dir is missing or has
/// an unusable repository format.
fn copy_templates(git_dir: &Path, template_dir: &Path) -> Result<(), CommandError> {
    let dir = match std::fs::read_dir(template_dir) {
        Ok(d) => d,
        Err(_) => {
            eprintln!("warning: templates not found in {}", template_dir.display());
            return Ok(());
        }
    };
    // Vintage check: a template config advertising an unknown format (or
    // extensions under version 0) invalidates the whole template dir.
    let tpl_config = template_dir.join("config");
    if tpl_config.is_file() {
        if let Ok(data) = std::fs::read(&tpl_config) {
            let cf = ConfigFile::parse(&data);
            let version = cf.get("core", "repositoryformatversion");
            let has_ext = cf.get("extensions", "objectformat").is_some()
                || cf.get("extensions", "refstorage").is_some()
                || cf.get("extensions", "submodulepathconfig").is_some();
            let bad = match version {
                None => false,
                Some(v) => v != "0" && v != "1" || (v == "0" && has_ext),
            };
            if bad {
                eprintln!(
                    "warning: not copying templates from '{}': unknown repository format",
                    template_dir.display()
                );
                return Ok(());
            }
        }
    }
    copy_templates_1(git_dir, template_dir, dir)?;
    Ok(())
}

fn copy_templates_1(
    dest_dir: &Path,
    src_dir: &Path,
    dir: std::fs::ReadDir,
) -> Result<(), CommandError> {
    std::fs::create_dir_all(dest_dir)
        .map_err(|e| CommandError::fatal(format!("fatal: {}", io_strerror(&e))))?;
    for entry in dir.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        let src = src_dir.join(name.as_ref());
        let dest = dest_dir.join(name.as_ref());
        let Ok(meta) = std::fs::symlink_metadata(&src) else { continue };
        if meta.is_dir() {
            let sub = match std::fs::read_dir(&src) {
                Ok(d) => d,
                Err(e) => {
                    return Err(CommandError::fatal(format!(
                        "fatal: cannot opendir '{}': {}",
                        src.display(),
                        io_strerror(&e)
                    )));
                }
            };
            copy_templates_1(&dest, &src, sub)?;
        } else if dest.symlink_metadata().is_ok() {
            // Never overwrite existing files (re-init keeps them).
            continue;
        } else if meta.file_type().is_symlink() {
            if let Ok(link) = std::fs::read_link(&src) {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::symlink;
                    let _ = symlink(&link, &dest);
                }
            }
        } else if meta.is_file() {
            match std::fs::copy(&src, &dest) {
                Ok(_) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let mode = meta.permissions().mode() & 0o777;
                        let _ = std::fs::set_permissions(
                            &dest,
                            std::fs::Permissions::from_mode(mode),
                        );
                    }
                }
                Err(e) => {
                    return Err(CommandError::fatal(format!(
                        "fatal: cannot copy '{}' to '{}': {}",
                        src.display(),
                        dest.display(),
                        io_strerror(&e)
                    )));
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Minimal config-file surgery preserving untouched content.
// ---------------------------------------------------------------------------

/// A parsed config file that remembers raw lines so untouched keys,
/// comments, and blank lines survive byte-for-byte.
#[derive(Debug, Clone, Default)]
struct ConfigFile {
    lines: Vec<String>,
}

#[derive(Debug, Clone)]
struct FoundKey {
    line: usize,
    indent: String,
    key: String,
}

impl ConfigFile {
    fn parse(data: &[u8]) -> ConfigFile {
        let text = String::from_utf8_lossy(data);
        ConfigFile { lines: text.lines().map(|l| l.to_string()).collect() }
    }

    fn find_key(&self, section: &str, subsection: Option<&str>, key: &str) -> Option<FoundKey> {
        let mut cur: Option<(String, Option<String>)> = None;
        for (i, l) in self.lines.iter().enumerate() {
            let t = l.trim();
            if t.starts_with('[') {
                if let Some(end) = t.find(']') {
                    cur = Some(split_section_header(t[1..end].trim()));
                } else {
                    cur = None;
                }
                continue;
            }
            if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
                continue;
            }
            if let Some((sec, sub)) = &cur {
                if sec == section && sub.as_deref() == subsection {
                    let (k, _) = split_kv(t);
                    if k == key {
                        let indent: String =
                            l.chars().take_while(|c| c.is_whitespace()).collect();
                        return Some(FoundKey { line: i, indent, key: k });
                    }
                }
            }
        }
        None
    }

    fn get(&self, section: &str, key: &str) -> Option<String> {
        self.get_in(section, None, key)
    }

    fn get_in(&self, section: &str, subsection: Option<&str>, key: &str) -> Option<String> {
        let found = self.find_key(section, subsection, key)?;
        self.lines.get(found.line).and_then(|l| {
            let (_, v) = split_kv(l.trim());
            Some(unquote(&v))
        })
    }

    fn set(&mut self, section: &str, subsection: Option<&str>, key: &str, value: &str) {
        let qv = quote_value(value);
        if let Some(found) = self.find_key(section, subsection, key) {
            self.lines[found.line] = format!("{}{} = {}", found.indent, found.key, qv);
            return;
        }
        // Append to the section, or create it at the end of file.
        let header = match subsection {
            Some(s) => format!("[{section} \"{s}\"]"),
            None => format!("[{section}]"),
        };
        let insert_at: usize;
        let mut last_in_section = None;
        let mut in_section = false;
        for (i, l) in self.lines.iter().enumerate() {
            let t = l.trim();
            if t.starts_with('[') {
                if in_section {
                    break;
                }
                if let Some(end) = t.find(']') {
                    let (sec, sub) = split_section_header(t[1..end].trim());
                    if sec == section && sub.as_deref() == subsection {
                        in_section = true;
                    }
                }
                continue;
            }
            if in_section {
                last_in_section = Some(i);
            }
        }
        if in_section {
            insert_at = last_in_section.map(|i| i + 1).unwrap_or(self.lines.len());
        } else {
            self.lines.push(header);
            insert_at = self.lines.len();
        }
        self.lines.insert(insert_at, format!("\t{key} = {qv}"));
    }

    fn unset(&mut self, section: &str, subsection: Option<&str>, key: &str) {
        let mut remove = None;
        if let Some(found) = self.find_key(section, subsection, key) {
            remove = Some(found.line);
        }
        if let Some(i) = remove {
            self.lines.remove(i);
            // Drop the section header if it has no keys left.
            let mut j = i;
            while j > 0 {
                j -= 1;
                let t = self.lines.get(j).map(|l| l.trim().to_string()).unwrap_or_default();
                if t.starts_with('[') {
                    let mut has_keys = false;
                    let mut k = j + 1;
                    while k < self.lines.len() {
                        let u = self.lines[k].trim();
                        if u.starts_with('[') {
                            break;
                        }
                        if !u.is_empty() && !u.starts_with('#') && !u.starts_with(';') {
                            has_keys = true;
                            break;
                        }
                        k += 1;
                    }
                    if !has_keys {
                        self.lines.remove(j);
                    }
                    break;
                }
            }
        }
    }

    fn serialize(&self) -> String {
        let mut s = self.lines.join("\n");
        s.push('\n');
        s
    }
}

fn split_section_header(inner: &str) -> (String, Option<String>) {
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

fn split_kv(trimmed: &str) -> (String, String) {
    match trimmed.find('=') {
        Some(i) => (trimmed[..i].trim().to_ascii_lowercase(), trimmed[i + 1..].trim().to_string()),
        None => match trimmed.split_once(char::is_whitespace) {
            Some((k, v)) => (k.trim().to_ascii_lowercase(), v.trim().to_string()),
            None => (trimmed.trim().to_ascii_lowercase(), String::new()),
        },
    }
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        let inner = &v[1..v.len() - 1];
        let mut out = String::new();
        let mut it = inner.chars();
        while let Some(c) = it.next() {
            if c == '\\' {
                match it.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(x) => out.push(x),
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        return out;
    }
    // Strip a trailing comment outside quotes (mirrors the reader).
    match v.find(" ;").or_else(|| v.find(" #")).or_else(|| v.find('\t')) {
        _ => v.to_string(),
    }
}

fn quote_value(v: &str) -> String {
    if v.is_empty()
        || v.starts_with(char::is_whitespace)
        || v.ends_with(char::is_whitespace)
        || v.contains('"')
        || v.contains('\\')
        || v.contains(';')
        || v.contains('#')
    {
        let mut q = String::from("\"");
        for c in v.chars() {
            match c {
                '"' | '\\' => {
                    q.push('\\');
                    q.push(c);
                }
                '\n' => q.push_str("\\n"),
                '\t' => q.push_str("\\t"),
                _ => q.push(c),
            }
        }
        q.push('"');
        q
    } else {
        v.to_string()
    }
}

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn write_config(
    git_dir: &Path,
    existing: Option<&ConfigFile>,
    reinit: bool,
    hash_algo: HashAlgo,
    ref_format: RefFormat,
    bare: bool,
    work_tree: &Option<PathBuf>,
    original_git_dir: &Path,
    shared: Option<SharedVal>,
    template_has: impl Fn((&str, &str)) -> bool,
    submodule_path_config: bool,
) -> Result<(), CommandError> {
    let mut cfg = existing.cloned().unwrap_or_default();

    let version_1 = hash_algo != HashAlgo::Sha1 || ref_format != RefFormat::Files;
    if version_1 {
        if hash_algo != HashAlgo::Sha1 {
            cfg.set("extensions", None, "objectformat", "sha256");
        } else if reinit {
            cfg.unset("extensions", None, "objectformat");
        }
        if ref_format != RefFormat::Files {
            cfg.set("extensions", None, "refstorage", "reftable");
        } else if reinit {
            cfg.unset("extensions", None, "refstorage");
        }
    } else if reinit {
        cfg.unset("extensions", None, "objectformat");
        cfg.unset("extensions", None, "refstorage");
    }
    if submodule_path_config {
        cfg.set("extensions", None, "submodulepathconfig", "true");
    }
    cfg.set("core", None, "repositoryformatversion", if version_1 { "1" } else { "0" });

    // Fresh repos probe the filesystem; reinits keep probed values.
    if !reinit {
        let config_path = git_dir.join("config");
        let _ = std::fs::write(&config_path, cfg.serialize())
            .map_err(|e| CommandError::fatal(format!("fatal: {}", io_strerror(&e))));
    }

    // filemode probe (runs on reinit too, like C).
    {
        let config_path = git_dir.join("config");
        let filemode = match std::fs::symlink_metadata(&config_path) {
            Ok(md) => probe_filemode(&config_path, &md, reinit),
            Err(_) => false,
        };
        cfg.set("core", None, "filemode", if filemode { "true" } else { "false" });
    }

    cfg.set("core", None, "bare", if bare { "true" } else { "false" });
    if !bare && !template_has(("core", "logallrefupdates")) {
        cfg.set("core", None, "logallrefupdates", "true");
    }
    if !bare {
        if let (Some(wt), true) = (work_tree, needs_work_tree_config(original_git_dir, work_tree)) {
            cfg.set("core", None, "worktree", &wt.to_string_lossy());
        }
    }

    if !reinit {
        // Symlink support probe.
        let probe = git_dir.join(format!(".probe-symlink-{}", std::process::id()));
        let supported = create_probe_file(&probe)
            && std::os::unix::fs::symlink("testing", &probe).is_ok()
            && std::fs::symlink_metadata(&probe)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
        let _ = std::fs::remove_file(&probe);
        if !supported {
            cfg.set("core", None, "symlinks", "false");
        }
        // Case-insensitivity probe.
        if git_dir.join("CoNfIg").exists() {
            cfg.set("core", None, "ignorecase", "true");
        }
        // UTF-8 composition probe.
        probe_precompose(git_dir, &mut cfg);
    }

    if let Some(s) = shared {
        if s.is_shared() {
            cfg.set("core", None, "sharedrepository", &s.config_value());
            cfg.set("receive", None, "denyNonFastforwards", "true");
        }
    }

    std::fs::write(git_dir.join("config"), cfg.serialize())
        .map_err(|e| CommandError::fatal(format!("fatal: {}", io_strerror(&e))))?;
    Ok(())
}

#[cfg(unix)]
fn probe_filemode(config_path: &Path, md: &std::fs::Metadata, reinit: bool) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let mode = md.permissions().mode();
    let toggled = mode ^ 0o100;
    if std::fs::set_permissions(config_path, std::fs::Permissions::from_mode(toggled)).is_err() {
        return false;
    }
    let changed = std::fs::symlink_metadata(config_path)
        .map(|m| m.permissions().mode() != mode)
        .unwrap_or(false);
    let _ = std::fs::set_permissions(config_path, std::fs::Permissions::from_mode(mode));
    if changed && !reinit && (mode & 0o100) != 0 {
        return false;
    }
    changed
}

#[cfg(not(unix))]
fn probe_filemode(_config_path: &Path, _md: &std::fs::Metadata, _reinit: bool) -> bool {
    false
}

fn create_probe_file(p: &Path) -> bool {
    // xmkstemp equivalent: prove we can create a fresh file (drop stale
    // leftovers first), then remove it so the caller can symlink the name.
    let _ = std::fs::remove_file(p);
    if std::fs::OpenOptions::new().write(true).create_new(true).open(p).is_err() {
        return false;
    }
    std::fs::remove_file(p).is_ok()
}

fn probe_precompose(git_dir: &Path, cfg: &mut ConfigFile) {
    let nfc = git_dir.join("ä");
    let nfd = git_dir.join("a\u{308}");
    // Clean leftovers, then create the NFC name and test the NFD name.
    let _ = std::fs::remove_file(&nfc);
    let _ = std::fs::remove_file(&nfd);
    if std::fs::OpenOptions::new().write(true).create_new(true).open(&nfc).is_ok() {
        let composed = nfd.exists();
        cfg.set("core", None, "precomposeunicode", if composed { "true" } else { "false" });
        let _ = std::fs::remove_file(&nfc);
        let _ = std::fs::remove_file(&nfd);
    }
}

/// Whether `core.worktree` must be recorded (git dir not `<worktree>/.git`).
fn needs_work_tree_config(git_dir: &Path, work_tree: &Option<PathBuf>) -> bool {
    let wt = match work_tree {
        Some(w) => w,
        None => return false,
    };
    if wt == Path::new("/") && git_dir == Path::new("/.git") {
        return false;
    }
    match git_dir.strip_prefix(wt) {
        Ok(rest) => rest != Path::new(".git"),
        Err(_) => true,
    }
}

/// `git init --separate-git-dir`: move an existing git dir aside (re-init to
/// move), then write the `gitdir:` link.
fn separate_git_dir(original_git_dir: &Path, real_git_dir: &Path) -> Result<(), CommandError> {
    if let Ok(md) = std::fs::symlink_metadata(original_git_dir) {
        let src = if md.file_type().is_symlink() || md.is_file() {
            match read_gitfile(original_git_dir) {
                Some(p) => {
                    if p.is_absolute() {
                        p
                    } else {
                        original_git_dir.parent().unwrap_or(Path::new(".")).join(p)
                    }
                }
                None => original_git_dir.to_path_buf(),
            }
        } else {
            original_git_dir.to_path_buf()
        };
        if src != *real_git_dir {
            std::fs::rename(&src, real_git_dir).map_err(|e| {
                CommandError::fatal(format!("fatal: unable to move {} to {}: {}", src.display(), real_git_dir.display(), io_strerror(&e)))
            })?;
        }
    }
    std::fs::create_dir_all(real_git_dir)
        .map_err(|e| CommandError::fatal(format!("fatal: {}", io_strerror(&e))))?;
    // The link records the physical path, like C's real_pathdup().
    let real_display = canonicalize(real_git_dir).unwrap_or_else(|_| real_git_dir.to_path_buf());
    if let Some(parent) = original_git_dir.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CommandError::fatal(format!("fatal: {}", io_strerror(&e))))?;
        }
    }
    std::fs::write(original_git_dir, format!("gitdir: {}\n", real_display.display()))
        .map_err(|e| CommandError::fatal(format!("fatal: {}", io_strerror(&e))))?;
    Ok(())
}

/// Best-effort `adjust_shared_perm()`: group-writable dirs (setgid) and
/// group-readable files inside the git dir.
fn adjust_shared_perm(git_dir: &Path, shared: SharedVal) {
    let (file_bits, dir_bits) = match shared {
        SharedVal::Group => (0o060, 0o070),
        SharedVal::Everybody => (0o064, 0o075),
        SharedVal::Mode(0) => return,
        SharedVal::Mode(m) => (m & 0o077, m & 0o077),
    };
    let mut stack = vec![git_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
            if md.file_type().is_symlink() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if md.is_dir() {
                    let mode = md.permissions().mode();
                    let new = (mode & 0o700) | dir_bits | 0o2000;
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(new));
                    stack.push(path);
                } else {
                    let mode = md.permissions().mode();
                    let new = (mode & 0o700) | file_bits;
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(new));
                }
            }
        }
    }
    // The git dir itself.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(md) = std::fs::symlink_metadata(git_dir) {
            let mode = md.permissions().mode();
            let new = (mode & 0o700) | dir_bits | 0o2000;
            let _ = std::fs::set_permissions(git_dir, std::fs::Permissions::from_mode(new));
        }
    }
}

/// The default initial branch: `$GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME` >
/// `init.defaultBranch` > `master` with the unconfigured-branch advice.
fn default_branch_name(
    base: &git_config::ConfigSet,
    quiet: bool,
) -> Result<String, CommandError> {
    if let Some(env) = std::env::var_os("GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME") {
        let s = env.to_string_lossy().into_owned();
        if !s.is_empty() {
            let full = format!("refs/heads/{s}");
            git_refs::validate_refname(&full).map_err(|_| {
                CommandError::fatal(format!("fatal: invalid branch name: init.defaultBranch = {s}"))
            })?;
            return Ok(s);
        }
    }
    if let Some(name) = base.get("init", "defaultbranch") {
        let full = format!("refs/heads/{name}");
        git_refs::validate_refname(&full).map_err(|_| {
            CommandError::fatal(format!("fatal: invalid branch name: init.defaultBranch = {name}"))
        })?;
        return Ok(name.to_string());
    }
    if !quiet
        && base
            .get("advice", "defaultbranchname")
            .and_then(git_config::parse_bool)
            .unwrap_or(true)
    {
        for line in DEFAULT_BRANCH_ADVICE.lines() {
            if line.is_empty() {
                eprintln!("hint:");
            } else {
                eprintln!("hint: {line}");
            }
        }
    }
    Ok("master".to_string())
}

const DEFAULT_BRANCH_ADVICE: &str = "Using 'master' as the name for the initial branch. This default branch name\nis subject to change. To configure the initial branch name to use in all\nof your new repositories, which will suppress this warning, call:\n\n\tgit config --global init.defaultBranch <name>\n\nNames commonly chosen instead of 'master' are 'main', 'trunk' and\n'development'. The just-created branch can be renamed via this command:\n\n\tgit branch -m <name>\n\nDisable this message with \"git config set advice.defaultBranchName false\"";

/// Make `path` absolute against `base` with lexical `.`/`..` normalization.
fn absolutize(base: &Path, path: &Path) -> PathBuf {
    let joined = if path.is_absolute() { path.to_path_buf() } else { base.join(path) };
    let mut out = PathBuf::new();
    for comp in joined.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        out
    }
}

fn canonicalize(p: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(p)
}

fn io_strerror(e: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::NotFound => "No such file or directory".to_string(),
        ErrorKind::PermissionDenied => "Permission denied".to_string(),
        ErrorKind::AlreadyExists => "File exists".to_string(),
        ErrorKind::NotADirectory => "Not a directory".to_string(),
        ErrorKind::IsADirectory => "Is a directory".to_string(),
        _ => {
            let s = e.to_string();
            // Strip Rust's " (os error N)" suffix to match strerror text.
            match s.rfind(" (os error ") {
                Some(i) => s[..i].to_string(),
                None => s,
            }
        }
    }
}
