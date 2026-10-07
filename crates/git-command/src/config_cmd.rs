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

// ---------------------------------------------------------------------------
// Usage texts (byte-exact, captured from the tree binary)
// ---------------------------------------------------------------------------

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
    --[no-]includes       respect include directives on lookup\n";

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
    --[no-]includes       respect include directives on lookup\n";

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
                          use default value when missing entry\n";

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
    --[no-]append         add a new line without altering any existing values\n";

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
    --[no-]fixed-value    use string equality when comparing values to value pattern\n";

const RENAME_SECTION_USAGE: &str = "usage: git config rename-section [<file-option>] <old-name> <new-name>

Config file location
    --[no-]global         use global config file
    --[no-]system         use system config file
    --[no-]local          use repository config file
    --[no-]worktree       use per-worktree config file
    -f, --[no-]file <file>
                          use given config file
    --[no-]blob <blob-id> read config from given blob object\n";

const REMOVE_SECTION_USAGE: &str = "usage: git config remove-section [<file-option>] <name>

Config file location
    --[no-]global         use global config file
    --[no-]system         use system config file
    --[no-]local          use repository config file
    --[no-]worktree       use per-worktree config file
    -f, --[no-]file <file>
                          use given config file
    --[no-]blob <blob-id> read config from given blob object\n";

const EDIT_USAGE: &str = "usage: git config edit [<file-option>]

Config file location
    --[no-]global         use global config file
    --[no-]system         use system config file
    --[no-]local          use repository config file
    --[no-]worktree       use per-worktree config file
    -f, --[no-]file <file>
                          use given config file
    --[no-]blob <blob-id> read config from given blob object\n";

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
fn unknown_option(usage: &'static str, full: &str) -> CommandError {
    CommandError::usage(format!("error: unknown option `{full}'\n{usage}"))
}

/// C echoes the option exactly as typed (including a `no-`
/// prefix and any `=value`) in unknown-option errors.
fn full_opt_name(name: &str, negated: bool, inline: Option<&str>) -> String {
    let base = if negated { format!("no-{name}") } else { name.to_string() };
    match inline {
        Some(v) => format!("{base}={v}"),
        None => base,
    }
}

/// C `do_get_value` for a `PARSE_OPT_NOARG | PARSE_OPT_NONEG`
/// option (the type flags and the `OPT_CMDMODE` actions):
/// the no- form is not found at all, and `--flag=value` has no
/// value to take.
fn check_cmd_flag(
    name: &str,
    negated: bool,
    inline: Option<&str>,
    usage: &'static str,
) -> Result<(), CommandError> {
    if negated {
        return Err(unknown_option(
            usage,
            &full_opt_name(name, negated, inline),
        ));
    }
    if inline.is_some() {
        return Err(CommandError::usage(format!(
            "error: option `{name}' takes no value"
        )));
    }
    Ok(())
}

