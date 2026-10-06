//! `git config`: the full C option matrix (D-09, D-10, D-11, D-15).
//!
//! A port of `builtin/config.c` (both the legacy flag spellings and the
//! `get/set/unset/list/rename-section/remove-section/edit` subcommand
//! spellings, per the `t/t1300-config.sh` mode loop) over the plan 01-02
//! engine (`ConfigSet` scope loader, `includeIf` matcher, typed
//! canonicalizer, layout-preserving `file.rs` splicer). Every user-facing
//! string and exit code below was captured from the tree C binary
//! (2.55.0.552); `t/t1300`, `t/t1305`, `t/t1308` are the oracles.
//!
//! Blob write and editor spawn are out of scope per O4 (blob *read* stays
//! in): `edit` and blob writes die with C's exact texts.
//!
//! Threat mitigations (plan threat model): T-01-16 (system-scope writes
//! require the explicit flag and go through the dot-lock splicer, never a
//! shell), T-01-17 (urlmatch normalization ported exactly, credential
//! differences preserved), T-01-18 (origins emitted C-exactly by design),
//! T-01-19 (no hooks executed; temp-dir fixtures in tests).

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::{Command, CommandError, RepoContext};
use git_config::{
    canonicalize_typed, file as cfgfile, parse_bool, parse_bool_text, parse_key_name, ConfigEntry,
    ConfigError, ConfigScope, ConfigSet, ConfigValueType, IncludeContext,
};

// C `CONFIG_FLAGS_*` bitfield (builtin/config.c).
const CONFIG_FLAGS_FIXED_VALUE: u32 = 1 << 0;
const CONFIG_FLAGS_MULTI_REPLACE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Usage texts (byte-exact, captured from the tree binary)
// ---------------------------------------------------------------------------

const LOCATION_HELP: &str = "Config file location
    --[no-]global         use global config file
    --[no-]system         use system config file
    --[no-]local          use repository config file
    --[no-]worktree       use per-worktree config file
    -f, --[no-]file <file>
                          use given config file
    --[no-]blob <blob-id> read config from given blob object
";

const DISPLAY_HELP: &str = "Display options
    -z, --[no-]null       terminate values with NUL byte
    --[no-]name-only      show variable names only
    --[no-]show-origin    show origin of config (file, standard input, blob, command line)
    --[no-]show-scope     show scope of config (worktree, local, global, system, command)
    --[no-]show-names     show config keys in addition to their values

Type
    -t, --[no-]type <type>
                          value is given this type
    --bool                value is \"true\" or \"false\"
    --int                 value is decimal number
    --bool-or-int         value is --bool or --int
    --bool-or-str         value is --bool or string
    --path                value is a path (file or directory name)
    --expiry-date         value is an expiry date
";

const LEGACY_USAGE: &str = "usage: git config list [<file-option>] [<display-option>] [--includes]
   or: git config get [<file-option>] [<display-option>] [--includes] [--all] [--regexp] [--value=<pattern>] [--fixed-value] [--default=<default>] [--url=<url>] <name>
   or: git config set [<file-option>] [--type=<type>] [--all] [--value=<pattern>] [--fixed-value] <name> <value>
   or: git config unset [<file-option>] [--all] [--value=<pattern>] [--fixed-value] <name>
   or: git config rename-section [<file-option>] <old-name> <new-name>
   or: git config remove-section [<file-option>] <name>
   or: git config edit [<file-option>]
   or: git config [<file-option>] --get-colorbool <name> [<stdout-is-tty>]

Config file location
    --[no-]global         use global config file
    --[no-]system         use system config file
    --[no-]local          use repository config file
    --[no-]worktree       use per-worktree config file
    -f, --[no-]file <file>
                          use given config file
    --[no-]blob <blob-id> read config from given blob object

Action
    --get                 get value: name [<value-pattern>]
    --get-all             get all values: key [<value-pattern>]
    --get-regexp          get values for regexp: name-regex [<value-pattern>]
    --get-urlmatch        get value specific for the URL: section[.var] URL
    --replace-all         replace all matching variables: name value [<value-pattern>]
    --add                 add a new variable: name value
    --unset               remove a variable: name [<value-pattern>]
    --unset-all           remove all matches: name [<value-pattern>]
    --rename-section      rename section: old-name new-name
    --remove-section      remove a section: name
    -l, --list            list all
    -e, --edit            open an editor
    --get-color           find the color configured: slot [<default>]
    --get-colorbool       find the color setting: slot [<stdout-is-tty>]

Display options
    -z, --[no-]null       terminate values with NUL byte
    --[no-]name-only      show variable names only
    --[no-]show-origin    show origin of config (file, standard input, blob, command line)
    --[no-]show-scope     show scope of config (worktree, local, global, system, command)
    --[no-]show-names     show config keys in addition to their values

Type
    -t, --[no-]type <type>
                          value is given this type
    --bool                value is \"true\" or \"false\"
    --int                 value is decimal number
    --bool-or-int         value is --bool or --int
    --bool-or-str         value is --bool or string
    --path                value is a path (file or directory name)
    --expiry-date         value is an expiry date

