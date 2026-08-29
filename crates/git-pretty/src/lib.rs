//! A git-compatible subset of `pretty.c`: the commit formatting language
//! used by `log`/`show`/`format-patch`.
//!
//! Implemented: builtin formats (`oneline`, `short`, `medium`, `full`,
//! `fuller`, `raw`, `reference`, `format:`, `tformat:`), the core
//! placeholder set, `%x##`/`%%`/`%n`, `+`/`-`/space toggles, `--date=`
//! modes over `git-date`, and color directives (`%C(...)`) emitted only
//! when color is enabled.
//!
//! Not yet implemented (tracked in `docs/plan/phase-a/PROGRESS.md`):
//! `%w()` wrapping, `<()`/`>()` alignment, `%(trailers...)`, mailmap.

use std::fmt::Write as _;

pub mod date;

use date::{show_date, DateMode};
use git_hash::Oid;
use git_date::Timestamp;

/// A parsed `Name <email> secs tz` ident line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub email: String,
    pub ts: Timestamp,
}

impl Ident {
    /// Parse a raw `author`/`committer` header value.
    pub fn parse(raw: &str) -> Option<Ident> {
        let lt = raw.find('<')?;
        let gt = raw[lt..].find('>')? + lt;
        let name = raw[..lt].trim_end();
        let email = &raw[lt + 1..gt];
        let tokens: Vec<&str> = raw[gt + 1..].split_whitespace().collect();
        let secs = tokens.first().and_then(|t| t.parse::<i64>().ok()).unwrap_or(0);
        let offset = tokens
            .get(1)
            .and_then(|tz| {
                let sign_n = if tz.starts_with('-') { -1i32 } else { 1i32 };
                let digits = tz.trim_start_matches(['+', '-']);
                if digits.len() >= 4 && digits.bytes().all(|b| b.is_ascii_digit()) {
                    let v: i32 = digits.parse().ok()?;
                    Some(sign_n * (v / 100 * 60 + v % 100))
                } else {
                    None
                }
            })
            .unwrap_or(0);
        Some(Ident {
            name: name.to_string(),
            email: email.to_string(),
            ts: Timestamp::new(secs, offset),
        })
    }
}

/// Everything the formatter needs to know about one commit.
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub oid: Oid,
    pub tree: Oid,
    pub parents: Vec<Oid>,
    pub author: Ident,
    pub committer: Ident,
    pub message: Vec<u8>,
    /// The commit encoding header value, if present.
    pub encoding: Option<String>,
}

impl CommitInfo {
    /// Parse a commit object's raw bytes.
    pub fn parse(oid: Oid, data: &[u8], algo: git_hash::HashAlgorithm) -> Option<CommitInfo> {
        let text = String::from_utf8_lossy(data).into_owned();
        let mut tree = None;
        let mut parents = Vec::new();
        let mut author = None;
        let mut committer = None;
        let mut encoding = None;
        let mut rest = &text[..];
        while let Some(line_end) = rest.find('\n') {
            let line = &rest[..line_end];
            if line.is_empty() {
                rest = &rest[line_end + 1..];
                break;
            }
            if let Some(v) = line.strip_prefix("tree ") {
                tree = Oid::from_hex(v, algo).ok();
            } else if let Some(v) = line.strip_prefix("parent ") {
                if let Ok(p) = Oid::from_hex(v, algo) {
                    parents.push(p);
                }
            } else if let Some(v) = line.strip_prefix("author ") {
                author = Ident::parse(v);
            } else if let Some(v) = line.strip_prefix("committer ") {
                committer = Ident::parse(v);
            } else if let Some(v) = line.strip_prefix("encoding ") {
                encoding = Some(v.to_string());
            }
            rest = &rest[line_end + 1..];
        }
        // The remainder is the message (C git keeps it verbatim).
        let message_start = text.len() - rest.len();
        Some(CommitInfo {
            oid,
            tree: tree?,
            parents,
            author: author?,
            committer: committer?,
            message: data[message_start.min(data.len())..].to_vec(),
            encoding,
        })
    }
}