/// C `do_get_value`: a negated option never takes a value;
/// neither does a no-value option.
fn check_opt_value(
    name: &str,
    negated: bool,
    noarg: bool,
    inline: Option<&str>,
) -> Result<(), CommandError> {
    if inline.is_some() && (negated || noarg) {
        let full = full_opt_name(name, negated, None);
        // C prints this one-liner without the usage block.
        return Err(CommandError::usage(format!(
            "error: option `{full}' takes no value"
        )));
    }
    Ok(())
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
    take_arg: &mut Option<PendingArg>,
    _usage: &'static str,
    mode: OptMode,
) -> Result<bool, CommandError> {
    // Options taking a separate `--opt value` or `--opt=value` argument.
    if take_arg.is_some() {
        return Ok(false);
    }
    match name {
        "global" => {
            check_opt_value(name, negated, true, value)?;
            loc.global = !negated;
        }
        "system" => {
            check_opt_value(name, negated, true, value)?;
            loc.system = !negated;
        }
        "local" => {
            check_opt_value(name, negated, true, value)?;
            loc.local = !negated;
        }
        "worktree" => {
            check_opt_value(name, negated, true, value)?;
            loc.worktree = !negated;
        }
        "file" => {
            check_opt_value(name, negated, false, value)?;
            if negated {
                loc.file = None;
            } else if let Some(v) = value {
                loc.file = Some(v.to_string());
            } else {
                *take_arg = Some(PendingArg::long("file"));
            }
        }
        "blob" => {
            check_opt_value(name, negated, false, value)?;
            if negated {
                loc.blob = None;
            } else if let Some(v) = value {
                loc.blob = Some(v.to_string());
            } else {
                *take_arg = Some(PendingArg::long("blob"));
            }
        }
        "includes" if mode.allows_includes() => {
            check_opt_value(name, negated, true, value)?;
            loc.includes = Some(!negated);
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// Apply one parsed display option.
#[allow(clippy::too_many_arguments)]
fn apply_disp_opt(
    disp: &mut DispOpts,
    name: &str,
    negated: bool,
    value: Option<&str>,
    take_arg: &mut Option<PendingArg>,
    usage: &'static str,
    mode: OptMode,
    with_type_flags: bool,
    with_default: bool,
) -> Result<bool, CommandError> {
    // Display options exist only in the legacy, get, and list
    // tables (C `CONFIG_DISPLAY_OPTIONS`).
    if !mode.allows_display() {
        return Ok(false);
    }
    match name {
        "null" => {
            check_opt_value(name, negated, true, value)?;
            disp.end_nul = !negated;
        }
        "name-only" => {
            check_opt_value(name, negated, true, value)?;
            disp.omit_values = !negated;
        }
        "show-origin" => {
            check_opt_value(name, negated, true, value)?;
            disp.show_origin = !negated;
        }
        "show-scope" => {
            check_opt_value(name, negated, true, value)?;
            disp.show_scope = !negated;
        }
        "show-names" => {
            check_opt_value(name, negated, true, value)?;
            disp.show_keys = !negated;
        }
        "type" if with_type_flags => {
            check_opt_value(name, negated, false, value)?;
            if negated {
                disp.cli_type = None;
            } else if let Some(v) = value {
                set_cli_type(&mut disp.cli_type, parse_type_value(v)?)?;
            } else {
                *take_arg = Some(PendingArg::long("type"));
            }
        }
        // C `OPT_CALLBACK_VALUE` is PARSE_OPT_NONEG: the no- form
        // is unknown, and `--flag=value` takes no value.
        "bool" if with_type_flags => {
            check_cmd_flag(name, negated, value, usage)?;
            set_cli_type(&mut disp.cli_type, CliType::Bool)?;
        }
        "int" if with_type_flags => {
            check_cmd_flag(name, negated, value, usage)?;
            set_cli_type(&mut disp.cli_type, CliType::Int)?;
        }
        "bool-or-int" if with_type_flags => {
            check_cmd_flag(name, negated, value, usage)?;
            set_cli_type(&mut disp.cli_type, CliType::BoolOrInt)?;
        }
        "bool-or-str" if with_type_flags => {
            check_cmd_flag(name, negated, value, usage)?;
            set_cli_type(&mut disp.cli_type, CliType::BoolOrStr)?;
        }
        "path" if with_type_flags => {
            check_cmd_flag(name, negated, value, usage)?;
            set_cli_type(&mut disp.cli_type, CliType::Path)?;
        }
        "expiry-date" if with_type_flags => {
            check_cmd_flag(name, negated, value, usage)?;
            set_cli_type(&mut disp.cli_type, CliType::ExpiryDate)?;
        }
        "default" if with_default && mode.allows_default() => {
            check_opt_value(name, negated, false, value)?;
            if negated {
                disp.default_value = None;
            } else if let Some(v) = value {
                disp.default_value = Some(v.to_string());
            } else {
                *take_arg = Some(PendingArg::long("default"));
            }
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// A `--opt` still waiting for its separate value argument.
struct PendingArg {
    /// The option's long name: the slot it fills.
    name: &'static str,
    /// The short switch, when the option came from `-x`.
    short: Option<char>,
}

impl PendingArg {
    fn long(name: &'static str) -> Self {
        PendingArg { name, short: None }
    }
    fn short(name: &'static str, c: char) -> Self {
        PendingArg { name, short: Some(c) }
    }
}

/// Outcome of consuming a pending `--opt <arg>` argument.
enum Pending {
    /// No option was pending; the argument is untouched.
    None,
    /// Consumed by a shared slot (file/blob/type/default).
    Consumed,
    /// Re-dispatched through the action-specific `extra`
    /// closure (the `--value`/`--comment` slots live there).
    Redispatch(String, String),
}

/// Consume a pending `--opt value` argument.
fn take_pending(
    loc: &mut LocOpts,
    disp: &mut DispOpts,
    pending: &mut Option<PendingArg>,
    arg: &str,
) -> Result<Pending, CommandError> {
    let Some(p) = pending.take() else {
        return Ok(Pending::None);
    };
    match p.name {
        "file" => loc.file = Some(arg.to_string()),
        "blob" => loc.blob = Some(arg.to_string()),
        "type" => set_cli_type(&mut disp.cli_type, parse_type_value(arg)?)?,
        "default" => disp.default_value = Some(arg.to_string()),
        "value" | "comment" => return Ok(Pending::Redispatch(p.name.to_string(), arg.to_string())),
        _ => {}
    }
    Ok(Pending::Consumed)
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
    mode: OptMode,
    with_type_flags: bool,
    with_default: bool,
    mut extra: impl FnMut(
        &mut LocOpts,
        &mut DispOpts,
        &str,
        bool,
        Option<&str>,
        &mut Option<PendingArg>,
    ) -> Result<bool, CommandError>,
) -> Result<(LocOpts, DispOpts, Vec<String>), CommandError> {
    let mut loc = LocOpts::default();
    let mut disp = DispOpts::default();
    let mut rest: Vec<String> = Vec::new();
    let mut pending: Option<PendingArg> = None;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match take_pending(&mut loc, &mut disp, &mut pending, a)? {
            // `--value <arg>` / `--comment <arg>`: route the
            // separated argument back through `extra`, which owns
            // the action-specific slots.
            Pending::Redispatch(opt, arg) => {
                if extra(&mut loc, &mut disp, &opt, false, Some(&arg), &mut pending)? {
                    i += 1;
                    continue;
                }
                return Err(unknown_option(usage, &opt));
            }
            // The shared slots consumed the argument: it is not
            // a positional.
            Pending::Consumed => {
                i += 1;
                continue;
            }
            Pending::None => {}
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
            if apply_loc_opt(&mut loc, opt, negated, inline, &mut pending, usage, mode)? {
                i += 1;
                continue;
            }
            if apply_disp_opt(&mut disp, opt, negated, inline, &mut pending, usage, mode, with_type_flags, with_default)? {
                i += 1;
                continue;
            }
            if extra(&mut loc, &mut disp, opt, negated, inline, &mut pending)? {
                i += 1;
                continue;
            }
            return Err(unknown_option(
                usage,
                &full_opt_name(opt, negated, inline),
            ));
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
                        pending = Some(PendingArg::short("file", shorts[j]));
                    }
                    break;
                }
                't' => {
                    let rest_chars: String = shorts[j + 1..].iter().collect();
                    if !rest_chars.is_empty() {
                        set_cli_type(&mut disp.cli_type, parse_type_value(&rest_chars)?)?;
                    } else {
                        pending = Some(PendingArg::short("type", shorts[j]));
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
            return Err(CommandError::usage(format!(
                "error: unknown switch `{}'\n{usage}",
                shorts[j]
            )));
        }
        i += 1;
    }
    if let Some(p) = pending.take() {
        // C: `option \`X' requires a value` / `switch \`X' requires a value`.
        let msg = match p.short {
            Some(c) => format!("error: switch `{c}' requires a value"),
            None => format!("error: option `{}' requires a value", p.name),
        };
        // C prints this one-liner without the usage block.
        return Err(CommandError::usage(msg));
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

/// C's display name for a scope-selected file: global/system files
/// print absolute and verbatim (never relativized, even under the
/// cwd); repo files render relative to the work-tree root
/// (`.git/config` from anywhere) or the git dir when bare
/// (`config`), matching C's post-chdir layout.
fn scope_display_name(
    repo: Option<&git_core::Repository>,
    scope: ConfigScope,
    path: &std::path::Path,
) -> String {
    if matches!(scope, ConfigScope::Global | ConfigScope::System) {
        return path.display().to_string();
    }
    if let Some(r) = repo {
        if let Some(wt) = &r.work_tree {
            if let Ok(rel) = path.strip_prefix(wt) {
                if !rel.as_os_str().is_empty() {
                    return rel.display().to_string();
                }
            }
        } else if let Ok(rel) = path.strip_prefix(&r.git_dir) {
            if !rel.as_os_str().is_empty() {
                return rel.display().to_string();
            }
        }
    }
    display_path(path)
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
        let set = match ConfigSet::parse(&obj.data) {
            Ok(parsed) => parsed,
            Err(ConfigError::BadLine { line, .. }) => {
                // C reports blob parse errors via error() + the list-path
                // fatal (probed on the tree binary).
                eprintln!("error: bad config line {line} in blob {oid}");
                return Err(CommandError::fatal("fatal: error processing config file(s)"));
            }
            Err(e) => {
                return Err(CommandError::fatal(format!("fatal: {e}")));
            }
        };
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
        let disk = if raw.is_absolute() { raw.clone() } else { config_file_base(ctx, repo).join(&raw) };
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
        let display = scope_display_name(repo, scope, &path);
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
        // C skips include-following on the layered read under
        // `--no-includes` (the flag defaults on only with no selector).
        let scoped = ConfigSet::load_repo_scoped(&scopes, respect).map_err(|e| match e {
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
                .map(|p| scope_display_name(Some(repo), s.scope, p))
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
            let display = scope_display_name(repo, scope, &path);
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
/// C `format_config` result: a formatted value, a valueless
/// key (C backs out the key delimiter and shows the key alone),
/// or a skipped (`:(optional)` path missing) entry.
enum Formatted {
    Value(String),
    Valueless,
    Skip,
}

fn format_typed(
    cli_type: Option<CliType>,
    key_dotted: &str,
    value: Option<&str>,
    loc: &TypeLoc,
) -> Result<Formatted, TypeError> {
    let Some(ty) = cli_type else {
        return Ok(match value {
            Some(v) => Formatted::Value(v.to_string()),
            None => Formatted::Valueless,
        });
    };
    match ty {
        CliType::Bool => {
            let Some(v) = value else {
                return Ok(Formatted::Value("true".to_string()));
            };
            parse_bool(v)
                .map(|b| Formatted::Value(b.to_string()))
                .ok_or_else(|| loc.render(format!("bad boolean config value '{v}' for '{key_dotted}'")))
        }
        CliType::Int => {
            let v = value.unwrap_or("");
            canonicalize_typed(ConfigValueType::Int, key_dotted, v)
                .map(Formatted::Value)
                .map_err(|e| loc.int_error(&e.to_string()))
        }
        CliType::BoolOrInt => {
            if let Some(b) = value.and_then(parse_bool_text) {
                // C `git_parse_maybe_bool_text` (empty counts as false).
                return Ok(Formatted::Value(b.to_string()));
            }
            if value.is_none() {
                return Ok(Formatted::Value("false".to_string()));
            }
            let v = value.unwrap_or("");
            canonicalize_typed(ConfigValueType::BoolOrInt, key_dotted, v)
                .map(Formatted::Value)
                .map_err(|e| loc.int_error(&e.to_string()))
        }
        CliType::BoolOrStr => {
            let Some(v) = value else {
                return Ok(Formatted::Value("true".to_string()));
            };
            Ok(Formatted::Value(
                parse_bool(v).map(|b| b.to_string()).unwrap_or_else(|| v.to_string()),
            ))
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
                        return Ok(Formatted::Skip);
                    }
                    Ok(Formatted::Value(expanded))
                }
                Err(msg) => Err(loc.render(msg)),
            }
        }
        CliType::ExpiryDate => {
            let Some(v) = value else {
                return Err(loc.missing(key_dotted));
            };
            parse_expiry(v)
                .map(|t| Formatted::Value(t.to_string()))
                .ok_or_else(|| loc.render(format!("'{v}' for '{key_dotted}' is not a valid timestamp")))
        }
        CliType::Color => {
            let Some(v) = value else {
                return Err(loc.missing(key_dotted));
            };
            color_parse_value(v)
                .map(Formatted::Value)
                .ok_or_else(|| loc.color_error(v))
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


// ---------------------------------------------------------------------------
// Matching + output (C `collect_config`, `show_all_config`, `get_value`)
// ---------------------------------------------------------------------------

/// Canonical dotted name for an entry (C lowercases section+key, keeps
/// subsection case).
fn dotted_name(e: &ConfigEntry) -> String {
    let section = e.section.to_ascii_lowercase();
    let key = e.key.to_ascii_lowercase();
    match &e.subsection {
        Some(sub) => format!("{section}.{sub}.{key}"),
        // Before any section header C's var has no stem at all.
        None if section.is_empty() => key,
        None => format!("{section}.{key}"),
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
    formatted_value: Formatted,
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
    match formatted_value {
        // C `format_config_path`: `:(optional)` missing files
        // make format_config return -1 and the caller skips.
        Formatted::Skip => None,
        // C `format_config` TYPE_NONE with a NULL value: the
        // key delimiter is backed out, only the key is shown.
        Formatted::Valueless => {
            line.push(term);
            Some(line)
        }
        Formatted::Value(value) => {
            if disp.show_keys {
                line.push(key_delim);
            }
            line.push_str(&value);
            line.push(term);
            Some(line)
        }
    }
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
        if matches!(formatted, Formatted::Skip) {
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
            if let Formatted::Value(value) = formatted {
                if let Some(line) = render_entry(disp, &fake, Formatted::Value(value), key_delim) {
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

// ---------------------------------------------------------------------------
// Write path (C `repo_config_set[_multivar]_in_file_gently`)
// ---------------------------------------------------------------------------

/// How a value pattern constrains matches (C `config_store_data`:
/// `value_pattern == NULL` matches all, `CONFIG_REGEX_NONE` matches
/// none, else a regex or fixed string).
enum WritePattern {
    /// No pattern: every entry with the key matches.
    All,
    /// C `CONFIG_REGEX_NONE`: no entry matches (only add a new one).
    None_,
    /// Regex (or fixed string when `fixed`).
    Pat(String),
}

/// C `normalize_value`: apply the display type to a value being
/// written (`builtin/config.c`).
fn normalize_value(
    key: &str,
    value: &str,
    ty: Option<CliType>,
) -> Result<String, CommandError> {
    let Some(ty) = ty else {
        return Ok(value.to_string());
    };
    match ty {
        CliType::Path | CliType::ExpiryDate => Ok(value.to_string()),
        CliType::Int => canonicalize_typed(ConfigValueType::Int, key, value)
            .map_err(|e| CommandError::fatal(format!("fatal: {e}"))),
        CliType::Bool => parse_bool(value)
            .map(|b| b.to_string())
            .ok_or_else(|| {
                CommandError::fatal(format!(
                    "fatal: bad boolean config value '{value}' for '{key}'"
                ))
            }),
        CliType::BoolOrInt => {
            if let Some(b) = parse_bool_text(value) {
                return Ok(b.to_string());
            }
            canonicalize_typed(ConfigValueType::BoolOrInt, key, value)
                .map_err(|e| CommandError::fatal(format!("fatal: {e}")))
        }
        CliType::BoolOrStr => Ok(parse_bool(value)
            .map(|b| b.to_string())
            .unwrap_or_else(|| value.to_string())),
        CliType::Color => color_parse_value(value)
            .map(|_| value.to_string())
            .ok_or_else(|| {
                CommandError::fatal(format!("fatal: cannot parse color '{value}'"))
            }),
    }
}

/// C `git_config_prepare_comment_string`: a comment beginning with
/// whitespace then `#` is used as-is; a bare `#` gets a leading SP;
/// anything else becomes ` # comment`.
fn prepare_comment(comment: &str) -> Result<String, CommandError> {
    if comment.contains('\n') {
        return Err(CommandError::fatal(format!(
            "fatal: no multi-line comment allowed: '{comment}'"
        )));
    }
    let leading = comment.len() - comment.trim_start_matches([' ', '\t']).len();
    if leading > 0 && comment.as_bytes()[leading] == b'#' {
        return Ok(comment.to_string());
    }
    if comment.starts_with('#') {
        return Ok(format!(" {comment}"));
    }
    Ok(format!(" # {comment}"))
}

/// Apply C `git_config_prepare_comment_string` to the
/// `--comment` payload (dies on multi-line input).
fn prepare_comment_opt(comment: Option<&str>) -> Result<Option<String>, CommandError> {
    comment.map(prepare_comment).transpose()
}

/// C `check_write`: writes need a resolved config file or a
/// repository (exit 128 with C's exact texts). Runs after
/// `location_options_init`, so every selector but `-f -`
/// (stdin resets the file to NULL) counts as having a file.
fn check_write(
    loc: &LocOpts,
    repo: Option<&git_core::Repository>,
) -> Result<(), CommandError> {
    let has_file = loc
        .file
        .as_deref()
        .map(|f| f != "-")
        .unwrap_or(false)
        || loc.global
        || loc.system
        || loc.local
        || loc.worktree;
    if !has_file && repo.is_none() {
        return Err(CommandError::fatal("fatal: not in a git directory"));
    }
    if loc.file.as_deref() == Some("-") {
        return Err(CommandError::fatal("fatal: writing to stdin is not supported"));
    }
    if loc.blob.is_some() {
        return Err(CommandError::fatal("fatal: writing config blobs is not supported"));
    }
    Ok(())
}

/// C `section_name_is_ok`: non-empty, alphanumeric or dash before
/// the first dot.
fn section_name_is_ok(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    name.bytes()
        .take_while(|&b| b != b'.')
        .all(|b| b == b'-' || b.is_ascii_alphanumeric())
}

/// Resolve the file a write targets (C `location_options_init`
/// write checks + `check_write`, then the selector default).
fn write_target_file(
    ctx: &RepoContext,
    repo: Option<&git_core::Repository>,
    loc: &LocOpts,
) -> Result<PathBuf, CommandError> {
    // C `location_options_init`: mutual exclusion and the
    // repo-requiring selectors.
    let n_selectors = [loc.global, loc.system, loc.local, loc.worktree]
        .iter()
        .filter(|b| **b)
        .count()
        + loc.file.is_some() as usize
        + loc.blob.is_some() as usize;
    if n_selectors > 1 {
        return Err(CommandError::usage("error: only one config file at a time"));
    }
    if repo.is_none() {
        if loc.local {
            return Err(CommandError::fatal(
                "fatal: --local can only be used inside a git repository",
            ));
        }
        if loc.worktree {
            return Err(CommandError::fatal(
                "fatal: --worktree can only be used inside a git repository",
            ));
        }
    }
    check_write(loc, repo)?;
    if let Some(file) = &loc.file {
        let raw = PathBuf::from(file);
        return Ok(if raw.is_absolute() { raw } else { config_file_base(ctx, repo).join(&raw) });
    }
    if loc.global {
        return git_config::global_config_file()
            .ok_or_else(|| CommandError::fatal("fatal: $HOME not set"));
    }
    if loc.system {
        return Ok(git_config::system_config_file());
    }
    if loc.local || loc.worktree {
        let repo = repo.ok_or_else(|| {
            CommandError::fatal("fatal: --local can only be used inside a git repository")
        })?;
        return Ok(scope_file(repo, loc)?.0);
    }
    let repo = repo.ok_or_else(|| CommandError::fatal("fatal: not in a git directory"))?;
    Ok(repo.common_dir.join("config"))
}

/// C `repo_config_set_multivar_in_file_gently` over the `file.rs`
/// splicer: replace (or, for `value == None`, remove) every entry
/// matching key + pattern, appending when nothing matched and a
/// value is given. `multi_replace` collapses several matches into
/// one new pair (C's multi-replace flag); without it, more
/// than one match is C's `CONFIG_NOTHING_SET` (warning + exit 5).
fn config_multivar_write(
    path: &Path,
    section: &str,
    subsection: Option<&str>,
    key: &str,
    value: Option<&str>,
    pattern: &WritePattern,
    fixed: bool,
    multi_replace: bool,
    comment: Option<&str>,
) -> Result<(), CommandError> {
    let dotted = match subsection {
        Some(sub) => format!("{section}.{sub}.{key}"),
        None => format!("{section}.{key}"),
    };
    // Count matches (C `store_aux` counting via `matches`).
    let matcher = match pattern {
        WritePattern::All => None,
        WritePattern::None_ => return config_write_add(path, section, subsection, key, value, comment),
        WritePattern::Pat(p) => {
            Some(if fixed {
                cfgfile::ValueMatcher::Fixed(p.clone())
            } else {
                cfgfile::ValueMatcher::compile(p, false).map_err(|_| {
                    CommandError::error(format!("error: invalid pattern: {p}"))
                })?
            })
        }
    };
    let text = read_file_text(path, &display_path(path))?;
    let matches =
        cfgfile::count_matches(&text, section, subsection, key, matcher.as_ref());

    if value.is_none() {
        // Unset: nothing matched (or several without multi-replace)
        // is CONFIG_NOTHING_SET; several with it removes them all.
        if matches == 0 {
            return Err(CommandError::silent(5));
        }
        if matches > 1 && !multi_replace {
            eprintln!("warning: {dotted} has multiple values");
            return Err(CommandError::silent(5));
        }
        let (new_text, _) = cfgfile::unset_all(&text, section, subsection, key, matcher.as_ref());
        return cfgfile::write_config_file(path, &new_text).map_err(|_| {
            CommandError::error(format!("error: could not write config file {}", path.display()))
        });
    }
    let value = value.unwrap();
    if matches == 0 {
        // No match: append the new pair (C writes section + pair).
        let new_text = cfgfile::add_value(&text, section, subsection, key, value, comment);
        return cfgfile::write_config_file(path, &new_text).map_err(|_| {
            CommandError::error(format!("error: could not write config file {}", path.display()))
        });
    }
    if matches > 1 && !multi_replace {
        eprintln!("warning: {dotted} has multiple values");
        return Err(CommandError::silent(5));
    }
    // Replace every match with the single new pair.
    let (new_text, _) = cfgfile::replace_all(
        &text,
        section,
        subsection,
        key,
        value,
        matcher.as_ref(),
        comment,
    );
    cfgfile::write_config_file(path, &new_text)
        .map_err(|_| CommandError::error(format!(
            "error: could not write config file {}",
            path.display()
        )))
}

/// C append path for `CONFIG_REGEX_NONE` (never matches): the new
/// pair is always appended. `value` is always `Some` here.
fn config_write_add(
    path: &Path,
    section: &str,
    subsection: Option<&str>,
    key: &str,
    value: Option<&str>,
    comment: Option<&str>,
) -> Result<(), CommandError> {
    let text = read_file_text(path, &display_path(path))?;
    let value = value.unwrap_or("true");
    let new_text = cfgfile::add_value(&text, section, subsection, key, value, comment);
    cfgfile::write_config_file(path, &new_text)
        .map_err(|_| CommandError::error(format!(
            "error: could not write config file {}",
            path.display()
        )))
}

/// C `check_argc`: `wrong number of arguments` then exit 129.
fn check_argc(argc: usize, min: usize, max: usize) -> Result<(), CommandError> {
    if argc >= min && argc <= max {
        return Ok(());
    }
    if min == max {
        Err(CommandError::usage(format!(
            "error: wrong number of arguments, should be {min}"
        )))
    } else {
        Err(CommandError::usage(format!(
            "error: wrong number of arguments, should be from {min} to {max}"
        )))
    }
}

/// C `prefix_filename` via `OPT_FILENAME`: a relative `-f` path
/// carries the repository prefix (the working directory relative
/// to the top level). Absolute paths, `-` (stdin), and paths given
/// outside a repository are left alone.
fn prefix_config_file(ctx: &RepoContext, repo: Option<&git_core::Repository>, file: &str) -> String {
    if file.is_empty() || file == "-" || file.starts_with('/') {
        return file.to_string();
    }
    let Some(work_tree) = repo.and_then(|r| r.work_tree.clone()) else {
        return file.to_string();
    };
    match ctx.cwd.strip_prefix(&work_tree) {
        Ok(rel) if rel.as_os_str().is_empty() => file.to_string(),
        Ok(rel) => format!("{}/{}", rel.to_string_lossy(), file),
        Err(_) => file.to_string(),
    }
}

/// Resolution base for a relative `-f` path: C stores the prefixed
/// name, which resolves against the top level (the process cwd
/// outside a repository).
fn config_file_base(ctx: &RepoContext, repo: Option<&git_core::Repository>) -> PathBuf {
    repo.and_then(|r| r.work_tree.clone()).unwrap_or_else(|| ctx.cwd.clone())
}

/// Split a write key into (section, subsection, key) with C's
/// `git_config_parse_key` errors (exit 128 via `git_config_parse_key`
/// die texts).
fn parse_write_key(name: &str) -> Result<(String, Option<String>, String), CommandError> {
    match parse_key_name(name) {
        Ok(k) => Ok((k.section, k.subsection, k.key)),
        // C: `ret = 0 - git_config_parse_key(...)` flips the
        // negative return back to the positive exit code (1/2).
        Err((code, msg)) => Err(CommandError {
            message: format!("error: {msg}"),
            code,
        }),
    }
}

/// Read the target file's current text (missing reads as empty).
fn read_file_text(path: &Path, display: &str) -> Result<String, CommandError> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(String::new());
    };
    // C runs `git_parse_source` over the file before splicing, so
    // a malformed line is fatal rather than silently overwritten.
    if let Some(line) = cfgfile::bad_line(&text) {
        return Err(CommandError::fatal(format!(
            "fatal: bad config line {line} in file {display}"
        )));
    }
    Ok(text)
}

/// C's display name for the file a write touches: the `-f`
/// argument as given (with the repository prefix), or the
/// repository-relative config path.
fn write_display_name(ctx: &RepoContext, repo: Option<&git_core::Repository>, loc: &LocOpts) -> String {
    if let Some(file) = &loc.file {
        return file.clone();
    }
    let scope = if loc.global {
        ConfigScope::Global
    } else if loc.system {
        ConfigScope::System
    } else {
        ConfigScope::Local
    };
    match write_target_file(ctx, repo, loc) {
        Ok(p) => scope_display_name(repo, scope, &p),
        Err(_) => ".git/config".to_string(),
    }
}

/// Spawn `$GIT_EDITOR`/`core.editor`/`$VISUAL`/`$EDITOR` on the
/// config file (C `launch_editor`, shell mode). `":"` is the null
/// editor. Returns C's error texts and exit codes.
fn launch_editor_on(ctx: &RepoContext, repo: Option<&git_core::Repository>, path: &Path) -> Result<(), CommandError> {
    // C `git_editor`: GIT_EDITOR, core.editor, VISUAL (not dumb),
    // EDITOR, DEFAULT_EDITOR (not dumb), else NULL.
    let dumb = std::env::var("TERM").map(|t| t == "dumb").unwrap_or(true)
        || !std::io::IsTerminal::is_terminal(&std::io::stdin());
    let core_editor = repo
        .and_then(|r| r.config.get("core", "editor").map(|s| s.to_string()))
        .or_else(|| {
            ctx.repository()
                .ok()
                .and_then(|r| r.config.get("core", "editor").map(|s| s.to_string()))
        });
    let editor = std::env::var("GIT_EDITOR").ok()
        .or(core_editor)
        .or_else(|| if !dumb { std::env::var("VISUAL").ok() } else { None })
        .or_else(|| std::env::var("EDITOR").ok());
    let editor = match editor {
        Some(e) if !e.is_empty() => e,
        _ if dumb => {
            return Err(CommandError::error("error: Terminal is dumb, but EDITOR unset"));
        }
        _ => "vi".to_string(),
    };
    if editor == ":" {
        return Ok(());
    }
    // C runs the editor through the shell (`p.use_shell = 1`).
    let realpath = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let realpath_s = realpath.display().to_string();
    let quoted = if realpath_s
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_'))
    {
        realpath_s.clone()
    } else {
        format!("'{}'", realpath_s.replace('\'', "'\\''"))
    };
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} {quoted}"))
        .status()
        .map_err(|_| CommandError::error(format!("error: unable to start editor '{editor}'")))?;
    if !status.success() {
        return Err(CommandError::error(format!(
            "error: there was a problem with the editor '{editor}'"
        )));
    }
    Ok(())
}

/// C `default_user_config` template for a fresh `--global` edit.
fn default_user_config() -> String {
    let name = std::env::var("GIT_AUTHOR_NAME")
        .ok()
        .or_else(|| std::env::var("EMAIL").ok())
        .unwrap_or_else(|| "unknown".to_string());
    let email = std::env::var("GIT_AUTHOR_EMAIL")
        .ok()
        .or_else(|| std::env::var("EMAIL").ok())
        .unwrap_or_else(|| "unknown".to_string());
    format!(
        "# This is Git's per-user configuration file.\n\
         [user]\n\
         # Please adapt and uncomment the following lines:\n\
         #\tname = {name}\n\
         #\temail = {email}\n"
    )
}

pub struct Config;

/// The seven subcommands (C `OPT_SUBCOMMAND` in `cmd_config`).
const SUBCOMMANDS: &[&str] = &[
    "list",
    "get",
    "set",
    "unset",
    "rename-section",
    "remove-section",
    "edit",
];

// C `ACTION_*` bitfield (builtin/config.c).
const A_GET: u32 = 1 << 0;
const A_GET_ALL: u32 = 1 << 1;
const A_GET_REGEXP: u32 = 1 << 2;
const A_REPLACE_ALL: u32 = 1 << 3;
const A_ADD: u32 = 1 << 4;
const A_UNSET: u32 = 1 << 5;
const A_UNSET_ALL: u32 = 1 << 6;
const A_RENAME_SECTION: u32 = 1 << 7;
const A_REMOVE_SECTION: u32 = 1 << 8;
const A_LIST: u32 = 1 << 9;
const A_EDIT: u32 = 1 << 10;
const A_SET: u32 = 1 << 11;
const A_SET_ALL: u32 = 1 << 12;
const A_GET_COLOR: u32 = 1 << 13;
const A_GET_COLORBOOL: u32 = 1 << 14;
const A_GET_URLMATCH: u32 = 1 << 15;

/// Which option groups a parse mode accepts. C gives each
/// subcommand its own option table (`cmd_config_get` has
/// display options, `cmd_config_set` does not, ...).
#[derive(Clone, Copy, PartialEq, Eq)]
enum OptMode {
    /// `cmd_config_actions` (legacy spellings).
    Legacy,
    /// `cmd_config_get`.
    Get,
    /// `cmd_config_set`.
    Set,
    /// `cmd_config_unset`.
    Unset,
    /// `cmd_config_list`.
    List,
    /// `cmd_config_rename_section`/`remove_section`/`edit`.
    LocationOnly,
}

impl OptMode {
    /// Filter options `--all`/`--value` (get, set, unset).
    fn allows_all(self) -> bool {
        matches!(self, OptMode::Get | OptMode::Set | OptMode::Unset)
    }
    /// `--regexp` (get only).
    fn allows_regexp(self) -> bool {
        matches!(self, OptMode::Get)
    }
    /// `--value` (get, set, unset).
    fn allows_value(self) -> bool {
        matches!(self, OptMode::Get | OptMode::Set | OptMode::Unset)
    }
    /// `--url` (get only).
    fn allows_url(self) -> bool {
        matches!(self, OptMode::Get)
    }
    /// `--append` (set only).
    fn allows_append(self) -> bool {
        matches!(self, OptMode::Set)
    }
    /// Display options: `-z`, `--name-only`, `--show-*`, type flags.
    fn allows_display(self) -> bool {
        matches!(self, OptMode::Legacy | OptMode::Get | OptMode::List)
    }
    /// `--includes`.
    fn allows_includes(self) -> bool {
        matches!(self, OptMode::Legacy | OptMode::Get | OptMode::List)
    }
    /// `--default`.
    fn allows_default(self) -> bool {
        matches!(self, OptMode::Legacy | OptMode::Get)
    }
    /// `--fixed-value`.
    fn allows_fixed_value(self) -> bool {
        !matches!(self, OptMode::List | OptMode::LocationOnly)
    }
    /// `--comment`.
    fn allows_comment(self) -> bool {
        matches!(self, OptMode::Legacy | OptMode::Set)
    }
    /// Action flags (`--get`, `--add`, ...).
    fn allows_actions(self) -> bool {
        matches!(self, OptMode::Legacy)
    }
}

/// Action-specific options. `--all`/`--regexp`/`--value`/
/// `--url`/`--append` exist only in the subcommand handlers
/// (C parses them with the subcommand's own option table);
/// the legacy spellings set `actions` instead.
#[derive(Default)]
struct Parsed {
    value_pattern: Option<String>,
    comment: Option<String>,
    url: Option<String>,
    fixed: bool,
    all: bool,
    regexp: bool,
    append: bool,
    actions: u32,
}

/// Parse the shared option surface. `mode` rejects the options
/// that C's per-subcommand option tables do not accept.
fn parse_config_opts(
    args: &[String],
    usage: &'static str,
    mode: OptMode,
) -> Result<(LocOpts, DispOpts, Vec<String>, Parsed), CommandError> {
    let mut parsed = Parsed::default();
    let extra = |opt_loc: &mut LocOpts,
                     opt_disp: &mut DispOpts,
                     opt: &str,
                     negated: bool,
                     arg: Option<&str>,
                     slot: &mut Option<PendingArg>|
     -> Result<bool, CommandError> {
        // C `OPT_CALLBACK_VALUE` is PARSE_OPT_NONEG: the no- form
        // of the type flags is unknown.
        let type_flag = |name: &str| check_cmd_flag(name, negated, arg, usage);
        match opt {
            "includes" if mode.allows_includes() => {
                check_opt_value(opt, negated, true, arg)?;
                opt_loc.includes = Some(!negated);
                Ok(true)
            }
            "file" => {
                check_opt_value(opt, negated, false, arg)?;
                opt_loc.file = arg.map(|s| s.to_string());
                Ok(true)
            }
            "blob" => {
                check_opt_value(opt, negated, false, arg)?;
                opt_loc.blob = arg.map(|s| s.to_string());
                Ok(true)
            }
            "global" => {
                check_opt_value(opt, negated, true, arg)?;
                opt_loc.global = !negated;
                Ok(true)
            }
            "system" => {
                check_opt_value(opt, negated, true, arg)?;
                opt_loc.system = !negated;
                Ok(true)
            }
            "local" => {
                check_opt_value(opt, negated, true, arg)?;
                opt_loc.local = !negated;
                Ok(true)
            }
            "worktree" => {
                check_opt_value(opt, negated, true, arg)?;
                opt_loc.worktree = !negated;
                Ok(true)
            }
            "null" if mode.allows_display() => {
                check_opt_value(opt, negated, true, arg)?;
                opt_disp.end_nul = !negated;
                Ok(true)
            }
            "name-only" if mode.allows_display() => {
                check_opt_value(opt, negated, true, arg)?;
                opt_disp.omit_values = !negated;
                Ok(true)
            }
            "show-origin" if mode.allows_display() => {
                check_opt_value(opt, negated, true, arg)?;
                opt_disp.show_origin = !negated;
                Ok(true)
            }
            "show-scope" if mode.allows_display() => {
                check_opt_value(opt, negated, true, arg)?;
                opt_disp.show_scope = !negated;
                Ok(true)
            }
            "show-names" if mode.allows_display() => {
                check_opt_value(opt, negated, true, arg)?;
                opt_disp.show_keys = !negated;
                Ok(true)
            }
            "type" if mode.allows_display() => {
                check_opt_value(opt, negated, false, arg)?;
                if negated {
                    opt_disp.cli_type = None;
                } else {
                    let ty = parse_type_name(
                        arg.ok_or_else(|| unknown_option(usage, "--type"))?,
                    )
                    .ok_or_else(|| unknown_option(usage, "--type"))?;
                    set_cli_type(&mut opt_disp.cli_type, ty)?;
                }
                Ok(true)
            }
            "bool" if mode.allows_display() => {
                type_flag(opt)?;
                set_cli_type(&mut opt_disp.cli_type, CliType::Bool)?;
                Ok(true)
            }
            "int" if mode.allows_display() => {
                type_flag(opt)?;
                set_cli_type(&mut opt_disp.cli_type, CliType::Int)?;
                Ok(true)
            }
            "bool-or-int" if mode.allows_display() => {
                type_flag(opt)?;
                set_cli_type(&mut opt_disp.cli_type, CliType::BoolOrInt)?;
                Ok(true)
            }
            "bool-or-str" if mode.allows_display() => {
                type_flag(opt)?;
                set_cli_type(&mut opt_disp.cli_type, CliType::BoolOrStr)?;
                Ok(true)
            }
            "path" if mode.allows_display() => {
                type_flag(opt)?;
                set_cli_type(&mut opt_disp.cli_type, CliType::Path)?;
                Ok(true)
            }
            "expiry-date" if mode.allows_display() => {
                type_flag(opt)?;
                set_cli_type(&mut opt_disp.cli_type, CliType::ExpiryDate)?;
                Ok(true)
            }
            "default" if mode.allows_default() => {
                check_opt_value(opt, negated, false, arg)?;
                if negated {
                    opt_disp.default_value = None;
                } else {
                    opt_disp.default_value = arg.map(|s| s.to_string());
                }
                Ok(true)
            }
            "fixed-value" if mode.allows_fixed_value() => {
                check_opt_value(opt, negated, true, arg)?;
                if !negated {
                    parsed.fixed = true;
                }
                Ok(true)
            }
            "comment" if mode.allows_comment() => {
                check_opt_value(opt, negated, false, arg)?;
                if negated {
                    parsed.comment = None;
                } else if let Some(v) = arg {
                    parsed.comment = Some(v.to_string());
                } else {
                    // `--comment <arg>`: defer to the pending
                    // mechanism, which re-dispatches here.
                    *slot = Some(PendingArg::long("comment"));
                }
                Ok(true)
            }
            // Filter options: subcommand handlers only.
            "all" if mode.allows_all() => {
                check_opt_value(opt, negated, true, arg)?;
                if !negated {
                    parsed.all = true;
                }
                Ok(true)
            }
            "regexp" if mode.allows_regexp() => {
                check_opt_value(opt, negated, true, arg)?;
                if !negated {
                    parsed.regexp = true;
                }
                Ok(true)
            }
            "value" if mode.allows_value() => {
                check_opt_value(opt, negated, false, arg)?;
                if negated {
                    parsed.value_pattern = None;
                } else if let Some(v) = arg {
                    parsed.value_pattern = Some(v.to_string());
                } else {
                    *slot = Some(PendingArg::long("value"));
                }
                Ok(true)
            }
            "url" if mode.allows_url() => {
                check_opt_value(opt, negated, false, arg)?;
                parsed.url = arg.map(|s| s.to_string());
                Ok(true)
            }
            "append" if mode.allows_append() => {
                check_opt_value(opt, negated, true, arg)?;
                if !negated {
                    parsed.append = true;
                }
                Ok(true)
            }
            // Action flags (C `OPT_CMDMODE` legacy spellings).
            "get" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_GET;
                Ok(true)
            }
            "get-all" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_GET_ALL;
                Ok(true)
            }
            "get-regexp" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_GET_REGEXP;
                Ok(true)
            }
            "get-urlmatch" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_GET_URLMATCH;
                Ok(true)
            }
            "replace-all" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_REPLACE_ALL;
                Ok(true)
            }
            "add" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_ADD;
                Ok(true)
            }
            "unset" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_UNSET;
                Ok(true)
            }
            "unset-all" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_UNSET_ALL;
                Ok(true)
            }
            "rename-section" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_RENAME_SECTION;
                Ok(true)
            }
            "remove-section" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_REMOVE_SECTION;
                Ok(true)
            }
            "list" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_LIST;
                Ok(true)
            }
            "edit" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_EDIT;
                Ok(true)
            }
            "list-short" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_LIST;
                Ok(true)
            }
            "edit-short" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_EDIT;
                Ok(true)
            }
            "get-color" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_GET_COLOR;
                Ok(true)
            }
            "get-colorbool" if mode.allows_actions() => {
                check_cmd_flag(opt, negated, arg, usage)?;
                parsed.actions |= A_GET_COLORBOOL;
                Ok(true)
            }
            _ => Ok(false),
        }
    };

    let (loc, disp, rest) = parse_opts(args, usage, mode, true, true, extra)?;
    Ok((loc, disp, rest, parsed))
}