Other
    --[no-]default <value>
                          with --get, use default value when missing entry
    --[no-]comment <value>
                          human-readable comment string (# will be prepended as needed)
    --[no-]fixed-value    use string equality when comparing values to value pattern
    --[no-]includes       respect include directives on lookup";

const LIST_USAGE: &str = "usage: git config list [<file-option>] [<display-option>] [--includes]

Config file location
    --[no-]global         use global config file
    --[no-]system         use system config file
    --[no-]local          use repository config file
    --[no-]worktree       use per-worktree config file
    -f, --[no-]file <file>
                          use given config file
    --[no-]blob <blob-id> read config from given blob object

Display options
    -z, --[no-]null       terminate values with NUL byte
    --[no-]name-only      show variable names only
    --[no-]show-origin    show origin of config (file, standard input, blob, command line)
    --[no-]show-scope     show scope of config (worktree, local, global, system, command)
    --[no-]show-names     show config keys in addition to their values

Type
    -t, --[no-]type <type>
                          value is given this type
    --bool                value is \"true\" or \"false\"
    --int                 value is decimal number
    --bool-or-int         value is --bool or --int
    --bool-or-str         value is --bool or string
    --path                value is a path (file or directory name)
    --expiry-date         value is an expiry date

Other
    --[no-]includes       respect include directives on lookup";

const GET_USAGE: &str = "usage: git config get [<file-option>] [<display-option>] [--includes] [--all] [--regexp=<regexp>] [--value=<pattern>] [--fixed-value] [--default=<default>] <name>

Config file location
    --[no-]global         use global config file
    --[no-]system         use system config file
    --[no-]local          use repository config file
    --[no-]worktree       use per-worktree config file
    -f, --[no-]file <file>
                          use given config file
    --[no-]blob <blob-id> read config from given blob object

Filter options
    --[no-]all            return all values for multi-valued config options
    --[no-]regexp         interpret the name as a regular expression
    --[no-]value <pattern>
                          show config with values matching the pattern
    --[no-]fixed-value    use string equality when comparing values to value pattern
    --[no-]url <URL>      show config matching the given URL

Display options
    -z, --[no-]null       terminate values with NUL byte
    --[no-]name-only      show variable names only
    --[no-]show-origin    show origin of config (file, standard input, blob, command line)
    --[no-]show-scope     show scope of config (worktree, local, global, system, command)
    --[no-]show-names     show config keys in addition to their values

Type
    -t, --[no-]type <type>
                          value is given this type
    --bool                value is \"true\" or \"false\"
    --int                 value is decimal number
    --bool-or-int         value is --bool or --int
    --bool-or-str         value is --bool or string
    --path                value is a path (file or directory name)
    --expiry-date         value is an expiry date

Other
    --[no-]includes       respect include directives on lookup
    --[no-]default <value>
                          use default value when missing entry";

const SET_USAGE: &str = "usage: git config set [<file-option>] [--type=<type>] [--comment=<message>] [--all] [--value=<pattern>] [--fixed-value] <name> <value>

Config file location
    --[no-]global         use global config file
    --[no-]system         use system config file
    --[no-]local          use repository config file
    --[no-]worktree       use per-worktree config file
    -f, --[no-]file <file>
                          use given config file
    --[no-]blob <blob-id> read config from given blob object

Type
    -t, --[no-]type <type>
                          value is given this type
    --bool                value is \"true\" or \"false\"
    --int                 value is decimal number
    --bool-or-int         value is --bool or --int
    --bool-or-str         value is --bool or string
    --path                value is a path (file or directory name)
    --expiry-date         value is an expiry date

Filter
    --[no-]all            replace multi-valued config option with new value
    --[no-]value <pattern>
                          show config with values matching the pattern
    --[no-]fixed-value    use string equality when comparing values to value pattern

Other
    --[no-]comment <value>
                          human-readable comment string (# will be prepended as needed)
    --[no-]append         add a new line without altering any existing values";

const UNSET_USAGE: &str = "usage: git config unset [<file-option>] [--all] [--value=<pattern>] [--fixed-value] <name>

Config file location
    --[no-]global         use global config file
    --[no-]system         use system config file
    --[no-]local          use repository config file
    --[no-]worktree       use per-worktree config file
    -f, --[no-]file <file>
                          use given config file
    --[no-]blob <blob-id> read config from given blob object

Filter
    --[no-]all            unset all multi-valued config options
    --[no-]value <pattern>
                          unset multi-valued config options with matching values
    --[no-]fixed-value    use string equality when comparing values to value pattern";

const RENAME_SECTION_USAGE: &str = "usage: git config rename-section [<file-option>] <old-name> <new-name>

Config file location
    --[no-]global         use global config file
    --[no-]system         use system config file
    --[no-]local          use repository config file
    --[no-]worktree       use per-worktree config file
    -f, --[no-]file <file>
                          use given config file
    --[no-]blob <blob-id> read config from given blob object";

const REMOVE_SECTION_USAGE: &str = "usage: git config remove-section [<file-option>] <name>

Config file location
    --[no-]global         use global config file
    --[no-]system         use system config file
    --[no-]local          use repository config file
    --[no-]worktree       use per-worktree config file
    -f, --[no-]file <file>
                          use given config file
    --[no-]blob <blob-id> read config from given blob object";

const EDIT_USAGE: &str = "usage: git config edit [<file-option>]

Config file location
    --[no-]global         use global config file
    --[no-]system         use system config file
    --[no-]local          use repository config file
    --[no-]worktree       use per-worktree config file
    -f, --[no-]file <file>
                          use given config file
    --[no-]blob <blob-id> read config from given blob object";

// ---------------------------------------------------------------------------
// Parsed options
// ---------------------------------------------------------------------------

/// Value types for `--type=` (C `builtin/config.c` `option_parse_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliType {
    Bool,
    Int,
    BoolOrInt,
    BoolOrStr,
    Path,
    ExpiryDate,
    Color,
}

fn parse_type_name(s: &str) -> Option<CliType> {
    match s {
        "bool" => Some(CliType::Bool),
        "int" => Some(CliType::Int),
        "bool-or-int" => Some(CliType::BoolOrInt),
        "bool-or-str" => Some(CliType::BoolOrStr),
        "path" => Some(CliType::Path),
        "expiry-date" => Some(CliType::ExpiryDate),
        "color" => Some(CliType::Color),
        _ => None,
    }
}

/// Location selectors (C `config_location_options`).
#[derive(Debug, Default)]
struct LocOpts {
    global: bool,
    system: bool,
    local: bool,
    worktree: bool,
    file: Option<String>,
    blob: Option<String>,
    /// `None` = default (`!source.file`), `Some` = `--[no-]includes`.
    includes: Option<bool>,
}

/// Display options (C `config_display_options`).
#[derive(Debug)]
struct DispOpts {
    end_nul: bool,
    omit_values: bool,
    show_origin: bool,
    show_scope: bool,
    show_keys: bool,
    cli_type: Option<CliType>,
    default_value: Option<String>,
}

impl Default for DispOpts {
    fn default() -> DispOpts {
        DispOpts {
            end_nul: false,
            omit_values: false,
            show_origin: false,
            show_scope: false,
            show_keys: false,
            cli_type: None,
            default_value: None,
        }
    }
}

/// Set the type, dying `only one type at a time` (exit 129) on conflict
/// (C `option_parse_type`).
fn set_cli_type(slot: &mut Option<CliType>, t: CliType) -> Result<(), CommandError> {
    if let Some(old) = *slot {
        if old != t {
            return Err(CommandError::usage("error: only one type at a time"));
        }
        return Ok(());
    }
    *slot = Some(t);
    Ok(())
}

/// A parse-options style error: `error: unknown option \`X'` plus the
/// subcommand usage block, exit 129.
fn unknown_option(usage: &'static str, opt: &str) -> CommandError {
    CommandError::usage(format!("error: unknown option `{opt}'\n{usage}"))
}

/// Parse one `--type`/`-t` value: `unrecognized --type argument` dies 128.
fn parse_type_value(v: &str) -> Result<CliType, CommandError> {
    parse_type_name(v).ok_or_else(|| CommandError::fatal(format!("fatal: unrecognized --type argument, {v}")))
}

/// Long-option matching with `--no-` negation support. Returns the
/// canonical name and whether it was negated.
fn split_long(arg: &str) -> Option<(&str, bool)> {
    let body = arg.strip_prefix("--")?;
    if body.is_empty() {
        return None;
    }
    match body.strip_prefix("no-") {
        Some(rest) if !rest.is_empty() => Some((rest, true)),
        _ => Some((body, false)),
    }
}

/// Apply one parsed location option. `usage` selects the error block.
fn apply_loc_opt(
    loc: &mut LocOpts,
    name: &str,
    negated: bool,
    value: Option<&str>,
    take_arg: &mut Option<String>,
    usage: &'static str,
) -> Result<(), CommandError> {
    // Options taking a separate `--opt value` or `--opt=value` argument.
    if take_arg.is_some() {
        return Err(unknown_option(usage, name));
    }
    match name {
        "global" if value.is_none() => loc.global = !negated,
        "system" if value.is_none() => loc.system = !negated,
        "local" if value.is_none() => loc.local = !negated,
        "worktree" if value.is_none() => loc.worktree = !negated,
        "file" => {
            if negated {
                loc.file = None;
            } else if let Some(v) = value {
                loc.file = Some(v.to_string());
            } else {
                *take_arg = Some("file".to_string());
            }
        }
        "blob" => {
            if negated {
                loc.blob = None;
            } else if let Some(v) = value {
                loc.blob = Some(v.to_string());
            } else {
                *take_arg = Some("blob".to_string());
            }
        }
        "includes" if value.is_none() => loc.includes = Some(!negated),
        _ => return Err(unknown_option(usage, name)),
    }
    Ok(())
}

/// Apply one parsed display option.
#[allow(clippy::too_many_arguments)]
fn apply_disp_opt(
    disp: &mut DispOpts,
    name: &str,
    negated: bool,
    value: Option<&str>,
    take_arg: &mut Option<String>,
    usage: &'static str,
    with_type_flags: bool,
    with_default: bool,
) -> Result<(), CommandError> {
    match name {
        "null" if value.is_none() => disp.end_nul = !negated,
        "name-only" if value.is_none() => disp.omit_values = !negated,
        "show-origin" if value.is_none() => disp.show_origin = !negated,
        "show-scope" if value.is_none() => disp.show_scope = !negated,
        "show-names" if value.is_none() => disp.show_keys = !negated,
        "type" => {
            if negated {
                disp.cli_type = None;
            } else if let Some(v) = value {
                set_cli_type(&mut disp.cli_type, parse_type_value(v)?)?;
            } else {
                *take_arg = Some("type".to_string());
            }
        }
        "bool" if with_type_flags && value.is_none() && !negated => {
            set_cli_type(&mut disp.cli_type, CliType::Bool)?;
        }
        "int" if with_type_flags && value.is_none() && !negated => {
            set_cli_type(&mut disp.cli_type, CliType::Int)?;
        }
        "bool-or-int" if with_type_flags && value.is_none() && !negated => {
            set_cli_type(&mut disp.cli_type, CliType::BoolOrInt)?;
        }
        "bool-or-str" if with_type_flags && value.is_none() && !negated => {
            set_cli_type(&mut disp.cli_type, CliType::BoolOrStr)?;
        }
        "path" if with_type_flags && value.is_none() && !negated => {
            set_cli_type(&mut disp.cli_type, CliType::Path)?;
        }
        "expiry-date" if with_type_flags && value.is_none() && !negated => {
            set_cli_type(&mut disp.cli_type, CliType::ExpiryDate)?;
        }
        "default" if with_default => {
            if negated {
                disp.default_value = None;
            } else if let Some(v) = value {
                disp.default_value = Some(v.to_string());
            } else {
                *take_arg = Some("default".to_string());
            }
        }
        _ => return Err(unknown_option(usage, name)),
    }
    Ok(())
}

/// Consume a pending `--opt value` argument. `value_slot`/`comment_slot`
/// receive the action-specific `--value`/`--comment` payloads.
fn take_pending(
    loc: &mut LocOpts,
    disp: &mut DispOpts,
    pending: &mut Option<String>,
    arg: &str,
    value_slot: &mut Option<String>,
    comment_slot: &mut Option<String>,
) -> Result<bool, CommandError> {
    let Some(which) = pending.take() else {
        return Ok(false);
    };
    match which.as_str() {
        "file" => loc.file = Some(arg.to_string()),
        "blob" => loc.blob = Some(arg.to_string()),
        "type" => set_cli_type(&mut disp.cli_type, parse_type_value(arg)?)?,
        "default" => disp.default_value = Some(arg.to_string()),
        "value" => *value_slot = Some(arg.to_string()),
        "comment" => *comment_slot = Some(arg.to_string()),
        _ => {}
    }
    Ok(true)
}

/// Full option parser shared by the legacy and subcommand spellings.
///
/// Handles location + display options, `-l/-e/-z/-f/-t` shorts (with
/// bundling for the boolean shorts), `--opt=value`, `--no-` negation, and
/// `--` termination with `PARSE_OPT_STOP_AT_NON_OPTION` semantics: the
/// first non-option ends option parsing. `extra` parses the remaining
/// action-specific flags; unknown options die with `usage`.
fn parse_opts(
    args: &[String],
    usage: &'static str,
    with_type_flags: bool,
    with_default: bool,
    value_slot: &mut Option<String>,
    comment_slot: &mut Option<String>,
    mut extra: impl FnMut(
        &mut LocOpts,
        &mut DispOpts,
        &str,
        bool,
        Option<&str>,
        &mut Option<String>,
    ) -> Result<bool, CommandError>,
) -> Result<(LocOpts, DispOpts, Vec<String>), CommandError> {
    let mut loc = LocOpts::default();
    let mut disp = DispOpts::default();
    let mut rest: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if take_pending(&mut loc, &mut disp, &mut pending, a, value_slot, comment_slot)? {
            i += 1;
            continue;
        }
        if a == "--" {
            rest.extend_from_slice(&args[i + 1..]);
            break;
        }
        if a == "-" || !a.starts_with('-') {
            rest.extend_from_slice(&args[i..]);
            break;
        }
        if let Some((name, negated)) = split_long(a) {
            let (opt, inline) = match name.split_once('=') {
                Some((o, v)) => (o, Some(v)),
                None => (name, None),
            };
            if apply_loc_opt(&mut loc, opt, negated, inline, &mut pending, usage).is_ok() {
                i += 1;
                continue;
            }
            if apply_disp_opt(&mut disp, opt, negated, inline, &mut pending, usage, with_type_flags, with_default)
                .is_ok()
            {
                i += 1;
                continue;
            }
            if extra(&mut loc, &mut disp, opt, negated, inline, &mut pending)? {
                i += 1;
                continue;
            }
            return Err(unknown_option(usage, opt));
        }
        // Short options (bundled booleans allowed).
        let shorts: Vec<char> = a[1..].chars().collect();
        let mut j = 0;
        let mut consumed = true;
        while j < shorts.len() {
            match shorts[j] {
                'l' => {
                    extra(&mut loc, &mut disp, "list-short", false, None, &mut pending)?;
                }
                'e' => {
                    extra(&mut loc, &mut disp, "edit-short", false, None, &mut pending)?;
                }
                'z' => {
                    disp.end_nul = true;
                }
                'f' => {
                    let rest_chars: String = shorts[j + 1..].iter().collect();
                    if !rest_chars.is_empty() {
                        loc.file = Some(rest_chars);
                    } else {
                        pending = Some("file".to_string());
                    }
                    break;
                }
                't' => {
                    let rest_chars: String = shorts[j + 1..].iter().collect();
                    if !rest_chars.is_empty() {
                        set_cli_type(&mut disp.cli_type, parse_type_value(&rest_chars)?)?;
                    } else {
                        pending = Some("type".to_string());
                    }
                    break;
                }
                _ => {
                    consumed = false;
                    break;
                }
            }
            j += 1;
        }
        if !consumed {
            return Err(unknown_option(usage, &a[1..]));
        }
        i += 1;
    }
    if pending.is_some() {
        // C reports a missing option argument with the usage block.
        return Err(unknown_option(usage, pending.as_deref().unwrap_or("")));
    }
    Ok((loc, disp, rest))
}