/// The `--pretty=` format selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Format {
    Oneline,
    Short,
    Medium,
    Full,
    Fuller,
    Raw,
    Reference,
    /// `format:<string>` (no trailing newline terminator).
    User(String),
    /// `tformat:<string>` (terminator semantics).
    UserTerminated(String),
}

impl Format {
    /// Parse a `--pretty=` / `--format=` argument (C git's
    /// `get_commit_format`).
    pub fn parse(spec: &str) -> Option<Format> {
        if let Some(f) = spec.strip_prefix("format:") {
            return Some(Format::User(f.to_string()));
        }
        if let Some(f) = spec.strip_prefix("tformat:") {
            return Some(Format::UserTerminated(f.to_string()));
        }
        Some(match spec {
            "" => Format::Medium,
            "oneline" => Format::Oneline,
            "short" => Format::Short,
            "medium" => Format::Medium,
            "full" => Format::Full,
            "fuller" => Format::Fuller,
            "raw" => Format::Raw,
            "reference" => Format::Reference,
            _ => return None,
        })
    }

    /// True for formats that separate commits without blank lines.
    pub fn is_oneline(&self) -> bool {
        matches!(self, Format::Oneline | Format::Reference)
            || matches!(self, Format::UserTerminated(_))
    }
}

/// Rendering options.
#[derive(Debug, Clone)]
pub struct Options {
    /// The `--date=` mode for `%ad`/`%cd`.
    pub date: DateMode,
    /// Abbreviation length for `%h`/`%t`/`%p` (with C git's default of 7).
    pub abbrev: usize,
    /// Color enabled (plumbing defaults to off; `%C(...)` emits only then).
    pub color: bool,
    /// Current epoch for relative/human dates.
    pub now: i64,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            date: DateMode::Default,
            abbrev: 7,
            color: false,
            now: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        }
    }
}

/// Errors while formatting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrettyError {
    UnknownPlaceholder(String),
}

impl std::fmt::Display for PrettyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrettyError::UnknownPlaceholder(p) => {
                write!(f, "fatal: unknown format specifier: {p}")
            }
        }
    }
}

impl std::error::Error for PrettyError {}