/// Per-subcommand usage block (C passes each handler's own
/// usage to `parse_options`).
fn subcommand_usage(name: &str) -> &'static str {
    match name {
        "list" => LIST_USAGE,
        "get" => GET_USAGE,
        "set" => SET_USAGE,
        "unset" => UNSET_USAGE,
        "rename-section" => RENAME_SECTION_USAGE,
        "remove-section" => REMOVE_SECTION_USAGE,
        "edit" => EDIT_USAGE,
        _ => LEGACY_USAGE,
    }
}

/// C `die_missing_set_value`: the implicit `name=value`
/// spelling (exit 129, with a hint when the prefix before
/// `=` is a valid key).
fn die_missing_set_value(arg: &str) -> CommandError {
    let last_dot = arg.rfind('.');
    let eq = last_dot.and_then(|d| arg[d + 1..].find('=').map(|e| d + 1 + e));
    let prefix = eq.map(|e| &arg[..e]);
    let valid = |key: &str| parse_key_name(key).is_ok();
    if let Some(p) = prefix {
        if valid(p) {
            let value = &arg[eq.unwrap() + 1..];
            return CommandError {
                message: format!(
                    "error: missing value to set to the variable '{arg}'\n\
                     hint: did you mean \"git config set {p} {value}\"?"
                ),
                code: 129,
            };
        }
    }
    if valid(arg) {
        CommandError {
            message: format!("error: missing value to set to the variable '{arg}'"),
            code: 129,
        }
    } else {
        CommandError {
            message: format!(
                "error: missing value to set to a variable with an invalid name '{arg}'"
            ),
            code: 129,
        }
    }
}

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
        // C's first pass keeps every argument and scans for a
        // subcommand token: the first non-option argument that
        // names a subcommand selects the subcommand mode (its
        // handler re-parses every other argument with its own
        // table); any other first non-option ends the scan and
        // selects the legacy flag mode.
        let sub_idx = args
            .iter()
            .position(|a| !a.starts_with('-') && SUBCOMMANDS.contains(&a.as_str()));

        if let Some(idx) = sub_idx {
            let name = args[idx].as_str();
            let mut stream = Vec::with_capacity(args.len().saturating_sub(1));
            stream.extend_from_slice(&args[..idx]);
            stream.extend_from_slice(&args[idx + 1..]);
            let usage = subcommand_usage(name);
            self.run_subcommand(ctx, name, &stream, usage, out)
        } else {
            self.run_legacy(ctx, args, out)
        }
    }
}