// ---------------------------------------------------------------------------
// Config sources: scope selectors → loaded entries with origin metadata
// ---------------------------------------------------------------------------

/// Where an entry was read from (C `config_origin_type`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum OriginType {
    File,
    Stdin,
    Blob,
    Cmdline,
}

impl OriginType {
    fn name(&self) -> &'static str {
        match self {
            OriginType::File => "file",
            OriginType::Stdin => "standard input",
            OriginType::Blob => "blob",
            OriginType::Cmdline => "command line",
        }
    }
}

/// One loaded entry with its display metadata (C `key_value_info`).
#[derive(Debug, Clone)]
struct LoadedEntry {
    scope: ConfigScope,
    origin: OriginType,
    /// Display filename (C `kvi->filename`: as-opened path, blob oid, or
    /// empty for command-line entries).
    filename: String,
    entry: ConfigEntry,
}

/// Quote a filename C-style for `--show-origin` (C `quote_c_style`): plain
/// paths print bare; anything needing it gets double quotes + escapes.
/// Empty names (command line, standard input) print as nothing.
fn quote_filename(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let needs = s.is_empty()
        || s.bytes().any(|b| {
            b < 0x20
                || b == 0x7f
                || matches!(b, b'"' | b'\\' | b';' | b'#' | b':' | b' ' | b'\t' | b'\'')
        });
    if !needs {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                out.push_str(&format!("\\{:03o}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Display a path like C: relative to the process cwd when inside it (so
/// the local scope prints `.git/config`), absolute otherwise.
fn display_path(path: &Path) -> String {
    if path.is_absolute() {
        if let Ok(cwd) = std::env::current_dir() {
            if let Ok(rel) = path.strip_prefix(&cwd) {
                if !rel.as_os_str().is_empty() {
                    return rel.display().to_string();
                }
            }
        }
        return path.display().to_string();
    }
    path.display().to_string()
}

/// C `die_errno` reason for config-file reads: the bare `strerror` text.
fn io_reason(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "No such file or directory".to_string(),
        std::io::ErrorKind::IsADirectory => "Is a directory".to_string(),
        std::io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
        _ => e.to_string(),
    }
}

/// Which scope-file operation targets (C `location_options_init` +
/// `repo_config_set_in_file_gently` defaulting).
struct WriteTarget {
    path: PathBuf,
    /// Display form for messages.
    display: String,
}

/// Resolve the location selectors to a read source. Returns the loaded
/// entries plus whether includes were respected. Errors die with C's exact
/// texts (`only one config file at a time`, `--local`/`--blob`/`--worktree`
/// outside a repo, `$HOME not set`).
struct ReadSource {
    entries: Vec<LoadedEntry>,
    /// A selected single file that does not exist (reads see it as empty;
    /// `list` dies `unable to read`, like C).
    missing_file: Option<String>,
}

fn load_read_source(
    ctx: &RepoContext,
    repo: Option<&git_core::Repository>,
    loc: &LocOpts,
) -> Result<ReadSource, CommandError> {
    let n_selectors = [loc.global, loc.system, loc.local, loc.worktree].iter().filter(|b| **b).count()
        + loc.file.is_some() as usize
        + loc.blob.is_some() as usize;
    if n_selectors > 1 {
        return Err(CommandError::usage("error: only one config file at a time"));
    }
    if repo.is_none() {
        if loc.local {
            return Err(CommandError::fatal("fatal: --local can only be used inside a git repository"));
        }
        if loc.blob.is_some() {
            return Err(CommandError::fatal("fatal: --blob can only be used inside a git repository"));
        }
        if loc.worktree {
            return Err(CommandError::fatal(
                "fatal: --worktree can only be used inside a git repository",
            ));
        }
    }
    let has_selector =
        loc.global || loc.system || loc.local || loc.worktree || loc.file.is_some() || loc.blob.is_some();
    // C `location_options_init`: an explicit selector (scope flag, `-f`,
    // `--blob`) disables include-following unless `--includes` is given;
    // only the default layered read follows includes.
    let respect = loc.includes.unwrap_or(!has_selector);

    // --blob: read the blob object (scope command, origin blob).
    if let Some(blob_rev) = &loc.blob {
        let repo = repo.ok_or_else(|| {
            CommandError::fatal("fatal: --blob can only be used inside a git repository")
        })?;
        let oid = crate::resolve_arg(repo, blob_rev).map_err(|_| {
            CommandError::fatal(format!("fatal: {blob_rev}: not a valid blob"))
        })?;
        let odb = git_odb::Odb::from_repo(repo).map_err(CommandError::from)?;
        let obj = odb.read(&oid).map_err(|_| {
            CommandError::fatal(format!("fatal: {blob_rev}: not a valid blob"))
        })?;
        if obj.kind != git_object::ObjectKind::Blob {
            return Err(CommandError::fatal(format!("fatal: {blob_rev}: not a valid blob")));
        }
        let mut set = ConfigSet::new();
        match ConfigSet::parse(&obj.data) {
            Ok(parsed) => set = parsed,
            Err(ConfigError::BadLine { line, .. }) => {
                // C reports blob parse errors via error() + the list-path
                // fatal (probed on the tree binary).
                eprintln!("error: bad config line {line} in blob {oid}");
                return Err(CommandError::fatal("fatal: error processing config file(s)"));
            }
            Err(e) => {
                return Err(CommandError::fatal(format!("fatal: {e}")));
            }
        }
        let _ = respect;
        let entries = set
            .entries()
            .iter()
            .cloned()
            .map(|entry| LoadedEntry {
                scope: ConfigScope::Command,
                origin: OriginType::Blob,
                filename: oid.to_string(),
                entry,
            })
            .collect();
        return Ok(ReadSource { entries, missing_file: None });
    }

    // -f <file> / stdin.
    if let Some(file) = &loc.file {
        if file == "-" {
            let mut buf = Vec::new();
            use std::io::Read as _;
            std::io::stdin()
                .read_to_end(&mut buf)
                .map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
            match ConfigSet::parse(&buf) {
                Ok(set) => {
                    let entries = set
                        .entries()
                        .iter()
                        .cloned()
                        .map(|entry| LoadedEntry {
                            scope: ConfigScope::Command,
                            origin: OriginType::Stdin,
                            filename: String::new(),
                            entry,
                        })
                        .collect();
                    return Ok(ReadSource { entries, missing_file: None });
                }
                Err(ConfigError::BadLine { line, .. }) => {
                    return Err(CommandError::fatal(format!(
                        "fatal: bad config line {line} in standard input"
                    )));
                }
                Err(e) => return Err(CommandError::fatal(format!("fatal: {e}"))),
            }
        }
        let raw = PathBuf::from(file);
        let disk = if raw.is_absolute() { raw.clone() } else { ctx.cwd.join(&raw) };
        // Missing file: reads see an empty config (list dies); the display
        // keeps the as-given form, like C.
        let data = match std::fs::read(&disk) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ReadSource { entries: Vec::new(), missing_file: Some(file.clone()) });
            }
            Err(e) => {
                return Err(CommandError::fatal(format!(
                    "fatal: unable to read config file '{}': {}",
                    file,
                    io_reason(&e),
                )));
            }
        };
        let set = if respect {
            // Includes resolve with repo context when available.
            let ictx = repo
                .map(|r| {
                    IncludeContext::for_repo(
                        &r.git_dir,
                        r.work_tree.clone().or_else(|| r.git_dir.parent().map(|p| p.to_path_buf())),
                    )
                })
                .unwrap_or_default();
            ConfigSet::from_file_with(&disk, &ictx).map_err(|e| file_load_error(&e, file))?
        } else {
            ConfigSet::parse(&data).map_err(|e| file_load_error(&e, file))?
        };
        let entries = set
            .entries()
            .iter()
            .cloned()
            .map(|entry| LoadedEntry {
                scope: ConfigScope::Command,
                origin: OriginType::File,
                filename: file.clone(),
                entry,
            })
            .collect();
        return Ok(ReadSource { entries, missing_file: None });
    }

    // Scope selectors over a repository (global/system work outside one).
    if loc.global || loc.system || loc.local || loc.worktree {
        let (path, scope) = if loc.global {
            (
                git_config::global_config_file()
                    .ok_or_else(|| CommandError::fatal("fatal: $HOME not set"))?,
                ConfigScope::Global,
            )
        } else if loc.system {
            (git_config::system_config_file(), ConfigScope::System)
        } else {
            let repo = repo.ok_or_else(|| CommandError::fatal("fatal: not in a git directory"))?;
            scope_file(repo, loc)?
        };
        let display = display_path(&path);
        // Existence check first: missing reads as empty (list dies on the
        // marker below), unreadable dies — like C.
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ReadSource { entries: Vec::new(), missing_file: Some(display.clone()) });
            }
            Err(e) => {
                return Err(CommandError::fatal(format!(
                    "fatal: unable to read config file '{}': {}",
                    display,
                    io_reason(&e),
                )));
            }
        };
        // A bare scope selector only follows its own includes under
        // `--includes`; otherwise the file reads as literal entries (C
        // `respect_includes` default).
        let set = if respect {
            // Scope files resolve their own includes (C always respects them
            // here; `respect` only gates `-f`).
            let ictx = repo
                .map(|r| {
                    IncludeContext::for_repo(
                        &r.git_dir,
                        r.work_tree.clone().or_else(|| r.git_dir.parent().map(|p| p.to_path_buf())),
                    )
                })
                .unwrap_or_default();
            ConfigSet::from_file_with(&path, &ictx).map_err(|e| file_load_error(&e, &display))?
        } else {
            ConfigSet::parse(&data).map_err(|e| file_load_error(&e, &display))?
        };
        let entries = set
            .entries()
            .iter()
            .cloned()
            .map(|entry| LoadedEntry {
                scope,
                origin: OriginType::File,
                filename: display.clone(),
                entry,
            })
            .collect();
        return Ok(ReadSource { entries, missing_file: None });
    }

    // Default: full layered scopes with per-entry scope tags, then the
    // command-line overlays (scope command, origin command line).
    let mut entries: Vec<LoadedEntry> = Vec::new();
    if let Some(repo) = repo {
        let scopes = git_config::RepoScopes {
            git_dir: repo.git_dir.clone(),
            commondir: repo.common_dir.clone(),
            worktree: repo.work_tree.clone(),
            git_dir_verbatim: repo.git_dir_specified.clone(),
        };
        let scoped = ConfigSet::load_repo_scoped(&scopes).map_err(|e| match e {
            ConfigError::BadLine { line, file } => {
                let name = file
                    .map(|p| display_path(&p))
                    .unwrap_or_else(|| ".git/config".to_string());
                CommandError::fatal(format!("fatal: bad config line {line} in file {name}"))
            }
            other => CommandError::fatal(format!("fatal: {other}")),
        })?;
        for s in scoped {
            let filename = s
                .entry
                .origin
                .as_ref()
                .map(|p| display_path(p))
                .unwrap_or_default();
            entries.push(LoadedEntry { scope: s.scope, origin: OriginType::File, filename, entry: s.entry });
        }
    } else {
        // Outside a repository: system + global scopes only (like C).
        // Missing files are skipped silently.
        let mut roots: Vec<(ConfigScope, PathBuf)> = Vec::new();
        if std::env::var_os("GIT_CONFIG_NOSYSTEM").is_none() {
            roots.push((ConfigScope::System, git_config::system_config_file()));
        }
        if let Some(g) = git_config::global_config_file() {
            roots.push((ConfigScope::Global, g));
        }
        for (scope, path) in roots {
            let display = display_path(&path);
            if !path.exists() {
                continue;
            }
            let set = ConfigSet::from_file_with(&path, &IncludeContext::default())
                .map_err(|e| file_load_error(&e, &display))?;
            for entry in set.entries().iter().cloned() {
                entries.push(LoadedEntry {
                    scope,
                    origin: OriginType::File,
                    filename: display.clone(),
                    entry,
                });
            }
        }
    }
    // Overlays applied by the caller afterwards (highest precedence).
    Ok(ReadSource { entries, missing_file: None })
}