/// Render one commit in `fmt`, writing to `out`.
pub fn format_commit(
    fmt: &Format,
    info: &CommitInfo,
    opts: &Options,
    out: &mut dyn std::io::Write,
) -> Result<(), PrettyError> {
    let mut buf = String::new();
    match fmt {
        Format::Oneline => {
            expand("%H %s", info, opts, &mut buf)?;
            buf.push('\n');
        }
        Format::User(f) | Format::UserTerminated(f) => {
            expand(f, info, opts, &mut buf)?;
            if matches!(fmt, Format::UserTerminated(_)) {
                buf.push('\n');
            }
        }
        Format::Reference => {
            let mut o = opts.clone();
            o.date = DateMode::Short;
            expand("%h (%s, %ad)", info, &o, &mut buf)?;
            buf.push('\n');
        }
        Format::Medium => {
            buf.push_str(&format!("commit {}\n", info.oid));
            if info.parents.len() > 1 {
                buf.push_str(&format!(
                    "Merge: {}\n",
                    info.parents
                        .iter()
                        .map(|p| short(p, opts.abbrev))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
            buf.push_str(&format!(
                "Author: {} <{}>\n",
                info.author.name, info.author.email
            ));
            buf.push_str(&format!("Date:   {}\n\n", show_date(info.author.ts, &opts.date, opts.now)));
            push_indented_body(&mut buf, info);
        }
        Format::Short => {
            buf.push_str(&format!("commit {}\n", info.oid));
            buf.push_str(&format!(
                "Author: {} <{}>\n\n",
                info.author.name, info.author.email
            ));
            let subj = subject(info);
            let first = subj.split('\n').next().unwrap_or("");
            buf.push_str("    ");
            buf.push_str(first);
            buf.push('\n');
        }
        Format::Full => {
            buf.push_str(&format!("commit {}\n", info.oid));
            buf.push_str(&format!(
                "Author: {} <{}>\n",
                info.author.name, info.author.email
            ));
            buf.push_str(&format!(
                "Commit: {} <{}>\n\n",
                info.committer.name, info.committer.email
            ));
            push_indented_body(&mut buf, info);
        }
        Format::Fuller => {
            buf.push_str(&format!("commit {}\n", info.oid));
            buf.push_str(&format!(
                "Author:     {} <{}>\n",
                info.author.name, info.author.email
            ));
            buf.push_str(&format!(
                "AuthorDate: {}\n",
                show_date(info.author.ts, &opts.date, opts.now)
            ));
            buf.push_str(&format!(
                "Commit:     {} <{}>\n",
                info.committer.name, info.committer.email
            ));
            buf.push_str(&format!(
                "CommitDate: {}\n\n",
                show_date(info.committer.ts, &opts.date, opts.now)
            ));
            push_indented_body(&mut buf, info);
        }
        Format::Raw => {
            buf.push_str(&format!("commit {}\n", info.oid));
            buf.push_str(&format!("tree {}\n", info.tree));
            for p in &info.parents {
                buf.push_str(&format!("parent {p}\n"));
            }
            buf.push_str(&format!(
                "author {} <{}> {}\n",
                info.author.name,
                info.author.email,
                show_date(info.author.ts, &DateMode::Raw, opts.now)
            ));
            buf.push_str(&format!(
                "committer {} <{}> {}\n\n",
                info.committer.name,
                info.committer.email,
                show_date(info.committer.ts, &DateMode::Raw, opts.now)
            ));
            push_indented_body(&mut buf, info);
        }
    }
    out.write_all(buf.as_bytes())
        .map_err(|e| PrettyError::UnknownPlaceholder(format!("io: {e}")))?;
    Ok(())
}

/// Append the message body indented by four spaces (C git's `pp_remainder`).
fn push_indented_body(buf: &mut String, info: &CommitInfo) {
    push_indented_text(buf, &String::from_utf8_lossy(&info.message));
}

/// Indent every line by four spaces (including blank ones), dropping
/// trailing blank lines, like C git's `pp_remainder`.
fn push_indented_text(buf: &mut String, msg: &str) {
    let mut lines: Vec<&str> = msg.split('\n').collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    for line in lines {
        buf.push_str("    ");
        buf.push_str(line);
        buf.push('\n');
    }
}

fn short(oid: &Oid, len: usize) -> String {
    let hex = format!("{oid}");
    hex[..len.min(hex.len())].to_string()
}

/// The subject: the message's first paragraph joined with spaces (C git's
/// `format_subject` with a single-space separator).
pub fn subject(info: &CommitInfo) -> String {
    let msg = String::from_utf8_lossy(&info.message);
    let mut parts: Vec<String> = Vec::new();
    for line in msg.split('\n') {
        if line.trim().is_empty() {
            break;
        }
        parts.push(line.trim_end().to_string());
    }
    parts.join(" ")
}

/// The body: everything after the first blank line (C git's `body_off`).
pub fn body(info: &CommitInfo) -> String {
    let msg = String::from_utf8_lossy(&info.message);
    let mut iter = msg.split('\n').peekable();
    let mut consumed = String::new();
    for line in iter.by_ref() {
        if line.trim().is_empty() {
            break;
        }
        consumed.push_str(line);
        consumed.push('\n');
    }
    let _ = iter;
    // Re-slice from the original bytes for fidelity.
    let idx = info
        .message
        .windows(consumed.len().min(info.message.len()))
        .position(|w| w == consumed.as_bytes())
        .map(|i| i + consumed.len())
        .unwrap_or(info.message.len());
    let rest = &info.message[idx..];
    // C git skips blank lines after the subject (skip_blank_lines).
    let start = rest
        .split(|&b| b != b'\n')
        .next()
        .map(|lead| lead.len())
        .unwrap_or(0);
    String::from_utf8_lossy(&rest[start.min(rest.len())..]).into_owned()
}

/// C git's `format_sanitized_subject` (`%f`).
pub fn sanitized_subject(info: &CommitInfo) -> String {
    let subj = subject(info);
    let mut out = String::new();
    let mut space = 2;
    let bytes = subj.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let title = c.is_ascii_lowercase()
            || c.is_ascii_uppercase()
            || c.is_ascii_digit()
            || c == b'.'
            || c == b'_';
        if title {
            if space == 1 {
                out.push('-');
            }
            space = 0;
            out.push(c as char);
            if c == b'.' {
                while i + 1 < bytes.len() && bytes[i + 1] == b'.' {
                    i += 1;
                }
            }
        } else {
            space |= 1;
        }
        i += 1;
    }
    while out.ends_with('.') || out.ends_with('-') {
        out.pop();
    }
    out
}

/// Expand a user format string into `out`.
pub fn expand(
    fmt: &str,
    info: &CommitInfo,
    opts: &Options,
    out: &mut String,
) -> Result<(), PrettyError> {
    let bytes: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c != '%' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        let Some(&next) = bytes.get(i) else {
            out.push('%');
            break;
        };
        // Toggles: `+`, `-`, or a space preceding a placeholder.
        match next {
            '+' | '-' | ' ' => {
                let marker = out.len();
                i += 1;
                let Some(&ph) = bytes.get(i) else {
                    out.push('%');
                    out.push(next);
                    break;
                };
                let mut chunk = String::new();
                i = expand_one(ph, &bytes, i + 1, info, opts, &mut chunk)?;
                if !chunk.is_empty() {
                    if next == '+' {
                        out.push('\n');
                    } else if next == ' ' {
                        out.push(' ');
                    }
                    out.push_str(&chunk);
                } else if next == '-' {
                    while out.ends_with('\n') {
                        out.pop();
                    }
                } else if next == '+' {
                    let _ = marker;
                }
                continue;
            }
            _ => {}
        }
        i = expand_one(next, &bytes, i + 1, info, opts, out)?;
    }
    Ok(())
}

/// Expand one placeholder starting after its character; returns the new
/// position in `chars`.
fn expand_one(
    ph: char,
    chars: &[char],
    pos: usize,
    info: &CommitInfo,
    opts: &Options,
    out: &mut String,
) -> Result<usize, PrettyError> {
    macro_rules! emit {
        ($e:expr) => {{
            let _ = out.write_fmt(format_args!("{}", $e));
            return Ok(pos);
        }};
    }
    match ph {
        'H' => emit!(info.oid),
        'T' => emit!(info.tree),
        'h' => emit!(short(&info.oid, opts.abbrev)),
        't' => emit!(short(&info.tree, opts.abbrev)),
        'P' => emit!(
            info.parents
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        ),
        'p' => emit!(
            info.parents
                .iter()
                .map(|p| short(p, opts.abbrev))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        'a' | 'c' => {
            let ident = if ph == 'a' { &info.author } else { &info.committer };
            let Some(&next) = chars.get(pos) else {
                return Err(PrettyError::UnknownPlaceholder(format!("%{ph}")));
            };
            let rendered = match next {
                'n' => ident.name.clone(),
                'e' => ident.email.clone(),
                'N' | 'E' => ident.name.clone(), // mailmap not implemented
                'd' => show_date(ident.ts, &opts.date, opts.now),
                'D' => show_date(ident.ts, &DateMode::Rfc, opts.now),
                'r' => show_date(ident.ts, &DateMode::Relative, opts.now),
                't' => ident.ts.secs.to_string(),
                'i' => show_date(ident.ts, &DateMode::Iso, opts.now),
                'I' => show_date(ident.ts, &DateMode::IsoStrict, opts.now),
                other => return Err(PrettyError::UnknownPlaceholder(format!("%{ph}{other}"))),
            };
            out.push_str(&rendered);
            Ok(pos + 1)
        }
        's' => emit!(subject(info)),
        'f' => emit!(sanitized_subject(info)),
        'b' => emit!(body(info)),
        'B' => emit!(String::from_utf8_lossy(&info.message)),
        'e' => emit!(info.encoding.clone().unwrap_or_default()),
        'n' => emit!('\n'),
        '%' => emit!('%'),
        'x' => {
            let hex: String = chars.get(pos..pos + 2).unwrap_or(&[]).iter().collect();
            if hex.len() == 2 {
                if let Ok(b) = u8::from_str_radix(&hex, 16) {
                    out.push(b as char);
                    return Ok(pos + 2);
                }
            }
            Err(PrettyError::UnknownPlaceholder("%x".to_string()))
        }
        'C' => {
            // %C(...) or %Creset: color directives; emit only when color on.
            if chars.get(pos) == Some(&'r') {
                // Creset
                if opts.color {
                    out.push_str("\x1b[m");
                }
                return Ok(pos + "reset".len());
            }
            if chars.get(pos) == Some(&'(') {
                let end = chars[pos..].iter().position(|&c| c == ')');
                if let Some(end) = end {
                    let name: String = chars[pos + 1..pos + end].iter().collect();
                    if opts.color {
                        out.push_str(&color_code(&name));
                    }
                    return Ok(pos + end + 1);
                }
            }
            Err(PrettyError::UnknownPlaceholder("%C".to_string()))
        }
        'G' => {
            // %G? signatures: no GPG support → 'N' for ?, empty for others.
            match chars.get(pos) {
                Some('?') => {
                    out.push('N');
                    Ok(pos + 1)
                }
                Some(_) => Ok(pos + 1),
                None => Err(PrettyError::UnknownPlaceholder("%G".to_string())),
            }
        }
        'd' | 'D' | 'N' | 'S' | 'g' | 'm' | 'w' | '<' | '>' | '(' => {
            // Decorations, notes, mailmap fields, graph marks, wrapping and
            // alignment directives: emit nothing (or skip the arg text).
            if matches!(ph, 'w' | '<' | '>' | '(') {
                if chars.get(pos) == Some(&'(') {
                    if let Some(end) = chars[pos..].iter().position(|&c| c == ')') {
                        return Ok(pos + end + 1);
                    }
                }
            }
            if ph == 'g' {
                // %g* reflog fields: skip the trailing letter.
                return Ok(pos + 1);
            }
            Ok(pos)
        }
        other => {
            let _ = other;
            Err(PrettyError::UnknownPlaceholder(format!("%{ph}")))
        }
    }
}

fn color_code(name: &str) -> String {
    let code = match name.trim() {
        "reset" => "\x1b[m",
        "black" => "\x1b[30m",
        "red" => "\x1b[31m",
        "green" => "\x1b[32m",
        "yellow" => "\x1b[33m",
        "blue" => "\x1b[34m",
        "magenta" => "\x1b[35m",
        "cyan" => "\x1b[36m",
        "white" => "\x1b[37m",
        "bold red" => "\x1b[1;31m",
        "bold green" => "\x1b[1;32m",
        "bold blue" => "\x1b[1;34m",
        _ => "",
    };
    code.to_string()
}

#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    fn sample_info() -> CommitInfo {
        CommitInfo {
            oid: Oid::from_hex(
                "73f1fbcb3f33e3d3ec64196f7e1f40a161f443c0",
                git_hash::HashAlgorithm::Sha1,
            )
            .unwrap(),
            tree: Oid::from_hex(
                "2e81171448eb9f2ee3821e3d447aa6b2fe3ddba1",
                git_hash::HashAlgorithm::Sha1,
            )
            .unwrap(),
            parents: Vec::new(),
            author: Ident {
                name: "A U Thor".into(),
                email: "author@example.com".into(),
                ts: Timestamp::new(1_582_024_274, 210),
            },
            committer: Ident {
                name: "C O Mitter".into(),
                email: "committer@example.com".into(),
                ts: Timestamp::new(1_582_024_274, 210),
            },
            message: b"subject line\n\nBody text\n".to_vec(),
            encoding: None,
        }
    }

    proptest! {
        /// No panics on arbitrary format strings.
        #[test]
        fn no_panic_on_arbitrary_format(fmt in ".*") {
            let info = sample_info();
            let opts = Options::default();
            let mut out = Vec::new();
            let _ = crate::format_commit(&Format::User(fmt.clone()), &info, &opts, &mut out);
            let _ = crate::format_commit(&Format::UserTerminated(fmt), &info, &opts, &mut out);
        }

        /// Date parsing round-trips through the iso format.
        #[test]
        fn timestamp_round_trip(secs in 0i64..4_000_000_000, offset in -780i32..780) {
            let ts = Timestamp::new(secs, offset);
            let rendered = ts.format_iso();
            let parsed = git_date::parse(&rendered, ts).unwrap();
            prop_assert_eq!(parsed.secs, ts.secs);
            prop_assert_eq!(parsed.offset_min, ts.offset_min);
        }
    }
}