impl Config {
    /// Subcommand mode (C `cmd_config_<name>` handlers).
    fn run_subcommand(
        &self,
        ctx: &RepoContext,
        name: &str,
        args: &[String],
        usage: &'static str,
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        let repo_opt = ctx.repository_opt()?;
        let repo = repo_opt.as_ref();
        let mode = match name {
            "get" => OptMode::Get,
            "set" => OptMode::Set,
            "unset" => OptMode::Unset,
            "list" => OptMode::List,
            _ => OptMode::LocationOnly,
        };
        let (mut loc, disp, rest, parsed) = parse_config_opts(args, usage, mode)?;
        if let Some(f) = &loc.file {
            loc.file = Some(prefix_config_file(ctx, repo, f));
        }

        match name {
            "list" => {
                check_argc(rest.len(), 0, 0)?;
                self.run_list(ctx, repo, &loc, &disp, out)
            }
            "get" => {
                check_argc(rest.len(), 1, 1)?;
                if parsed.fixed && parsed.value_pattern.is_none() {
                    return Err(CommandError::usage(
                        "error: --fixed-value only applies with 'value-pattern'",
                    ));
                }
                if disp.default_value.is_some() && (parsed.all || parsed.url.is_some()) {
                    return Err(CommandError::usage(
                        "error: --default= cannot be used with --all or --url=",
                    ));
                }
                if parsed.url.is_some()
                    && (parsed.all || parsed.regexp || parsed.value_pattern.is_some())
                {
                    return Err(CommandError::usage(
                        "error: --url= cannot be used with --all, --regexp or --value",
                    ));
                }
                if let Some(url) = parsed.url.as_deref() {
                    return self.run_get_urlmatch(ctx, repo, &rest, &loc, &disp, url, out);
                }
                if disp.cli_type == Some(CliType::Color)
                    && rest.first().map(|s| s.is_empty()).unwrap_or(false)
                    && disp.default_value.is_some()
                {
                    return self.run_get_color(ctx, repo, &rest, &loc, &disp, out);
                }
                self.run_get(
                    ctx,
                    repo,
                    &rest,
                    &loc,
                    &disp,
                    parsed.value_pattern.as_deref(),
                    parsed.all,
                    parsed.regexp,
                    parsed.fixed,
                    out,
                )
            }
            "set" => {
                if rest.len() == 1 {
                    return Err(die_missing_set_value(&rest[0]));
                }
                check_argc(rest.len(), 2, 2)?;
                if parsed.fixed && parsed.value_pattern.is_none() {
                    return Err(CommandError::usage(
                        "error: --fixed-value only applies with --value=<pattern>",
                    ));
                }
                if parsed.append && parsed.value_pattern.is_some() {
                    return Err(CommandError::usage(
                        "error: --append cannot be used with --value=<pattern>",
                    ));
                }
                // C `--append` sets the pattern to
                // `CONFIG_REGEX_NONE`, which never matches: the
                // new pair is always appended.
                let pattern = if parsed.append {
                    Some(WritePattern::None_)
                } else {
                    parsed.value_pattern.as_deref().map(|p| WritePattern::Pat(p.to_string()))
                };
                let multi = parsed.all || pattern.is_some();
                let comment = prepare_comment_opt(parsed.comment.as_deref())?;
                self.run_set(
                    ctx,
                    repo,
                    &rest,
                    &loc,
                    &disp,
                    pattern,
                    multi,
                    parsed.fixed,
                    comment.as_deref(),
                    false,
                    out,
                )
            }
            "unset" => {
                check_argc(rest.len(), 1, 1)?;
                if parsed.fixed && parsed.value_pattern.is_none() {
                    return Err(CommandError::usage(
                        "error: --fixed-value only applies with 'value-pattern'",
                    ));
                }
                let pattern = parsed.value_pattern.as_deref().map(|p| WritePattern::Pat(p.to_string()));
                let multi = parsed.all || pattern.is_some();
                self.run_unset(ctx, repo, &rest, &loc, pattern, multi, parsed.fixed, out)
            }
            "rename-section" => {
                check_argc(rest.len(), 2, 2)?;
                self.run_rename_section(ctx, repo, &rest, &loc, false, out)
            }
            "remove-section" => {
                check_argc(rest.len(), 1, 1)?;
                self.run_remove_section(ctx, repo, &rest, &loc, out)
            }
            "edit" => {
                check_argc(rest.len(), 0, 0)?;
                self.run_edit(ctx, repo, &loc, out)
            }
            _ => Err(CommandError::usage("error: no action specified")),
        }
    }