/// Map a single-file load error to C's read texts.
fn file_load_error(e: &git_config::ConfigError, display: &str) -> CommandError {
    use git_config::ConfigError;
    match e {
        ConfigError::BadLine { line, .. } => {
            CommandError::fatal(format!("fatal: bad config line {line} in file {display}"))
        }
        ConfigError::IncludeDepth { .. } | ConfigError::IncludeCycle(_) | ConfigError::RemoteUrlForbidden => {
            CommandError::fatal(format!("fatal: {e}"))
        }
        other => CommandError::fatal(format!("fatal: {other}")),
    }
}

/// Resolve `--global/--system/--local/--worktree` to a file + scope.
fn scope_file(repo: &git_core::Repository, loc: &LocOpts) -> Result<(PathBuf, ConfigScope), CommandError> {
    if loc.global {
        return git_config::global_config_file()
            .map(|p| (p, ConfigScope::Global))
            .ok_or_else(|| CommandError::fatal("fatal: $HOME not set"));
    }
    if loc.system {
        return Ok((git_config::system_config_file(), ConfigScope::System));
    }
    if loc.local {
        return Ok((repo.common_dir.join("config"), ConfigScope::Local));
    }
    // --worktree: config.worktree when the extension is enabled (scope
    // still reports local, like C), else the shared config on a single
    // worktree, else C's multi-worktree die.
    let enabled = repo.config.get("core", "repositoryformatversion").is_some()
        && repo.config.get_bool("extensions", "worktreeconfig") == Some(true);
    if enabled {
        return Ok((repo.git_dir.join("config.worktree"), ConfigScope::Local));
    }
    let linked = repo.git_dir.join("worktrees");
    let multiple = std::fs::read_dir(&linked).map(|mut d| d.any(|_| true)).unwrap_or(false);
    if multiple {
        return Err(CommandError::fatal(
            "fatal: --worktree cannot be used with multiple working trees unless the config\nextension worktreeConfig is enabled. Please read \"CONFIGURATION FILE\"\nsection in \"git help worktree\" for details",
        ));
    }
    Ok((repo.common_dir.join("config"), ConfigScope::Local))
}

// ---------------------------------------------------------------------------
// Value formatting (C `format_config` + type backends)
// ---------------------------------------------------------------------------

/// The effective value: `None` for bare keys (C NULL).
fn eff_value(e: &LoadedEntry) -> Option<&str> {
    if e.entry.value_is_null {
        None
    } else {
        Some(e.entry.value.as_str())
    }
}

/// C `interpolate_path` for `--path` reads: `%(prefix)/` expands to the
/// install prefix, `~`/`~/` to `$HOME`; anything else is verbatim.
fn interpolate_display(value: &str) -> Result<String, String> {
    if let Some(rest) = value.strip_prefix("%(prefix)/") {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/usr/bin/git"));
        let prefix = exe.parent().and_then(|p| p.parent()).unwrap_or(Path::new("/usr"));
        return Ok(prefix.join(rest).display().to_string());
    }
    if value == "~" || value.starts_with("~/") {
        match std::env::var_os("HOME") {
            Some(h) => {
                let h = h.to_string_lossy().into_owned();
                if value == "~" {
                    return Ok(h);
                }
                return Ok(format!("{h}/{}", &value[2..]));
            }
            None => return Err(format!("failed to expand user dir in: '{value}'")),
        }
    }
    if let Some(rest) = value.strip_prefix('~') {
        // `~user/...`: git looks the user up; without passwd access this
        // fails exactly like C's NULL return.
        if rest.is_empty() || rest.starts_with('/') {
            return Err(format!("failed to expand user dir in: '{value}'"));
        }
        return Err(format!("failed to expand user dir in: '{value}'"));
    }
    Ok(value.to_string())
}

/// C `parse_expiry_date`: `never`/`false` → 0, `all`/`now` → TIME_MAX,
/// else approxidate.
fn parse_expiry(value: &str) -> Option<u64> {
    if value == "never" || value == "false" {
        return Some(0);
    }
    if value == "all" || value == "now" {
        return Some(u64::MAX);
    }
    let now = {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        git_date::Timestamp::new(secs, 0)
    };
    git_date::parse(value, now).ok().map(|t| t.secs as u64)
}

/// Format one entry's value per the display type. `gently` skips
/// unformattable values (list/get-all paths); otherwise the first failure
/// is returned for the C-exact die. `key_dotted` is the canonical
/// `section[.sub.]key` name for messages.
fn format_typed(
    cli_type: Option<CliType>,
    key_dotted: &str,
    value: Option<&str>,
    loc: &TypeLoc,
) -> Result<Option<String>, TypeError> {
    let Some(ty) = cli_type else {
        return Ok(value.map(|v| v.to_string()));
    };
    match ty {
        CliType::Bool => {
            let Some(v) = value else {
                return Ok(Some("true".to_string()));
            };
            parse_bool(v)
                .map(|b| Some(b.to_string()))
                .ok_or_else(|| loc.render(format!("bad boolean config value '{v}' for '{key_dotted}'")))
        }
        CliType::Int => {
            let v = value.unwrap_or("");
            canonicalize_typed(ConfigValueType::Int, key_dotted, v)
                .map(Some)
                .map_err(|e| loc.int_error(&e.to_string()))
        }
        CliType::BoolOrInt => {
            if let Some(b) = value.and_then(parse_bool_text) {
                // C `git_parse_maybe_bool_text` (empty counts as false).
                return Ok(Some(b.to_string()));
            }
            if value.is_none() {
                return Ok(Some("false".to_string()));
            }
            let v = value.unwrap_or("");
            canonicalize_typed(ConfigValueType::BoolOrInt, key_dotted, v)
                .map(Some)
                .map_err(|e| loc.int_error(&e.to_string()))
        }
        CliType::BoolOrStr => {
            let Some(v) = value else {
                return Ok(Some("true".to_string()));
            };
            Ok(Some(parse_bool(v).map(|b| b.to_string()).unwrap_or_else(|| v.to_string())))
        }
        CliType::Path => {
            let Some(v) = value else {
                return Err(loc.missing(key_dotted));
            };
            // `:(optional)` missing files are skipped (status 1).
            let (optional, path) = match v.strip_prefix(":(optional)") {
                Some(p) => (true, p),
                None => (false, v),
            };
            match interpolate_display(path) {
                Ok(expanded) => {
                    if optional && !Path::new(&expanded).exists() {
                        return Ok(None);
                    }
                    Ok(Some(expanded))
                }
                Err(msg) => Err(loc.render(msg)),
            }
        }
        CliType::ExpiryDate => {
            let Some(v) = value else {
                return Err(loc.missing(key_dotted));
            };
            parse_expiry(v)
                .map(|t| Some(t.to_string()))
                .ok_or_else(|| loc.render(format!("'{v}' for '{key_dotted}' is not a valid timestamp")))
        }
        CliType::Color => {
            let Some(v) = value else {
                return Err(loc.missing(key_dotted));
            };
            color_parse_value(v).map(Some).ok_or_else(|| loc.color_error(v))
        }
    }
}