    /// Legacy flag mode (C `cmd_config_actions`).
    fn run_legacy(
        &self,
        ctx: &RepoContext,
        args: &[String],
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        let repo_opt = ctx.repository_opt()?;
        let repo = repo_opt.as_ref();
        let (mut loc, disp, rest, parsed) = parse_config_opts(args, LEGACY_USAGE, OptMode::Legacy)?;
        if let Some(f) = &loc.file {
            loc.file = Some(prefix_config_file(ctx, repo, f));
        }

        // C `cmd_config_actions` validations (exit 129).
        if (parsed.actions & (A_GET_COLOR | A_GET_COLORBOOL)) != 0 && disp.cli_type.is_some() {
            return Err(CommandError::usage(
                "error: --get-color and variable type are incoherent",
            ));
        }

        let actions_implicit = parsed.actions == 0;
        let actions = if actions_implicit {
            match rest.len() {
                1 => A_GET,
                2 => A_SET,
                3 => A_SET_ALL,
                _ => return Err(CommandError::usage("error: no action specified")),
            }
        } else {
            parsed.actions
        };

        if actions_implicit && rest.len() == 1 {
            // The implicit `name=value` spelling.
            if let Some(dot) = rest[0].rfind('.') {
                if rest[0][dot + 1..].contains('=') {
                    return Err(die_missing_set_value(&rest[0]));
                }
            }
        }
        if disp.omit_values && actions != A_LIST && actions != A_GET_REGEXP {
            return Err(CommandError::usage(
                "error: --name-only is only applicable to --list or --get-regexp",
            ));
        }
        if disp.show_origin
            && actions != A_GET
            && actions != A_GET_ALL
            && actions != A_GET_REGEXP
            && actions != A_LIST
        {
            return Err(CommandError::usage(
                "error: --show-origin is only applicable to --get, --get-all, --get-regexp, and --list",
            ));
        }
        if disp.default_value.is_some() && actions != A_GET {
            return Err(CommandError::usage("error: --default is only applicable to --get"));
        }
        if parsed.comment.is_some()
            && actions != A_ADD
            && actions != A_SET
            && actions != A_SET_ALL
            && actions != A_REPLACE_ALL
        {
            return Err(CommandError::usage(
                "error: --comment is only applicable to add/set/replace operations",
            ));
        }
        // C checks `--fixed-value` against the positional
        // argument counts (the value pattern is positional in
        // the legacy spellings).
        if parsed.fixed {
            let allowed = match actions {
                A_GET | A_GET_ALL | A_GET_REGEXP | A_UNSET | A_UNSET_ALL => rest.len() > 1,
                A_SET_ALL | A_REPLACE_ALL => rest.len() > 2,
                _ => false,
            };
            if !allowed {
                return Err(CommandError::usage(
                    "error: --fixed-value only applies with 'value-pattern'",
                ));
            }
        }

        let comment = prepare_comment_opt(parsed.comment.as_deref())?;
        let fixed = parsed.fixed;

        match actions {
            A_LIST => {
                check_argc(rest.len(), 0, 0)?;
                // C `display_options_init_list`: keys are
                // always shown.
                let mut disp = disp.clone_disp();
                disp.show_keys = true;
                self.run_list(ctx, repo, &loc, &disp, out)
            }
            A_EDIT => self.run_edit(ctx, repo, &loc, out),
            A_SET => {
                check_write(&loc, repo)?;
                check_argc(rest.len(), 2, 2)?;
                self.run_set(
                    ctx,
                    repo,
                    &rest,
                    &loc,
                    &disp,
                    None,
                    false,
                    fixed,
                    comment.as_deref(),
                    true,
                    out,
                )
            }
            A_SET_ALL => {
                check_write(&loc, repo)?;
                check_argc(rest.len(), 2, 3)?;
                let pattern = rest.get(2).map(|p| WritePattern::Pat(p.clone()));
                self.run_set_all(
                    ctx,
                    repo,
                    &rest,
                    &loc,
                    &disp,
                    pattern,
                    false,
                    fixed,
                    comment.as_deref(),
                    out,
                )
            }
            A_ADD => {
                check_write(&loc, repo)?;
                check_argc(rest.len(), 2, 2)?;
                self.run_add(ctx, repo, &rest, &loc, &disp, comment.as_deref(), out)
            }
            A_REPLACE_ALL => {
                check_write(&loc, repo)?;
                check_argc(rest.len(), 2, 3)?;
                let pattern = rest.get(2).map(|p| WritePattern::Pat(p.clone()));
                self.run_replace_all(
                    ctx,
                    repo,
                    &rest,
                    &loc,
                    &disp,
                    pattern,
                    fixed,
                    comment.as_deref(),
                    out,
                )
            }
            A_GET => {
                check_argc(rest.len(), 1, 2)?;
                let pattern = rest.get(1).map(|s| s.as_str());
                self.run_get(ctx, repo, &rest, &loc, &disp, pattern, false, false, fixed, out)
            }
            A_GET_ALL => {
                check_argc(rest.len(), 1, 2)?;
                let pattern = rest.get(1).map(|s| s.as_str());
                self.run_get(ctx, repo, &rest, &loc, &disp, pattern, true, false, fixed, out)
            }
            A_GET_REGEXP => {
                // C forces `--show-names` for the legacy
                // `--get-regexp` spelling.
                let mut disp = disp.clone_disp();
                disp.show_keys = true;
                check_argc(rest.len(), 1, 2)?;
                let pattern = rest.get(1).map(|s| s.as_str());
                self.run_get(ctx, repo, &rest, &loc, &disp, pattern, true, true, fixed, out)
            }
            A_GET_URLMATCH => {
                check_argc(rest.len(), 2, 2)?;
                let url = rest[1].clone();
                self.run_get_urlmatch(ctx, repo, &rest, &loc, &disp, &url, out)
            }
            A_UNSET => {
                check_write(&loc, repo)?;
                check_argc(rest.len(), 1, 2)?;
                // C: a positional pattern routes to the
                // multivar path (without MULTI_REPLACE);
                // without one the single-set API removes the
                // key outright.
                let pattern = rest.get(1).map(|p| WritePattern::Pat(p.clone()));
                let multi = pattern.is_some();
                self.run_unset(ctx, repo, &rest, &loc, pattern, multi, fixed, out)
            }
            A_UNSET_ALL => {
                check_write(&loc, repo)?;
                check_argc(rest.len(), 1, 2)?;
                let pattern = rest.get(1).map(|p| WritePattern::Pat(p.clone()));
                self.run_unset(ctx, repo, &rest, &loc, pattern, true, fixed, out)
            }
            A_RENAME_SECTION => {
                check_write(&loc, repo)?;
                check_argc(rest.len(), 2, 2)?;
                self.run_rename_section(ctx, repo, &rest, &loc, false, out)
            }
            A_REMOVE_SECTION => {
                check_write(&loc, repo)?;
                check_argc(rest.len(), 1, 1)?;
                self.run_remove_section(ctx, repo, &rest, &loc, out)
            }
            A_GET_COLOR => {
                check_argc(rest.len(), 1, 2)?;
                self.run_get_color(ctx, repo, &rest, &loc, &disp, out)
            }
            A_GET_COLORBOOL => {
                check_argc(rest.len(), 1, 2)?;
                self.run_get_colorbool(ctx, repo, &rest, &loc, &disp, out)
            }
            _ => Err(CommandError::usage("error: no action specified")),
        }
    }
}
impl Config {
    /// `git config list` (C `ACTION_LIST` over `show_all_config`).
    fn run_list(
        &self,
        ctx: &RepoContext,
        repo: Option<&git_core::Repository>,
        loc: &LocOpts,
        disp: &DispOpts,
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        // C `display_options_init_list`: list always shows keys,
        // and `-z` joins key and value with '\n' (terminated by
        // NUL), not a NUL separator.
        let mut disp = disp.clone_disp();
        disp.show_keys = true;
        let src = load_read_source(ctx, repo, loc)?;
        if let Some(file) = src.missing_file {
            // C: `die_errno("unable to read config file '%s'")`.
            return Err(CommandError::fatal(format!(
                "fatal: unable to read config file '{file}': No such file or directory"
            )));
        }
        let key_delim = if disp.end_nul { '\n' } else { '=' };
        for e in &src.entries {
            let name = dotted_name(&e.entry);
            let formatted = match format_typed(disp.cli_type, &name, eff_value(e), &TypeLoc::of(e)) {
                Ok(v) => v,
                Err(te) => return Err(emit_type_error(te)),
            };
            if let Some(line) = render_entry(&disp, e, formatted, key_delim) {
                write_line(out, &line)?;
            }
        }
        Ok(())
    }