/// Location context for type-error dies (C `die_bad_number` +
/// `config_error_nonbool` paths).
enum TypeLoc {
    /// A scope/config file entry: `bad config line N in file F`.
    File { line: usize, file: String },
    Stdin { line: usize },
    Blob { line: usize, blob: String },
    /// Command-line entries: bare dies without a location.
    Cmdline,
}

impl TypeLoc {
    fn of(e: &LoadedEntry) -> TypeLoc {
        match e.origin {
            OriginType::File => TypeLoc::File { line: e.entry.lineno, file: e.filename.clone() },
            OriginType::Stdin => TypeLoc::Stdin { line: e.entry.lineno },
            OriginType::Blob => TypeLoc::Blob { line: e.entry.lineno, blob: e.filename.clone() },
            OriginType::Cmdline => TypeLoc::Cmdline,
        }
    }

    fn render(&self, detail: String) -> TypeError {
        match self {
            TypeLoc::File { line, file } => TypeError::BadLine { detail, line: *line, file: file.clone() },
            TypeLoc::Stdin { line } => TypeError::BadLineStdin { detail, line: *line },
            TypeLoc::Blob { line, blob } => TypeError::BadLineBlob { line: *line, blob: blob.clone() },
            TypeLoc::Cmdline => TypeError::Fatal(detail),
        }
    }

    /// C `config_error_nonbool` on a bare key: `missing value for 'k'`.
    fn missing(&self, key_dotted: &str) -> TypeError {
        self.render(format!("missing value for '{key_dotted}'"))
    }

    fn die(&self, detail: String) -> TypeError {
        self.render(detail)
    }

    fn int_error(&self, base: &str) -> TypeError {
        // C `die_bad_number`: `bad numeric ...` bare for command line,
        // `in file F` / `in standard input` qualified otherwise (probed).
        // Blob value failures surface as bad-line errors (probed).
        if matches!(self, TypeLoc::Blob { .. }) {
            return self.render(base.to_string());
        }
        let qualified = match self {
            TypeLoc::File { file, .. } => Some(format!("in file {file}")),
            TypeLoc::Stdin { .. } => Some("in standard input".to_string()),
            TypeLoc::Blob { .. } | TypeLoc::Cmdline => None,
        };
        match qualified {
            Some(q) => match base.split_once(": ") {
                Some((head, reason)) => self.render(format!("{head} {q}: {reason}")),
                None => self.render(base.to_string()),
            },
            None => self.render(base.to_string()),
        }
    }

    fn color_error(&self, value: &str) -> TypeError {
        // C: `error: invalid color value: V` then the bad-line fatal.
        self.render(format!("invalid color value: {value}"))
    }
}

/// A failed value format, ready to render C-exactly.
enum TypeError {
    /// `error: {detail}` then `fatal: bad config line {line} in file {file}`.
    BadLine { detail: String, line: usize, file: String },
    /// `error: {detail}` then `fatal: bad config line {line} in standard input`.
    BadLineStdin { detail: String, line: usize },
    /// `error: bad config line {line} in blob {blob}` then
    /// `fatal: error processing config file(s)`.
    BadLineBlob { line: usize, blob: String },
    /// `fatal: {detail}`.
    Fatal(String),
}

impl TypeError {
    /// Render to `(stderr_line, fatal)`; the caller prints the stderr line
    /// (if any) and returns the fatal. Exit code is always 128.
    fn render(self) -> (Option<String>, CommandError) {
        match self {
            TypeError::BadLine { detail, line, file } => (
                Some(format!("error: {detail}")),
                CommandError::fatal(format!("fatal: bad config line {line} in file {file}")),
            ),
            TypeError::BadLineStdin { detail, line } => (
                Some(format!("error: {detail}")),
                CommandError::fatal(format!("fatal: bad config line {line} in standard input")),
            ),
            TypeError::BadLineBlob { line, blob } => (
                Some(format!("error: bad config line {line} in blob {blob}")),
                CommandError::fatal("fatal: error processing config file(s)"),
            ),
            TypeError::Fatal(detail) => (None, CommandError::fatal(format!("fatal: {detail}"))),
        }
    }
}

// ---------------------------------------------------------------------------
// Color parsing (C `color.c` `color_parse_mem_1`, no new deps)
// ---------------------------------------------------------------------------

const ANSI_FG: i32 = 30;
const ANSI_BRIGHT_FG: i32 = 90;
const BG_OFFSET: i32 = 10;
const FG_256: i32 = 38;
const FG_RGB: i32 = 38;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorValue {
    Unspecified,
    Normal,
    Ansi(i32),
    C256(u8),
    Rgb(u8, u8, u8),
}

fn match_word_ci(word: &str, mat: &str) -> bool {
    word.len() == mat.len() && word.eq_ignore_ascii_case(mat)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn parse_ansi_color(out: &mut ColorValue, name: &str) -> bool {
    const NAMES: &[&str] = &["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
    if match_word_ci(name, "default") {
        *out = ColorValue::Ansi(9 + ANSI_FG);
        return true;
    }
    let (mut base, mut rest) = (ANSI_FG, name);
    if rest.len() >= 6 && rest[..6].eq_ignore_ascii_case("bright") {
        base = ANSI_BRIGHT_FG;
        rest = &rest[6..];
    }
    for (i, n) in NAMES.iter().enumerate() {
        if match_word_ci(rest, n) {
            *out = ColorValue::Ansi(i as i32 + base);
            return true;
        }
    }
    false
}

fn parse_color_name(out: &mut ColorValue, name: &str) -> bool {
    if match_word_ci(name, "normal") {
        *out = ColorValue::Normal;
        return true;
    }
    let b = name.as_bytes();
    if !b.is_empty() && b[0] == b'#' && (b.len() == 7 || b.len() == 4) {
        if b.len() == 7 {
            let hex2 = |pair: &[u8]| -> Option<u8> {
                Some(hex_val(pair[0])? << 4 | hex_val(pair[1])?)
            };
            if let (Some(r), Some(g), Some(bl)) = (hex2(&b[1..3]), hex2(&b[3..5]), hex2(&b[5..7])) {
                *out = ColorValue::Rgb(r, g, bl);
                return true;
            }
        } else {
            let n4 = |c: u8| hex_val(c).map(|v| v << 4 | v);
            if let (Some(r), Some(g), Some(bl)) = (n4(b[1]), n4(b[2]), n4(b[3])) {
                *out = ColorValue::Rgb(r, g, bl);
                return true;
            }
        }
        // Bad hex falls through to the name/number attempts (which fail).
    }
    if parse_ansi_color(out, name) {
        return true;
    }
    // Literal 256-color number (C `strtol`, full-string match).
    let (neg, digits) = match b.strip_prefix(b"-") {
        Some(d) => (true, d),
        None => (false, b.strip_prefix(b"+").unwrap_or(b)),
    };
    if !digits.is_empty() && digits.iter().all(|c| c.is_ascii_digit()) {
        if let Ok(val) = std::str::from_utf8(digits).unwrap_or("").parse::<i64>() {
            let val = if neg { -val } else { val };
            if val == -1 {
                *out = ColorValue::Normal;
                return true;
            }
            if (0..8).contains(&val) {
                *out = ColorValue::Ansi(val as i32 + ANSI_FG);
                return true;
            }
            if (8..16).contains(&val) {
                *out = ColorValue::Ansi(val as i32 - 8 + ANSI_BRIGHT_FG);
                return true;
            }
            if (16..256).contains(&val) {
                *out = ColorValue::C256(val as u8);
                return true;
            }
        }
    }
    false
}

fn parse_attr_name(name: &str) -> Option<u32> {
    const ATTRS: &[(&str, u32, u32)] = &[
        ("bold", 1, 22),
        ("dim", 2, 22),
        ("italic", 3, 23),
        ("ul", 4, 24),
        ("blink", 5, 25),
        ("reverse", 7, 27),
        ("strike", 9, 29),
    ];
    let mut rest = name;
    let mut negate = false;
    if let Some(s) = rest.strip_prefix("no") {
        rest = s.strip_prefix('-').unwrap_or(s);
        negate = true;
    }
    for (n, val, neg) in ATTRS {
        if *n == rest {
            return Some(if negate { *neg } else { *val });
        }
    }
    None
}

fn color_is_empty(c: &ColorValue) -> bool {
    matches!(c, ColorValue::Unspecified | ColorValue::Normal)
}

/// C `color_parse` → the ANSI escape (empty string for empty/`normal`),
/// or `None` on `invalid color value`.
fn color_parse_value(value: &str) -> Option<String> {
    let words: Vec<&str> = value.split(|c: char| c.is_ascii_whitespace()).filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return Some(String::new());
    }
    let mut has_reset = false;
    let mut attr: u32 = 0;
    let mut fg = ColorValue::Unspecified;
    let mut bg = ColorValue::Unspecified;
    for w in words {
        if match_word_ci(w, "reset") {
            has_reset = true;
            continue;
        }
        let mut c = ColorValue::Unspecified;
        if parse_color_name(&mut c, w) {
            if fg == ColorValue::Unspecified {
                fg = c;
                continue;
            }
            if bg == ColorValue::Unspecified {
                bg = c;
                continue;
            }
            return None;
        }
        match parse_attr_name(w) {
            Some(bit) => attr |= 1 << bit,
            None => return None,
        }
    }
    if !has_reset && attr == 0 && color_is_empty(&fg) && color_is_empty(&bg) {
        return Some(String::new());
    }
    let mut out = String::from("\x1b[");
    let mut sep = false;
    if has_reset {
        sep = true;
    }
    let mut bit = 0u32;
    let mut a = attr;
    while a != 0 {
        if a & 1 != 0 {
            if sep {
                out.push(';');
            }
            out.push_str(&bit.to_string());
            sep = true;
        }
        a >>= 1;
        bit += 1;
    }
    let mut color_part = |c: &ColorValue, bg_flag: bool| {
        let off = if bg_flag { BG_OFFSET } else { 0 };
        match c {
            ColorValue::Unspecified | ColorValue::Normal => {}
            ColorValue::Ansi(v) => {
                if sep {
                    out.push(';');
                }
                out.push_str(&(v + off).to_string());
                sep = true;
            }
            ColorValue::C256(v) => {
                if sep {
                    out.push(';');
                }
                out.push_str(&format!("{};5;{v}", FG_256 + off));
                sep = true;
            }
            ColorValue::Rgb(r, g, b) => {
                if sep {
                    out.push(';');
                }
                out.push_str(&format!("{};2;{r};{g};{b}", FG_RGB + off));
                sep = true;
            }
        }
    };
    color_part(&fg, false);
    color_part(&bg, true);
    out.push('m');
    Some(out)
}

// ---------------------------------------------------------------------------
// URL normalization + matching (C `urlmatch.c`, no new deps)
// ---------------------------------------------------------------------------

const URL_SCHEME_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+.-";
const URL_HOST_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789.-_[]:";
const URL_UNSAFE: &[u8] = b" <>\"%{}|\\^`";
const URL_RESERVED: &[u8] = b":/?#[]@!$&'()*+,;=";

#[derive(Debug, Clone, Default)]
struct UrlInfo {
    url: Vec<u8>,
    scheme_len: usize,
    user_off: usize,
    user_len: usize,
    host_off: usize,
    host_len: usize,
    port_off: usize,
    port_len: usize,
    path_off: usize,
}

/// Append with C `append_normalized_escapes` semantics. Returns false on a
/// bad `%XX` sequence.
fn append_normalized_escapes(out: &mut Vec<u8>, from: &[u8], esc_ok: &[u8]) -> bool {
    let mut i = 0;
    while i < from.len() {
        let mut ch = from[i];
        let mut was_esc = false;
        i += 1;
        if ch == b'%' {
            if i + 2 > from.len() {
                return false;
            }
            let (Some(h), Some(l)) = (hex_val(from[i]), hex_val(from[i + 1])) else {
                return false;
            };
            ch = h << 4 | l;
            i += 2;
            was_esc = true;
        }
        if ch <= 0x1F
            || ch >= 0x7F
            || URL_UNSAFE.contains(&ch)
            || (was_esc && esc_ok.contains(&ch))
        {
            out.extend_from_slice(format!("%{ch:02X}").as_bytes());
        } else {
            out.push(ch);
        }
    }
    true
}

/// C `url_normalize_1`. On success returns the normalized info; on failure
/// the English error text C dies with.
fn url_normalize(url: &str, allow_globs: bool) -> Result<UrlInfo, String> {
    let raw = url.as_bytes();
    // Scheme + "://".
    let mut spanned = 0;
    while spanned < raw.len() && URL_SCHEME_CHARS.as_bytes().contains(&raw[spanned]) {
        spanned += 1;
    }
    if spanned == 0
        || !raw[0].is_ascii_alphabetic()
        || spanned + 3 > raw.len()
        || raw[spanned] != b':'
        || raw[spanned + 1] != b'/'
        || raw[spanned + 2] != b'/'
    {
        return Err("invalid URL scheme name or missing '://' suffix".to_string());
    }
    let mut norm: Vec<u8> = Vec::with_capacity(raw.len() + 1);
    let scheme_len = spanned;
    for b in &raw[..spanned + 3] {
        norm.push(b.to_ascii_lowercase());
    }
    let mut rest = &raw[spanned + 3..];

    // user:password@
    let mut user_off = 0;
    let mut user_len = 0;
    if let Some(at) = rest.iter().position(|&b| b == b'@') {
        let slash = rest.iter().position(|&b| b == b'/' || b == b'?' || b == b'#').unwrap_or(rest.len());
        if at < slash {
            user_off = norm.len();
            if at > 0 {
                if !append_normalized_escapes(&mut norm, &rest[..at], URL_RESERVED) {
                    return Err("invalid %XX escape sequence".to_string());
                }
                if let Some(ci) = norm[scheme_len + 3..].iter().position(|&b| b == b':') {
                    let passwd_off = scheme_len + 3 + ci + 1;
                    user_len = passwd_off - 1 - (scheme_len + 3);
                    let _ = passwd_off;
                } else {
                    user_len = norm.len() - (scheme_len + 3);
                }
            }
            norm.push(b'@');
            rest = &rest[at + 1..];
        }
    }

    // Host.
    let mut host_off = 0;
    let slash = rest.iter().position(|&b| b == b'/' || b == b'?' || b == b'#').unwrap_or(rest.len());
    if rest.is_empty() || matches!(rest[0], b':' | b'/' | b'?' | b'#') {
        if !norm.starts_with(b"file:") {
            return Err("missing host and scheme is not 'file:'".to_string());
        }
    } else {
        host_off = norm.len();
    }
    // Find the port colon: last ':' before slash, unless ']' intervenes.
    let mut colon = slash;
    while colon > 0 && rest[colon - 1] != b':' && rest[colon - 1] != b']' {
        colon -= 1;
    }
    let has_port_colon = colon > 0 && colon < rest.len() && rest[colon - 1] == b':' && colon - 1 < slash;
    let colon_ptr = if has_port_colon { colon - 1 } else { slash };
    if !has_port_colon && host_off == 0 && colon_ptr < slash && colon_ptr + 1 != slash {
        // file: URLs may not have a port number — only when host empty.
        if norm.starts_with(b"file:") && colon_ptr < slash {
            // C checks `!host_off && colon_ptr < slash_ptr && colon_ptr+1 != slash_ptr`.
        }
    }
    // Validate host chars.
    let host_part = &rest[..colon_ptr.min(rest.len())];
    let allowed = if allow_globs {
        URL_HOST_CHARS.to_string() + "*"
    } else {
        URL_HOST_CHARS.to_string()
    };
    if host_part.iter().any(|b| !allowed.as_bytes().contains(b)) {
        return Err("invalid characters in host name".to_string());
    }
    for b in host_part {
        norm.push(b.to_ascii_lowercase());
    }
    let mut port_off = 0;
    let mut port_len = 0;
    let mut host_len = 0;
    // Port (C: strip leading zeros; empty/all-zeros means default → drop;
    // http:80 and https:443 are dropped; else 1..65535 required).
    if colon_ptr < slash {
        // `file:` URLs may not carry a port at all.
        if host_off == 0 && colon_ptr + 1 != slash {
            return Err("a 'file:' URL may not have a port number".to_string());
        }
        let mut port = &rest[colon_ptr + 1..slash];
        while !port.is_empty() && port[0] == b'0' {
            port = &port[1..];
        }
        let skip = port.is_empty()
            || (port == b"80" && norm.starts_with(b"http:"))
            || (port == b"443" && norm.starts_with(b"https:"));
        if !skip {
            if port.iter().any(|b| !b.is_ascii_digit()) {
                return Err("invalid port number".to_string());
            }
            let pnum: u64 = if port.len() <= 5 { std::str::from_utf8(port).unwrap_or("").parse().unwrap_or(0) } else { 0 };
            if pnum == 0 || pnum > 65535 {
                return Err("invalid port number".to_string());
            }
            norm.push(b':');
            port_off = norm.len();
            norm.extend_from_slice(port);
            port_len = port.len();
        }
        rest = &rest[slash..];
    } else {
        rest = &rest[slash.min(rest.len())..];
    }
    if host_off != 0 {
        host_len = norm.len() - host_off - if port_len > 0 { port_len + 1 } else { 0 };
    }

    // Path with `.`/`..` resolution (C loop, mirrored exactly).
    let path_off = norm.len();
    let path_start = path_off;
    norm.push(b'/');
    let mut url_rest = rest;
    if url_rest.first() == Some(&b'/') {
        url_rest = &url_rest[1..];
    }
    loop {
        let seg_end = url_rest.iter().position(|&b| b == b'/' || b == b'?' || b == b'#').unwrap_or(url_rest.len());
        let seg_start_off = norm.len();
        let mut skip_add_slash = false;
        if !append_normalized_escapes(&mut norm, &url_rest[..seg_end], URL_RESERVED) {
            return Err("invalid %XX escape sequence".to_string());
        }
        if norm[seg_start_off..] == [b'.'] {
            if seg_start_off == path_start + 1 {
                norm.truncate(norm.len() - 1);
                skip_add_slash = true;
            } else {
                norm.truncate(norm.len() - 2);
            }
        } else if norm[seg_start_off..] == [b'.', b'.'] {
            let prev = norm.len() - 3;
            if prev == path_start {
                return Err("invalid '..' path segment".to_string());
            }
            let mut ps = prev;
            while norm[ps] != b'/' {
                ps -= 1;
            }
            if ps == path_start {
                norm.truncate(ps + 1);
                skip_add_slash = true;
            } else {
                norm.truncate(ps);
            }
        }
        url_rest = &url_rest[seg_end..];
        if url_rest.first() != Some(&b'/') {
            break;
        }
        url_rest = &url_rest[1..];
        if !skip_add_slash {
            norm.push(b'/');
        }
    }

    // Trailing query/fragment: copy with escape normalization.
    if !url_rest.is_empty() {
        if !append_normalized_escapes(&mut norm, url_rest, URL_RESERVED) {
            return Err("invalid %XX escape sequence".to_string());
        }
    }

    Ok(UrlInfo { url: norm, scheme_len, user_off, user_len, host_off, host_len, port_off, port_len, path_off })
}

/// C `url_match_prefix`: prefix matches at a `/` boundary (implicit
/// trailing slash on both sides).
fn url_match_prefix(url: &[u8], prefix: &[u8]) -> usize {
    if prefix.is_empty() || (prefix.len() == 1 && prefix[0] == b'/') {
        return if url.is_empty() || url[0] == b'/' { 1 } else { 0 };
    }
    let mut plen = prefix.len();
    if prefix[plen - 1] == b'/' {
        plen -= 1;
    }
    if url.len() < plen || &url[..plen] != &prefix[..plen] {
        return 0;
    }
    if url.len() == plen || url[plen] == b'/' {
        return plen + 1;
    }
    0
}

/// C `match_host`: dot-separated components, `*` matching any component.
fn match_host(url: &UrlInfo, pat: &UrlInfo) -> bool {
    let mut u = &url.url[url.host_off..url.host_off + url.host_len];
    let mut p = &pat.url[pat.host_off..pat.host_off + pat.host_len];
    while !u.is_empty() && !p.is_empty() {
        let un = u.iter().position(|&b| b == b'.').unwrap_or(u.len());
        let pn = p.iter().position(|&b| b == b'.').unwrap_or(p.len());
        let (utok, ptok) = (&u[..un], &p[..pn]);
        if !(ptok.len() == 1 && ptok[0] == b'*') && (utok.len() != ptok.len() || utok != ptok) {
            return false;
        }
        u = if un < u.len() { &u[un + 1..] } else { &u[un..] };
        p = if pn < p.len() { &p[pn + 1..] } else { &p[pn..] };
    }
    u.is_empty() && p.is_empty()
}

struct UrlMatch {
    hostmatch_len: usize,
    pathmatch_len: usize,
    user_matched: bool,
}

/// C `match_urls`: scheme + optional user + host + port + path-prefix.
fn match_urls(url: &UrlInfo, prefix: &UrlInfo) -> Option<UrlMatch> {
    if prefix.scheme_len != url.scheme_len || url.url[..url.scheme_len] != prefix.url[..prefix.scheme_len] {
        return None;
    }
    let mut user_matched = false;
    if prefix.user_off != 0 {
        if url.user_off == 0
            || url.user_len != prefix.user_len
            || url.url[url.user_off..url.user_off + url.user_len]
                != prefix.url[prefix.user_off..prefix.user_off + prefix.user_len]
        {
            return None;
        }
        user_matched = true;
    }
    if !match_host(url, prefix) {
        return None;
    }
    if url.port_len != prefix.port_len
        || (prefix.port_len > 0
            && url.url[url.port_off..url.port_off + url.port_len]
                != prefix.url[prefix.port_off..prefix.port_off + prefix.port_len])
    {
        return None;
    }
    let pathmatch_len = url_match_prefix(
        &url.url[url.path_off..],
        &prefix.url[prefix.path_off..],
    );
    if pathmatch_len == 0 {
        return None;
    }
    Some(UrlMatch { hostmatch_len: prefix.host_len, pathmatch_len, user_matched })
}

/// C `cmp_matches`: more specific (longer host, then path, then user) wins.
fn cmp_matches(a: &UrlMatch, b: &UrlMatch) -> std::cmp::Ordering {
    a.hostmatch_len
        .cmp(&b.hostmatch_len)
        .then(a.pathmatch_len.cmp(&b.pathmatch_len))
        .then(b.user_matched.cmp(&a.user_matched))
}

// ---------------------------------------------------------------------------
// Matching + output (C `collect_config`, `show_all_config`, `get_value`)
// ---------------------------------------------------------------------------

/// Canonical dotted name for an entry (C lowercases section+key, keeps
/// subsection case).
fn dotted_name(e: &ConfigEntry) -> String {
    match &e.subsection {
        Some(sub) => format!("{}.{}.{}", e.section.to_ascii_lowercase(), sub, e.key.to_ascii_lowercase()),
        None => format!("{}.{}", e.section.to_ascii_lowercase(), e.key.to_ascii_lowercase()),
    }
}

/// Lowercase a key-pattern's first and last dotted components (C
/// `get_value` naive lowercasing).
fn lowercase_pattern(pat: &str) -> String {
    let mut parts: Vec<String> = pat.split('.').map(|s| s.to_string()).collect();
    if !parts.is_empty() {
        parts[0] = parts[0].to_lowercase();
        let last = parts.len() - 1;
        parts[last] = parts[last].to_lowercase();
    }
    parts.join(".")
}

/// Render one entry line per the display options (C `format_config`).
/// `key_delim` is `' '` for get, `'='` for list (`'\n'` when NUL).
/// Returns `None` when the entry is skipped (gently, or optional-missing).
fn render_entry(
    disp: &DispOpts,
    e: &LoadedEntry,
    formatted_value: Option<String>,
    key_delim: char,
) -> Option<String> {
    let term = if disp.end_nul { '\0' } else { '\n' };
    let mut line = String::new();
    if disp.show_scope {
        line.push_str(e.scope.name());
        line.push(if disp.end_nul { '\0' } else { '\t' });
    }
    if disp.show_origin {
        line.push_str(e.origin.name());
        line.push(':');
        if disp.end_nul {
            line.push_str(&e.filename);
        } else {
            line.push_str(&quote_filename(&e.filename));
        }
        line.push(if disp.end_nul { '\0' } else { '\t' });
    }
    let name = dotted_name(&e.entry);
    if disp.show_keys {
        line.push_str(&name);
    }
    if disp.omit_values {
        line.push(term);
        return Some(line);
    }
    let Some(value) = formatted_value else {
        // Missing optional value (e.g. `:(optional)` path): skip the entry.
        return None;
    };
    if disp.show_keys {
        line.push(key_delim);
    }
    line.push_str(&value);
    line.push(term);
    Some(line)
}

/// Write one rendered line (already terminated) to `out`.
fn write_line(out: &mut dyn Write, line: &str) -> Result<(), CommandError> {
    out.write_all(line.as_bytes()).map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
    Ok(())
}

/// Match entries for `get/get-all/get-regexp` (C `collect_config` key +
/// value-pattern logic, minus output).
fn match_key(
    e: &ConfigEntry,
    canon_key: Option<&(String, Option<String>, String)>,
    key_regex: Option<&str>,
    value_matcher: Option<&cfgfile::ValueMatcher>,
    negate_match: bool,
) -> bool {
    if let Some(re) = key_regex {
        if !cfgfile::regex_search(re, &dotted_name(e)) {
            return false;
        }
    } else if let Some((sec, sub, key)) = canon_key {
        if e.section.to_ascii_lowercase() != *sec
            || e.subsection.as_deref() != sub.as_deref()
            || e.key.to_ascii_lowercase() != *key
        {
            return false;
        }
    }
    if let Some(m) = value_matcher {
        let v = if e.value_is_null { "" } else { e.value.as_str() };
        let hit = m.matches(v);
        if hit == negate_match {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// get / list / urlmatch
// ---------------------------------------------------------------------------

struct GetReq {
    key: String,
    regexp: bool,
    all: bool,
    value_pattern: Option<String>,
    fixed_value: bool,
}

/// Emit a TypeError: print the stderr line (if any), return the fatal.
fn emit_type_error(te: TypeError) -> CommandError {
    match te.render() {
        (Some(err_line), fatal) => {
            eprintln!("{err_line}");
            fatal
        }
        (None, fatal) => fatal,
    }
}

/// C `get_value`: collect matching entries, format strictly, print the
/// last (or all), exit 1 when empty.
fn run_get(
    entries: &[LoadedEntry],
    disp: &DispOpts,
    req: &GetReq,
    key_delim: char,
    out: &mut dyn Write,
) -> Result<(), CommandError> {
    // Key validation / pattern compilation.
    let mut canon: Option<(String, Option<String>, String)> = None;
    let mut key_re: Option<String> = None;
    if req.regexp {
        let lowered = lowercase_pattern(&req.key);
        cfgfile::ValueMatcher::compile(&lowered, false).map_err(|_| {
            CommandError { message: format!("error: invalid key pattern: {}", req.key), code: 6 }
        })?;
        key_re = Some(lowered);
    } else {
        match parse_key_name(&req.key) {
            Ok(k) => canon = Some((k.section, k.subsection, k.key)),
            Err((_code, msg)) => {
                // C `get_value` maps every key failure to INVALID_KEY (1).
                return Err(CommandError { message: format!("error: {msg}"), code: 1 });
            }
        }
    }
    let (matcher, negate) = match &req.value_pattern {
        None => (None, false),
        Some(p) => {
            if req.fixed_value {
                (Some(cfgfile::ValueMatcher::Fixed(p.clone())), false)
            } else if let Some(rest) = p.strip_prefix('!') {
                let m = cfgfile::ValueMatcher::compile(rest, false).map_err(|_| {
                    CommandError { message: format!("error: invalid pattern: {rest}"), code: 6 }
                })?;
                (Some(m), true)
            } else {
                let m = cfgfile::ValueMatcher::compile(p, false).map_err(|_| {
                    CommandError { message: format!("error: invalid pattern: {p}"), code: 6 }
                })?;
                (Some(m), false)
            }
        }
    };

    // Collect (C `collect_config` with gently=0: the first bad value dies).
    let mut collected: Vec<String> = Vec::new();
    for e in entries {
        if !match_key(&e.entry, canon.as_ref(), key_re.as_deref(), matcher.as_ref(), negate) {
            continue;
        }
        let name = dotted_name(&e.entry);
        let loc = TypeLoc::of(e);
        let formatted = match format_typed(disp.cli_type, &name, eff_value(e), &loc) {
            Ok(v) => v,
            Err(te) => return Err(emit_type_error(te)),
        };
        // Optional-missing values are skipped (status 1).
        if formatted.is_none() {
            continue;
        }
        if let Some(line) = render_entry(disp, e, formatted, key_delim) {
            collected.push(line);
        }
    }

    if collected.is_empty() {
        if let Some(def) = &disp.default_value {
            // The default formats with command-line origin metadata.
            let fake = LoadedEntry {
                scope: ConfigScope::Command,
                origin: OriginType::Cmdline,
                filename: String::new(),
                entry: ConfigEntry {
                    section: String::new(),
                    subsection: None,
                    key: req.key.clone(),
                    value: def.clone(),
                    origin: None,
                    lineno: 0,
                    value_is_null: false,
                },
            };
            let loc = TypeLoc::Cmdline;
            let formatted = match format_typed(disp.cli_type, &req.key, Some(def), &loc) {
                Ok(v) => v,
                Err(TypeError::Fatal(_)) => {
                    return Err(CommandError::fatal(format!(
                        "fatal: failed to format default config value: {def}"
                    )));
                }
                Err(te) => return Err(emit_type_error(te)),
            };
            if let Some(value) = formatted {
                if let Some(line) = render_entry(disp, &fake, Some(value), key_delim) {
                    write_line(out, &line)?;
                    return Ok(());
                }
            }
            return Err(CommandError::silent(1));
        }
        return Err(CommandError::silent(1));
    }

    if req.all {
        for line in &collected {
            write_line(out, line)?;
        }
    } else {
        write_line(out, collected.last().unwrap())?;
    }
    Ok(())
}

pub struct Config;

impl Command for Config {
    fn name(&self) -> &'static str {
        "config"
    }

    fn run(
        &self,
        ctx: &RepoContext,
        args: &[String],
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        enum Action {
            List,
            Get,
            GetAll,
            GetRegexp,
            GetUrlmatch,
            Set,
            SetAll,
            ReplaceAll,
            Add,
            Unset,
            UnsetAll,
            RenameSection,
            RemoveSection,
            Edit,
            GetColor,
            GetColorBool,
        }

        let mut actions = 0;
        let mut loc = LocOpts::default();
        let mut disp = DispOpts::default();
        let mut value_slot: Option<String> = None;
        let mut comment_slot: Option<String> = None;
        let mut flags = 0;
        let mut append = false;
        let mut value_pattern = None;
        let mut url = None;

        let mut extra = |opt_loc: &mut LocOpts,
                         opt_disp: &mut DispOpts,
                         opt: &str,
                         negated: bool,
                         arg: Option<&str>,
                         _slot: &mut Option<String>| -> Result<bool, CommandError> {
            match opt {
                "includes" => {
                    opt_loc.includes = Some(!negated);
                    Ok(true)
                }
                "file" => {
                    if negated {
                        return Err(unknown_option(LEGACY_USAGE, opt));
                    }
                    opt_loc.file = arg.map(|s| s.to_string());
                    Ok(true)
                }
                "blob" => {
                    if negated {
                        return Err(unknown_option(LEGACY_USAGE, opt));
                    }
                    opt_loc.blob = arg.map(|s| s.to_string());
                    Ok(true)
                }
                "global" => {
                    opt_loc.global = !negated;
                    Ok(true)
                }
                "system" => {
                    opt_loc.system = !negated;
                    Ok(true)
                }
                "local" => {
                    opt_loc.local = !negated;
                    Ok(true)
                }
                "worktree" => {
                    opt_loc.worktree = !negated;
                    Ok(true)
                }
                "null" => {
                    opt_disp.end_nul = !negated;
                    Ok(true)
                }
                "name-only" => {
                    opt_disp.omit_values = !negated;
                    Ok(true)
                }
                "show-origin" => {
                    opt_disp.show_origin = !negated;
                    Ok(true)
                }
                "show-scope" => {
                    opt_disp.show_scope = !negated;
                    Ok(true)
                }
                "show-names" => {
                    opt_disp.show_keys = !negated;
                    Ok(true)
                }
                "type" => {
                    if negated {
                        opt_disp.cli_type = None;
                    } else {
                        let ty = parse_type_name(arg.ok_or_else(|| {
                            unknown_option(LEGACY_USAGE, "--type")
                        })?)
                        .ok_or_else(|| {
                            unknown_option(LEGACY_USAGE, "--type")
                        })?;
                        set_cli_type(&mut opt_disp.cli_type, ty)?;
                    }
                    Ok(true)
                }
                "bool" => {
                    if negated {
                        opt_disp.cli_type = None;
                    } else {
                        set_cli_type(&mut opt_disp.cli_type, CliType::Bool)?;
                    }
                    Ok(true)
                }
                "int" => {
                    if negated {
                        opt_disp.cli_type = None;
                    } else {
                        set_cli_type(&mut opt_disp.cli_type, CliType::Int)?;
                    }
                    Ok(true)
                }
                "bool-or-int" => {
                    if negated {
                        opt_disp.cli_type = None;
                    } else {
                        set_cli_type(&mut opt_disp.cli_type, CliType::BoolOrInt)?;
                    }
                    Ok(true)
                }
                "bool-or-str" => {
                    if negated {
                        opt_disp.cli_type = None;
                    } else {
                        set_cli_type(&mut opt_disp.cli_type, CliType::BoolOrStr)?;
                    }
                    Ok(true)
                }
                "path" => {
                    if negated {
                        opt_disp.cli_type = None;
                    } else {
                        set_cli_type(&mut opt_disp.cli_type, CliType::Path)?;
                    }
                    Ok(true)
                }
                "expiry-date" => {
                    if negated {
                        opt_disp.cli_type = None;
                    } else {
                        set_cli_type(&mut opt_disp.cli_type, CliType::ExpiryDate)?;
                    }
                    Ok(true)
                }
                "default" => {
                    if negated {
                        opt_disp.default_value = None;
                    } else {
                        opt_disp.default_value = arg.map(|s| s.to_string());
                    }
                    Ok(true)
                }
                "fixed-value" => {
                    flags |= CONFIG_FLAGS_FIXED_VALUE;
                    Ok(true)
                }
                "all" => {
                    flags |= CONFIG_FLAGS_MULTI_REPLACE;
                    Ok(true)
                }
                "value" => {
                    value_slot = arg.map(|s| s.to_string());
                    Ok(true)
                }
                "url" => {
                    url = arg.map(|s| s.to_string());
                    Ok(true)
                }
                "comment" => {
                    comment_slot = arg.map(|s| s.to_string());
                    Ok(true)
                }
                "append" => {
                    append = true;
                    Ok(true)
                }
                _ => Ok(false),
            }
        };

        let (loc, disp, rest) = parse_opts(
            args,
            LEGACY_USAGE,
            true,
            true,
            &mut value_slot,
            &mut comment_slot,
            extra,
        )?;

        let actions_implicit = actions == 0;
        if actions_implicit {
            match rest.len() {
                1 => actions = Action::Get as u32,
                2 => actions = Action::Set as u32,
                3 => actions = Action::SetAll as u32,
                _ => return Err(CommandError::usage("no action specified")),
            }
        }

        // Map remaining args to actions
        if !rest.is_empty() {
            match rest[0].as_str() {
                "list" => actions = Action::List as u32,
                "get" => actions = Action::Get as u32,
                "get-all" => actions = Action::GetAll as u32,
                "get-regexp" => actions = Action::GetRegexp as u32,
                "get-urlmatch" => actions = Action::GetUrlmatch as u32,
                "set" => actions = Action::Set as u32,
                "set-all" => actions = Action::SetAll as u32,
                "replace-all" => actions = Action::ReplaceAll as u32,
                "add" => actions = Action::Add as u32,
                "unset" => actions = Action::Unset as u32,
                "unset-all" => actions = Action::UnsetAll as u32,
                "rename-section" => actions = Action::RenameSection as u32,
                "remove-section" => actions = Action::RemoveSection as u32,
                "edit" => actions = Action::Edit as u32,
                "get-color" => actions = Action::GetColor as u32,
                "get-colorbool" => actions = Action::GetColorBool as u32,
                _ => return Err(CommandError::usage(format!(
                    "unknown subcommand '{}'",
                    rest[0]
                ))),
            }
        }

        // Dispatch to appropriate handler
        match actions {
            x if x == (Action::List as u32) => {
                self.run_list(ctx, &rest[1..], out)
            }
            x if x == (Action::Get as u32) => {
                self.run_get(ctx, &rest[1..], out, &loc, &disp, value_slot.as_deref())
            }
            x if x == (Action::GetAll as u32) => {
                self.run_get_all(ctx, &rest[1..], out, &loc, &disp, value_slot.as_deref())
            }
            x if x == (Action::GetRegexp as u32) => {
                self.run_get_regexp(ctx, &rest[1..], out, &loc, &disp, value_slot.as_deref())
            }
            x if x == (Action::GetUrlmatch as u32) => {
                self.run_get_urlmatch(ctx, &rest[1..], out, &loc, &disp, url.as_deref())
            }
            x if x == (Action::Set as u32) => {
                self.run_set(ctx, &rest[1..], out, &loc, value_slot.as_deref(), comment_slot.as_deref())
            }
            x if x == (Action::SetAll as u32) => {
                self.run_set_all(ctx, &rest[1..], out, &loc, value_slot.as_deref(), comment_slot.as_deref())
            }
            x if x == (Action::ReplaceAll as u32) => {
                self.run_replace_all(ctx, &rest[1..], out, &loc, value_slot.as_deref(), comment_slot.as_deref(), flags)
            }
            x if x == (Action::Add as u32) => {
                self.run_add(ctx, &rest[1..], out, &loc, value_slot.as_deref(), comment_slot.as_deref())
            }
            x if x == (Action::Unset as u32) => {
                self.run_unset(ctx, &rest[1..], out, &loc, value_slot.as_deref(), flags)
            }
            x if x == (Action::UnsetAll as u32) => {
                self.run_unset_all(ctx, &rest[1..], out, &loc, value_slot.as_deref(), flags)
            }
            x if x == (Action::RenameSection as u32) => {
                self.run_rename_section(ctx, &rest[1..], out, &loc)
            }
            x if x == (Action::RemoveSection as u32) => {
                self.run_remove_section(ctx, &rest[1..], out, &loc)
            }
            x if x == (Action::Edit as u32) => {
                self.run_edit(ctx, &rest[1..], out, &loc)
            }
            x if x == (Action::GetColor as u32) => {
                self.run_get_color(ctx, &rest[1..], out, &loc, &disp)
            }
            x if x == (Action::GetColorBool as u32) => {
                self.run_get_colorbool(ctx, &rest[1..], out, &loc, &disp)
            }
            _ => Err(CommandError::usage("no action specified")),
        }
    }
}

// gsd:config-part-8