    /// `git config get` family (C `get_value`).
    fn run_get(
        &self,
        ctx: &RepoContext,
        repo: Option<&git_core::Repository>,
        args: &[String],
        loc: &LocOpts,
        disp: &DispOpts,
        value_pattern: Option<&str>,
        all: bool,
        regexp: bool,
        fixed: bool,
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        let Some(key) = args.first() else {
            return Err(CommandError::usage("error: wrong number of arguments, should be 1"));
        };
        let src = load_read_source(ctx, repo, loc)?;
        if src.missing_file.is_some() {
            return Err(CommandError::silent(1));
        }
        let req = GetReq {
            key: key.clone(),
            regexp,
            all,
            value_pattern: value_pattern.map(|s| s.to_string()),
            fixed_value: fixed,
        };
        let key_delim = if disp.end_nul { '\0' } else { ' ' };
        run_get(&src.entries, disp, &req, key_delim, out)
    }

    /// `git config get-urlmatch` (C `get_urlmatch`).
    fn run_get_urlmatch(
        &self,
        ctx: &RepoContext,
        repo: Option<&git_core::Repository>,
        args: &[String],
        loc: &LocOpts,
        disp: &DispOpts,
        url: &str,
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        let Some(var) = args.first() else {
            return Err(CommandError::usage("error: wrong number of arguments, should be 2"));
        };
        let target = url_normalize(url, false).map_err(|e| CommandError::fatal(format!("fatal: {e}")))?;
        // C lowercases the whole var, then splits at the first dot.
        let lowered = var.to_lowercase();
        let (section, key) = match lowered.split_once('.') {
            Some((sec, rest)) => (sec.to_string(), Some(rest.to_string())),
            None => (lowered, None),
        };
        // C forces show_keys when the key is absent (the caller
        // asks for whole sections).
        let show_keys = key.is_none();
        let mut disp = disp.clone_disp();
        disp.show_keys = show_keys;

        let src = load_read_source(ctx, repo, loc)?;
        if src.missing_file.is_some() {
            return Err(CommandError::silent(1));
        }
        // C `urlmatch_config_entry`: var must start `section.`, the
        // part before the last dot is a URL pattern (globs allowed),
        // and the trailing key must match. Best match per key wins
        // (C `string_list` keyed by the trailing key).
        let mut matched: Vec<(String, LoadedEntry, (usize, usize, bool))> = Vec::new();
        for e in &src.entries {
            let dotted = dotted_name(&e.entry);
            let Some(rest) = dotted.strip_prefix(&section) else {
                continue;
            };
            let Some(rest) = rest.strip_prefix('.') else {
                continue;
            };
            let (config_url, entry_key) = match rest.rsplit_once('.') {
                Some((u, k)) => (u, k),
                None => (rest, rest),
            };
            let config_url = config_url.to_string();
            let entry_key = entry_key.to_string();
            if let Some(key) = &key {
                if *key != entry_key {
                    continue;
                }
            }
            let Ok(norm) = url_normalize(&config_url, true) else {
                continue;
            };
            let Some(m) = match_urls(&target, &norm) else {
                continue;
            };
            // C `cmp_matches`: longer host, then longer path, then
            // user-matched wins. C replaces on ties (`select_fn >= 0`),
            // so a later (higher-priority) entry wins an equal rank.
            let rank = (m.hostmatch_len, m.pathmatch_len, m.user_matched);
            if let Some(pos) = matched.iter().position(|(k, _, _)| *k == entry_key) {
                if rank >= matched[pos].2 {
                    matched[pos] = (entry_key, e.clone(), rank);
                }
            } else {
                matched.push((entry_key, e.clone(), rank));
            }
        }
        matched.sort_by(|a, b| a.0.cmp(&b.0));
        let key_delim = if disp.end_nul { '\0' } else { ' ' };
        for (_, e, _) in &matched {
            let name = dotted_name(&e.entry);
            let formatted = match format_typed(disp.cli_type, &name, eff_value(e), &TypeLoc::of(e)) {
                Ok(v) => v,
                Err(te) => return Err(emit_type_error(te)),
            };
            if let Some(line) = render_entry(&disp, e, formatted, key_delim) {
                write_line(out, &line)?;
            }
        }
        if matched.is_empty() {
            return Err(CommandError::silent(1));
        }
        Ok(())
    }

    /// `git config set` (C `ACTION_SET`).
    fn run_set(
        &self,
        ctx: &RepoContext,
        repo: Option<&git_core::Repository>,
        args: &[String],
        loc: &LocOpts,
        disp: &DispOpts,
        pattern: Option<WritePattern>,
        multi: bool,
        fixed: bool,
        comment: Option<&str>,
        legacy: bool,
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        let _ = out;
        let (name, value) = (args[0].clone(), args[1].clone());
        let (section, subsection, key) = parse_write_key(&name)?;
        let value = normalize_value(&name, &value, disp.cli_type)?;
        let comment = comment.map(prepare_comment).transpose()?;
        let path = write_target_file(ctx, repo, loc)?;
        let dotted = match &subsection {
            Some(sub) => format!("{section}.{sub}.{key}"),
            None => format!("{section}.{key}"),
        };
        match pattern {
            // C `repo_config_set_in_file_gently`: append when
            // absent, replace when unique, refuse on several
            // matches (warning first, then the per-mode hint).
            Some(WritePattern::None_) | None => {
                let display = write_display_name(ctx, repo, loc);
                let text = read_file_text(&path, &display)?;
                let new_text = match cfgfile::set_value(
                    &text,
                    &section,
                    subsection.as_deref(),
                    &key,
                    &value,
                    comment.as_deref(),
                ) {
                    Ok(t) => t,
                    Err(_) => {
                        eprintln!("warning: {dotted} has multiple values");
                        let hint = if legacy {
                            "Use a regexp, --add or --replace-all to change"
                        } else {
                            "Use --value=<pattern>, --append or --all to change"
                        };
                        return Err(CommandError {
                            message: format!(
                                "error: cannot overwrite multiple values with a single value\n       {hint} {name}."
                            ),
                            code: 5,
                        });
                    }
                };
                cfgfile::write_config_file(&path, &new_text).map_err(|_| {
                    CommandError::error(format!(
                        "error: could not write config file {}",
                        path.display()
                    ))
                })
            }
            Some(pat) => config_multivar_write(
                &path,
                &section,
                subsection.as_deref(),
                &key,
                Some(&value),
                &pat,
                fixed,
                multi,
                comment.as_deref(),
            ),
        }
    }

    /// `git config set-all` (C `ACTION_SET_ALL`).
    fn run_set_all(
        &self,
        ctx: &RepoContext,
        repo: Option<&git_core::Repository>,
        args: &[String],
        loc: &LocOpts,
        disp: &DispOpts,
        pattern: Option<WritePattern>,
        multi: bool,
        fixed: bool,
        comment: Option<&str>,
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        let _ = out;
        let (name, value) = (args[0].clone(), args[1].clone());
        let (section, subsection, key) = parse_write_key(&name)?;
        let value = normalize_value(&name, &value, disp.cli_type)?;
        let comment = comment.map(prepare_comment).transpose()?;
        let path = write_target_file(ctx, repo, loc)?;
        let pattern = pattern.unwrap_or(WritePattern::All);
        config_multivar_write(
            &path,
            &section,
            subsection.as_deref(),
            &key,
            Some(&value),
            &pattern,
            fixed,
            multi,
            comment.as_deref(),
        )
    }

    /// `git config replace-all` (C `ACTION_REPLACE_ALL`).
    fn run_replace_all(
        &self,
        ctx: &RepoContext,
        repo: Option<&git_core::Repository>,
        args: &[String],
        loc: &LocOpts,
        disp: &DispOpts,
        pattern: Option<WritePattern>,
        fixed: bool,
        comment: Option<&str>,
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        let _ = out;
        let (name, value) = (args[0].clone(), args[1].clone());
        let (section, subsection, key) = parse_write_key(&name)?;
        let value = normalize_value(&name, &value, disp.cli_type)?;
        let comment = comment.map(prepare_comment).transpose()?;
        let path = write_target_file(ctx, repo, loc)?;
        let pattern = pattern.unwrap_or(WritePattern::All);
        config_multivar_write(
            &path,
            &section,
            subsection.as_deref(),
            &key,
            Some(&value),
            &pattern,
            fixed,
            true,
            comment.as_deref(),
        )
    }

    /// `git config add` (C `ACTION_ADD`: `CONFIG_REGEX_NONE`).
    fn run_add(
        &self,
        ctx: &RepoContext,
        repo: Option<&git_core::Repository>,
        args: &[String],
        loc: &LocOpts,
        disp: &DispOpts,
        comment: Option<&str>,
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        let _ = out;
        let (name, value) = (args[0].clone(), args[1].clone());
        let (section, subsection, key) = parse_write_key(&name)?;
        let value = normalize_value(&name, &value, disp.cli_type)?;
        let comment = comment.map(prepare_comment).transpose()?;
        let path = write_target_file(ctx, repo, loc)?;
        config_write_add(&path, &section, subsection.as_deref(), &key, Some(&value), comment.as_deref())
    }

    /// `git config unset` / `unset-all` (C `ACTION_UNSET[_ALL]`).
    fn run_unset(
        &self,
        ctx: &RepoContext,
        repo: Option<&git_core::Repository>,
        args: &[String],
        loc: &LocOpts,
        pattern: Option<WritePattern>,
        multi: bool,
        fixed: bool,
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        let _ = out;
        let name = args[0].clone();
        let (section, subsection, key) = parse_write_key(&name)?;
        let path = write_target_file(ctx, repo, loc)?;
        let pattern = pattern.unwrap_or(WritePattern::All);
        config_multivar_write(
            &path,
            &section,
            subsection.as_deref(),
            &key,
            None,
            &pattern,
            fixed,
            multi,
            None,
        )
    }

    /// `git config rename-section` (C `ACTION_RENAME_SECTION`).
    fn run_rename_section(
        &self,
        ctx: &RepoContext,
        repo: Option<&git_core::Repository>,
        args: &[String],
        loc: &LocOpts,
        remove: bool,
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        let _ = out;
        let old = args[0].clone();
        let new = if remove { None } else { Some(args[1].clone()) };
        if let Some(n) = &new {
            if !section_name_is_ok(n) {
                eprintln!("error: invalid section name: {n}");
                return Err(CommandError::silent(255));
            }
        }
        let path = write_target_file(ctx, repo, loc)?;
        let display = write_display_name(ctx, repo, loc);
        let text = read_file_text(&path, &display)?;
        let (new_text, count) = if remove {
            cfgfile::remove_section(&text, &old)
        } else {
            cfgfile::rename_section(&text, &old, new.as_deref().unwrap_or(""))
        };
        if count == 0 {
            return Err(CommandError::fatal(format!("fatal: no such section: {old}")));
        }
        cfgfile::write_config_file(&path, &new_text).map_err(|_| {
            CommandError::error(format!("error: could not write config file {}", path.display()))
        })
    }

    /// `git config remove-section` (C `ACTION_REMOVE_SECTION`).
    fn run_remove_section(
        &self,
        ctx: &RepoContext,
        repo: Option<&git_core::Repository>,
        args: &[String],
        loc: &LocOpts,
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        self.run_rename_section(ctx, repo, args, loc, true, out)
    }

    /// `git config edit` (C `ACTION_EDIT` over `show_editor`).
    fn run_edit(
        &self,
        ctx: &RepoContext,
        repo: Option<&git_core::Repository>,
        loc: &LocOpts,
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        let _ = out;
        if loc.file.as_deref() == Some("-") {
            return Err(CommandError::fatal("fatal: editing stdin is not supported"));
        }
        if loc.blob.is_some() {
            return Err(CommandError::fatal("fatal: editing blobs is not supported"));
        }
        if repo.is_none() && loc.file.is_none() && !loc.global && !loc.system {
            return Err(CommandError::fatal("fatal: not in a git directory"));
        }
        let path = write_target_file(ctx, repo, loc)?;
        // C `show_editor`: `--global` creates the file with the
        // default template when missing.
        if loc.global && !path.exists() {
            let template = default_user_config();
            cfgfile::write_config_file(&path, &template).map_err(|_| {
                CommandError::fatal(format!(
                    "fatal: cannot create configuration file {}",
                    path.display()
                ))
            })?;
        }
        launch_editor_on(ctx, repo, &path)
    }

    /// `git config get-color` (C `get_color`).
    fn run_get_color(
        &self,
        ctx: &RepoContext,
        repo: Option<&git_core::Repository>,
        args: &[String],
        loc: &LocOpts,
        _disp: &DispOpts,
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        let slot = args.first().cloned().unwrap_or_default();
        let def_color = args.get(1).cloned();
        let src = load_read_source(ctx, repo, loc)?;
        let mut found = false;
        let mut parsed = String::new();
        for e in &src.entries {
            if dotted_name(&e.entry) != slot {
                continue;
            }
            let Some(v) = eff_value(e) else {
                // C `config_error_nonbool`: error, then the (empty)
                // color is still printed with exit 0.
                eprintln!("error: missing value for '{slot}'");
                break;
            };
            if let Some(color) = color_parse_value(v) {
                parsed = color;
                found = true;
            } else {
                eprintln!("error: invalid color value: {v}");
            }
            break;
        }
        if !found {
            if let Some(def) = def_color {
                if color_parse_value(&def).is_none() {
                    eprintln!("error: unable to parse default color value");
                    return Err(CommandError::silent(1));
                }
                if let Some(color) = color_parse_value(&def) {
                    parsed = color;
                }
            }
        }
        write_line(out, &parsed)
    }

    /// `git config get-colorbool` (C `get_colorbool`).
    fn run_get_colorbool(
        &self,
        ctx: &RepoContext,
        repo: Option<&git_core::Repository>,
        args: &[String],
        loc: &LocOpts,
        _disp: &DispOpts,
        out: &mut dyn Write,
    ) -> Result<(), CommandError> {
        let slot = args.first().cloned().unwrap_or_default();
        // Optional `<stdout-is-tty>` argument.
        let print = args.len() > 1;
        let mut tty = std::io::IsTerminal::is_terminal(&std::io::stdout());
        if print {
            tty = parse_bool(&args[1]).unwrap_or(false);
        }
        let src = load_read_source(ctx, repo, loc)?;
        // C `git_get_colorbool_config`: slot, then `diff.color`,
        // then `color.ui` fallbacks; `git_config_colorbool` mapping.
        let mut slot_found: Option<u8> = None; // NEVER=0, AUTO=1, ALWAYS=2
        let mut diff_found: Option<u8> = None;
        let mut ui_found: Option<u8> = None;
        for e in &src.entries {
            let name = dotted_name(&e.entry);
            let cb = colorbool_value(eff_value(e));
            if name == slot {
                if slot_found.is_none() {
                    slot_found = Some(cb);
                }
            } else if name == "diff.color" {
                if diff_found.is_none() {
                    diff_found = Some(cb);
                }
            } else if name == "color.ui" {
                if ui_found.is_none() {
                    ui_found = Some(cb);
                }
            }
        }
        let mut found = slot_found;
        if found.is_none() && slot == "color.diff" {
            found = diff_found;
        }
        if found.is_none() {
            found = ui_found;
        }
        // C: unknown defaults to AUTO; want_color maps AUTO to the
        // tty state, ALWAYS/NEVER to themselves.
        let result = match found {
            Some(2) => true,
            Some(0) => false,
            _ => tty,
        };
        if print {
            write_line(out, if result { "true" } else { "false" })?;
            Ok(())
        } else if result {
            Ok(())
        } else {
            Err(CommandError::silent(1))
        }
    }
}

/// C `git_config_colorbool`: `never`/`always`/`auto` names, else
/// boolean (`false` → NEVER, truthy → AUTO).
fn colorbool_value(value: Option<&str>) -> u8 {
    let Some(v) = value else {
        return 1; // bare key: true → AUTO
    };
    if v.eq_ignore_ascii_case("never") {
        return 0;
    }
    if v.eq_ignore_ascii_case("always") {
        return 2;
    }
    if v.eq_ignore_ascii_case("auto") {
        return 1;
    }
    if parse_bool(v).unwrap_or(false) {
        1
    } else {
        0
    }
}

/// Clone the display options (C copies the struct for urlmatch).
impl DispOpts {
    fn clone_disp(&self) -> DispOpts {
        DispOpts {
            end_nul: self.end_nul,
            omit_values: self.omit_values,
            show_origin: self.show_origin,
            show_scope: self.show_scope,
            show_keys: self.show_keys,
            cli_type: self.cli_type,
            default_value: self.default_value.clone(),
        }
    }
}

// gsd:config-part-8
