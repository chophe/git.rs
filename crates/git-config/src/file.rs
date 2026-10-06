//! Layout-preserving scope-file editor.
//!
//! A port of the `config.c` store path (`set_multivar_in_file`): edits apply
//! to physical lines (never a re-render from memory), so comments, ordering,
//! and whitespace survive byte-for-byte outside the edited lines. Replaced
//! lines are rewritten canonically (`\tkey = value`, value quoted per C
//! `write_pair`), exactly like C — including dropping the old line's trailing
//! comment and canonicalizing the indent (probed on the tree binary).

use std::path::Path;

use super::{split_key_value, split_section, strip_continuation, unquote_value, ConfigError};

/// A value-pattern matcher for `--replace-all` / `--unset-all` (and the
/// `--value=` form of `set`): fixed-string equality, or an `REG_EXTENDED`
/// subset (`store.value_pattern`) otherwise. A leading `!` negates (C
/// `do_not_match`); callers strip it and flip the result.
#[derive(Debug, Clone)]
pub enum ValueMatcher {
    /// `--fixed-value`: exact string equality.
    Fixed(String),
    /// Regular expression (unanchored search, like `regexec`).
    Regex(String),
}

/// A regex that failed to compile (C `CONFIG_INVALID_PATTERN`,
/// `error("invalid pattern: %s")` at the surface).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidPattern;

impl ValueMatcher {
    /// Build a matcher: `fixed` selects exact matching, otherwise the pattern
    /// is compiled as a regex (validated eagerly so bad patterns fail before
    /// touching the file, like C `regcomp`).
    pub fn compile(pattern: &str, fixed: bool) -> Result<ValueMatcher, InvalidPattern> {
        if fixed {
            return Ok(ValueMatcher::Fixed(pattern.to_string()));
        }
        validate_regex(pattern)?;
        Ok(ValueMatcher::Regex(pattern.to_string()))
    }

    /// Test a parsed entry value (C matches against the parsed value).
    pub fn matches(&self, value: &str) -> bool {
        match self {
            ValueMatcher::Fixed(pat) => value == pat,
            ValueMatcher::Regex(pat) => regex_search(pat, value),
        }
    }
}

/// `set_value` when the key already has several values: C refuses
/// (`CONFIG_NOTHING_SET`, surfaced as "cannot overwrite multiple values with
/// a single value...").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipleValues;

/// One key entry: its section/key identity, parsed value, and physical span.
#[derive(Debug, Clone)]
struct KeySpan {
    line: usize,
    end: usize,
    section: String,
    subsection: Option<String>,
    key: String,
    value: String,
}

/// One physical line's role. `Key` indexes `spans`; `Continued` lines belong
/// to the previous entry (folded into its value, like `parse_into`).
#[derive(Debug, Clone)]
enum Line {
    Blank,
    Comment,
    Other,
    Section { name: String, sub: Option<String> },
    Key(usize),
    Continued,
}

#[derive(Debug)]
struct Doc {
    lines: Vec<String>,
    kinds: Vec<Line>,
    spans: Vec<KeySpan>,
}

fn push_key(
    kinds: &mut Vec<Line>,
    spans: &mut Vec<KeySpan>,
    section: &str,
    subsection: &Option<String>,
    line: usize,
    raw_value: &str,
    key: String,
    continuation: &mut Option<usize>,
) {
    let (stripped, cont) = strip_continuation(raw_value.trim());
    let value = unquote_value(&stripped).unwrap_or_else(|_| stripped.clone());
    let idx = spans.len();
    spans.push(KeySpan {
        line,
        end: line,
        section: section.to_string(),
        subsection: subsection.clone(),
        key,
        value,
    });
    kinds.push(Line::Key(idx));
    if cont {
        *continuation = Some(idx);
    }
}

/// Classify every line, mirroring the `parse_into` grammar (single-line
/// `[section]key = value` headers, backslash continuations ended by blank
/// lines, `#`/`;` comments). Malformed headers (`[` without `]`) are opaque
/// `Other` lines: the surface validates files before editing, and the editor
/// never invents matches in garbage.
fn parse_doc(text: &str) -> Doc {
    let lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
    let mut kinds: Vec<Line> = Vec::with_capacity(lines.len());
    let mut spans: Vec<KeySpan> = Vec::new();
    let mut section = String::new();
    let mut subsection: Option<String> = None;
    let mut continuation: Option<usize> = None;

    for (i, line) in lines.iter().enumerate() {
        if let Some(idx) = continuation.take() {
            if line.trim().is_empty() {
                kinds.push(Line::Blank);
                continue;
            }
            // Continuation body: appended verbatim (like `parse_into`).
            let (body, cont) = strip_continuation(line);
            spans[idx].value.push_str(&body);
            spans[idx].end = i;
            kinds.push(Line::Continued);
            if cont {
                continuation = Some(idx);
            }
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            kinds.push(Line::Blank);
        } else if trimmed.starts_with('#') || trimmed.starts_with(';') {
            kinds.push(Line::Comment);
        } else if trimmed.starts_with('[') {
            match trimmed.find(']') {
                None => kinds.push(Line::Other),
                Some(end) => {
                    let inner = trimmed[1..end].trim();
                    let (s, sub) = split_section(inner);
                    section = s;
                    subsection = sub;
                    kinds.push(Line::Section {
                        name: section.clone(),
                        sub: subsection.clone(),
                    });
                    // C also parses `key = value` after `]` on the same line.
                    let rest = trimmed[end + 1..].trim();
                    if !rest.is_empty() && !rest.starts_with('#') && !rest.starts_with(';') {
                        let (key, raw) = split_key_value(rest);
                        push_key(
                            &mut kinds,
                            &mut spans,
                            &section,
                            &subsection,
                            i,
                            &raw,
                            key,
                            &mut continuation,
                        );
                    }
                }
            }
        } else {
            let (key, raw) = split_key_value(trimmed);
            push_key(
                &mut kinds,
                &mut spans,
                &section,
                &subsection,
                i,
                &raw,
                key,
                &mut continuation,
            );
        }
    }

    Doc { lines, kinds, spans }
}

fn section_eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn key_matches(span: &KeySpan, section: &str, subsection: Option<&str>, key: &str) -> bool {
    section_eq(&span.section, section)
        && span.subsection.as_deref() == subsection
        && span.key == key
}

fn find_matches(doc: &Doc, section: &str, subsection: Option<&str>, key: &str) -> Vec<usize> {
    doc.spans
        .iter()
        .enumerate()
        .filter(|(_, s)| key_matches(s, section, subsection, key))
        .map(|(i, _)| i)
        .collect()
}

/// Quote a value per C `write_pair`: surrounding quotes when it starts/ends
/// with a space or contains `;`, `#`, or `\r`; `\n`/`\t`/`"`/`\` escaped.
pub fn render_value(value: &str) -> String {
    let needs_quote = value.starts_with(' ')
        || value.ends_with(' ')
        || value.chars().any(|c| c == ';' || c == '#' || c == '\r');
    let mut out = String::with_capacity(value.len() + 2);
    if needs_quote {
        out.push('"');
    }
    for c in value.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    if needs_quote {
        out.push('"');
    }
    out
}

/// Render a fresh pair line per C `write_pair` (`\tkey = value`), with an
/// optional `# comment` suffix (C appends ` # comment`, dropping any old
/// trailing comment — probed on the tree binary).
fn render_pair(key: &str, value: &str, comment: Option<&str>) -> String {
    let mut line = format!("\t{} = {}", key, render_value(value));
    if let Some(c) = comment {
        line.push_str(" # ");
        line.push_str(c);
    }
    line
}

/// Render a fresh section header per C `store_create_section`.
fn render_section(section: &str, subsection: Option<&str>) -> String {
    match subsection {
        Some(sub) => format!(
            "[{} \"{}\"]",
            section,
            sub.replace('\\', "\\\\").replace('"', "\\\"")
        ),
        None => format!("[{section}]"),
    }
}

/// End-of-section-block insertion point: the next header line, else EOF
/// (before the phantom trailing element that `split('\n')` leaves on
/// newline-terminated files).
fn section_insert_at(doc: &Doc, header: usize, lines: &[String]) -> usize {
    if let Some(i) = doc.kinds.iter().enumerate().skip(header + 1).find_map(|(i, k)| match k {
        Line::Section { .. } => Some(i),
        _ => None,
    }) {
        return i;
    }
    let mut eof = lines.len();
    if eof > 0 && lines[eof - 1].is_empty() {
        eof -= 1;
    }
    eof
}

/// Append a pair: end of the last matching section, else a new section at
/// EOF (like C; an empty file starts fresh without a stray blank line).
fn append_pair(
    lines: &mut Vec<String>,
    doc: &Doc,
    section: &str,
    subsection: Option<&str>,
    pair: String,
) {
    if lines.len() == 1 && lines[0].is_empty() {
        lines.clear();
    }
    match last_section_line(doc, section, subsection) {
        Some(h) => {
            let at = section_insert_at(doc, h, lines);
            lines.insert(at, pair);
        }
        None => {
            lines.push(render_section(&section.to_ascii_lowercase(), subsection));
            lines.push(pair);
        }
    }
}

/// Last matching section header line, if any.
fn last_section_line(doc: &Doc, section: &str, subsection: Option<&str>) -> Option<usize> {
    doc.kinds.iter().enumerate().rev().find_map(|(i, k)| match k {
        Line::Section { name, sub }
            if section_eq(name, section) && sub.as_deref() == subsection =>
        {
            Some(i)
        }
        _ => None,
    })
}

/// Set a single value: replaces the entry when exactly one matches, appends
/// (end of the last matching section, else a new section at EOF) when none
/// matches, and refuses when several match (C `CONFIG_NOTHING_SET`).
/// `comment` (C `--comment`) is appended as ` # comment`, replacing any old
/// trailing comment.
pub fn set_value(
    text: &str,
    section: &str,
    subsection: Option<&str>,
    key: &str,
    value: &str,
    comment: Option<&str>,
) -> Result<String, MultipleValues> {
    let doc = parse_doc(text);
    let hits = find_matches(&doc, section, subsection, key);
    let mut lines: Vec<String> = doc.lines.clone();
    match hits.len() {
        0 => {
            append_pair(
                &mut lines,
                &doc,
                section,
                subsection,
                render_pair(&key.to_ascii_lowercase(), value, comment),
            );
            Ok(lines.join("\n"))
        }
        1 => {
            let span = &doc.spans[hits[0]];
            lines.splice(
                span.line..=span.end,
                [render_pair(&span.key, value, comment)],
            );
            Ok(lines.join("\n"))
        }
        _ => Err(MultipleValues),
    }
}

/// Append a duplicate key (C `--add`): always a new line at the end of the
/// last matching section, or a new section at EOF.
pub fn add_value(
    text: &str,
    section: &str,
    subsection: Option<&str>,
    key: &str,
    value: &str,
    comment: Option<&str>,
) -> String {
    let doc = parse_doc(text);
    let mut lines: Vec<String> = doc.lines.clone();
    append_pair(
        &mut lines,
        &doc,
        section,
        subsection,
        render_pair(&key.to_ascii_lowercase(), value, comment),
    );
    lines.join("\n")
}

/// Whether `matcher` (if any) accepts the span's parsed value.
fn matcher_accepts(matcher: Option<&ValueMatcher>, value: &str) -> bool {
    matcher.map_or(true, |m| m.matches(value))
}

/// Replace every matching value (C `--replace-all` with optional value
/// pattern). With no matches the value is appended (like C `MULTI_REPLACE`).
/// Returns the new text plus the number of replaced entries.
pub fn replace_all(
    text: &str,
    section: &str,
    subsection: Option<&str>,
    key: &str,
    value: &str,
    matcher: Option<&ValueMatcher>,
    comment: Option<&str>,
) -> (String, usize) {
    let doc = parse_doc(text);
    let targets: Vec<usize> = find_matches(&doc, section, subsection, key)
        .into_iter()
        .filter(|&i| matcher_accepts(matcher, &doc.spans[i].value))
        .collect();
    if targets.is_empty() {
        return (add_value(text, section, subsection, key, value, comment), 0);
    }
    let mut lines: Vec<String> = doc.lines.clone();
    // Descending spans so earlier indices stay valid.
    let mut ordered = targets;
    ordered.sort_by_key(|&i| std::cmp::Reverse(doc.spans[i].line));
    let count = ordered.len();
    for i in ordered {
        let span = &doc.spans[i];
        lines.splice(
            span.line..=span.end,
            [render_pair(&span.key, value, comment)],
        );
    }
    (lines.join("\n"), count)
}

/// Remove every matching entry (C `--unset-all` / `--unset` with optional
/// value pattern). A section left with no keys and no comments loses its
/// header too (C `maybe_remove_section`). Returns the text plus removals.
pub fn unset_all(
    text: &str,
    section: &str,
    subsection: Option<&str>,
    key: &str,
    matcher: Option<&ValueMatcher>,
) -> (String, usize) {
    let doc = parse_doc(text);
    let targets: Vec<usize> = find_matches(&doc, section, subsection, key)
        .into_iter()
        .filter(|&i| matcher_accepts(matcher, &doc.spans[i].value))
        .collect();
    if targets.is_empty() {
        return (text.to_string(), 0);
    }
    let removed = targets.len();
    let mut dead: Vec<bool> = vec![false; doc.lines.len()];
    for &i in &targets {
        let span = &doc.spans[i];
        for line in span.line..=span.end {
            dead[line] = true;
        }
    }
    // Maybe-remove-section: for each removed span's header, drop the header
    // when its block keeps no keys and no comments (blanks don't block, like
    // C which only looks at COMMENT/ENTRY/SECTION events).
    let mut headers: Vec<usize> = Vec::new();
    for &i in &targets {
        let mut h = None;
        for (idx, kind) in doc.kinds.iter().enumerate().take(doc.spans[i].line + 1) {
            if matches!(kind, Line::Section { .. }) {
                h = Some(idx);
            }
        }
        if let Some(h) = h {
            if !headers.contains(&h) {
                headers.push(h);
            }
        }
    }
    for h in headers {
        let end = doc.kinds.iter().enumerate().skip(h + 1).find_map(|(i, k)| match k {
            Line::Section { .. } => Some(i),
            _ => None,
        });
        let end = end.unwrap_or(doc.lines.len());
        let mut keeps_keys = false;
        let mut keeps_comments = false;
        for kind in doc.kinds.iter().take(end).skip(h + 1) {
            match kind {
                Line::Section { .. } => break,
                Line::Comment => keeps_comments = true,
                Line::Key(s) => {
                    if !dead[doc.spans[*s].line] {
                        keeps_keys = true;
                    }
                }
                _ => {}
            }
        }
        if !keeps_keys && !keeps_comments {
            dead[h] = true;
        }
    }
    let lines: Vec<String> = doc
        .lines
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !dead[*i])
        .map(|(_, l)| l)
        .collect();
    (lines.join("\n"), removed)
}

/// C `section_name_match` (`config.c`): match a raw header line against a
/// section name, case-sensitively. The name's first `.` maps to the
/// subsection separator, so `s.y` matches `[s "y"]`; a dotless name matches
/// only a plain `[s]` header, and a dotted name also matches a literal
/// `[s.y]` header.
fn header_matches_section(line: &str, name: &str) -> bool {
    let inner = match line.trim_start().strip_prefix('[') {
        Some(rest) => rest,
        None => return false,
    };
    let end = match inner.find(']') {
        Some(e) => e,
        None => return false,
    };
    let body = inner[..end].trim();
    // Literal match for quote-free headers (covers dotted `[s.y]`).
    if !body.contains('"') && body == name {
        return true;
    }
    let q = match body.find('"') {
        Some(q) => q,
        None => return false,
    };
    let sec = body[..q].trim_end();
    let rest = &body[q + 1..];
    let sub = match rest.find('"') {
        Some(j) => &rest[..j],
        None => return false,
    };
    match name.find('.') {
        Some(d) => sec == &name[..d] && *sub == name[d + 1..],
        None => false,
    }
}

/// Rename a section (C `--rename-section` → `write_section(new_name)`): every
/// header matching `old` (C `section_name_match`, case-sensitive, `sec.sub`
/// matching `[sec "sub"]`) is rewritten as a plain `[new]` header —
/// subsections are dropped, like C. Returns text plus rewritten headers.
pub fn rename_section(text: &str, old: &str, new: &str) -> (String, usize) {
    let doc = parse_doc(text);
    let mut lines: Vec<String> = doc.lines.clone();
    let mut count = 0;
    for (i, kind) in doc.kinds.iter().enumerate() {
        if matches!(kind, Line::Section { .. }) && header_matches_section(&lines[i], old) {
            // Preserve anything after `]` on a single-line header.
            let rest = match lines[i].find(']') {
                Some(j) => lines[i][j + 1..].to_string(),
                None => String::new(),
            };
            lines[i] = format!("[{new}]{rest}");
            count += 1;
        }
    }
    (lines.join("\n"), count)
}

/// Remove a section and all its entries (C `--remove-section`, same
/// `section_name_match` rule as rename). Returns text plus removed headers.
pub fn remove_section(text: &str, section: &str) -> (String, usize) {
    let doc = parse_doc(text);
    let mut dead = vec![false; doc.lines.len()];
    let mut count = 0;
    let mut drops: Vec<(usize, usize)> = Vec::new();
    for (i, kind) in doc.kinds.iter().enumerate() {
        if matches!(kind, Line::Section { .. }) && header_matches_section(&doc.lines[i], section) {
            let end = doc.kinds.iter().enumerate().skip(i + 1).find_map(|(j, k)| match k {
                Line::Section { .. } => Some(j),
                _ => None,
            });
            drops.push((i, end.unwrap_or(doc.lines.len())));
            count += 1;
        }
    }
    for (h, end) in drops {
        for line in h..end {
            dead[line] = true;
        }
    }
    let lines: Vec<String> = doc
        .lines
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !dead[*i])
        .map(|(_, l)| l)
        .collect();
    (lines.join("\n"), count)
}

/// Atomically replace a scope file: write to `<file>.lock` (C `lockfile.c`
/// naming, created exclusively so competing writers fail instead of tearing
/// state), fsync, then rename over the target.
pub fn write_config_file(path: &Path, content: &str) -> Result<(), ConfigError> {
    let lock_string = format!("{}.lock", path.display());
    let lock_path = Path::new(&lock_string);
    {
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock_path)
            .map_err(|e| ConfigError::LockDenied { path: path.to_path_buf(), reason: e.to_string() })?;
        f.write_all(content.as_bytes())
            .map_err(|e| ConfigError::Io(e.to_string()))?;
        f.sync_all()
            .map_err(|e| ConfigError::Io(e.to_string()))?;
    }
    std::fs::rename(lock_path, path).map_err(|e| ConfigError::Io(e.to_string()))?;
    Ok(())
}

// --- REG_EXTENDED subset (C `regcomp`/`regexec` for value patterns) ---

#[derive(Debug, Clone)]
enum Rx {
    Lit(u8),
    Dot,
    Class { neg: bool, ranges: Vec<(u8, u8)> },
    Seq(Vec<Rx>),
    Alt(Vec<Rx>),
    Rep { node: Box<Rx>, min: usize, max: Option<usize> },
    AnchorStart,
    End,
}

fn validate_regex(pat: &str) -> Result<(), InvalidPattern> {
    let bytes = pat.as_bytes();
    let mut pos = 0;
    let (_, end) = parse_rx(bytes, &mut pos, false, true)?;
    if end == bytes.len() {
        Ok(())
    } else {
        Err(InvalidPattern)
    }
}

/// Unanchored `REG_EXTENDED`-subset search (C `regexec` semantics for value
/// and key patterns). Exposed for the `config` command's `--regexp` paths.
pub fn regex_search(pat: &str, text: &str) -> bool {    let bytes = pat.as_bytes();
    let mut pos = 0;
    let Ok((node, _)) = parse_rx(bytes, &mut pos, false, true) else {
        return false;
    };
    let t = text.as_bytes();
    // Unanchored search (`regexec` semantics). `AnchorStart`/`End` nodes
    // constrain from inside, so no position special-casing is needed.
    (0..=t.len()).any(|start| {
        // Never start inside a multi-byte char.
        (start == 0 || start >= t.len() || (t[start] & 0xC0) != 0x80)
            && !rx_match(&node, t, start).is_empty()
    })
}

/// Parse one `|`-separated alternative list. `fresh` tracks whether `^` is
/// legal here (pattern start, after `(`, or after `|` — like glibc ERE).
fn parse_rx(
    p: &[u8],
    pos: &mut usize,
    in_group: bool,
    fresh: bool,
) -> Result<(Rx, usize), InvalidPattern> {
    let mut alts: Vec<Rx> = Vec::new();
    let mut seq: Vec<Rx> = Vec::new();
    let mut fresh = fresh;
    loop {
        if *pos >= p.len() {
            break;
        }
        match p[*pos] {
            b')' if in_group => break,
            // Unmatched `)` is literal (glibc behavior).
            b')' => {
                seq.push(Rx::Lit(b')'));
                *pos += 1;
                fresh = false;
            }
            b'|' => {
                alts.push(finish_seq(seq));
                seq = Vec::new();
                *pos += 1;
                fresh = true;
            }
            // `$` is an anchor only at the end or before `)`/`|`; a `^`
            // opens an anchor only where one is legal; otherwise literal.
            b'$' if *pos + 1 == p.len() || p[*pos + 1] == b')' || p[*pos + 1] == b'|' => {
                seq.push(Rx::End);
                *pos += 1;
                fresh = false;
            }
            b'^' if fresh && seq.is_empty() => {
                seq.push(Rx::AnchorStart);
                *pos += 1;
                fresh = false;
            }
            _ => {
                let atom = parse_atom(p, pos)?;
                let (min, max) = match p.get(*pos) {
                    Some(b'*') => {
                        *pos += 1;
                        (0, None)
                    }
                    Some(b'+') => {
                        *pos += 1;
                        (1, None)
                    }
                    Some(b'?') => {
                        *pos += 1;
                        (0, Some(1))
                    }
                    Some(b'{') => parse_interval_or_literal(p, pos),
                    _ => (1, Some(1)),
                };
                seq.push(if min == 1 && max == Some(1) {
                    atom
                } else {
                    Rx::Rep {
                        node: Box::new(atom),
                        min,
                        max,
                    }
                });
                fresh = false;
            }
        }
    }
    alts.push(finish_seq(seq));
    let consumed = *pos;
    if alts.len() == 1 {
        Ok((alts.swap_remove(0), consumed))
    } else {
        Ok((Rx::Alt(alts), consumed))
    }
}

fn finish_seq(mut seq: Vec<Rx>) -> Rx {
    if seq.len() == 1 {
        seq.swap_remove(0)
    } else {
        Rx::Seq(seq)
    }
}

fn parse_atom(p: &[u8], pos: &mut usize) -> Result<Rx, InvalidPattern> {
    match p.get(*pos) {
        None => Err(InvalidPattern),
        Some(b'.') => {
            *pos += 1;
            Ok(Rx::Dot)
        }
        Some(b'(') => {
            *pos += 1;
            let (node, _) = parse_rx(p, pos, true, true)?;
            if p.get(*pos) != Some(&b')') {
                return Err(InvalidPattern);
            }
            *pos += 1;
            Ok(node)
        }
        Some(b'[') => {
            *pos += 1;
            parse_class(p, pos)
        }
        Some(b'\\') => {
            *pos += 1;
            match p.get(*pos) {
                Some(c) => {
                    *pos += 1;
                    Ok(Rx::Lit(*c))
                }
                None => Err(InvalidPattern),
            }
        }
        // A quantifier with no atom is invalid (glibc `regcomp` agrees).
        Some(b'*') | Some(b'+') | Some(b'?') => Err(InvalidPattern),
        Some(c) => {
            *pos += 1;
            Ok(Rx::Lit(*c))
        }
    }
}

/// Parse `{m}`, `{m,}`, or `{m,n}`. Anything else leaves a literal `{`
/// (glibc treats a malformed interval as ordinary text); `n < m` is invalid.
fn parse_interval_or_literal(p: &[u8], pos: &mut usize) -> (usize, Option<usize>) {
    let save = *pos;
    *pos += 1;
    let mut m: usize = 0;
    let mut digits = 0;
    while let Some(c) = p.get(*pos) {
        if c.is_ascii_digit() {
            m = m.saturating_mul(10).saturating_add((c - b'0') as usize);
            digits += 1;
            *pos += 1;
        } else {
            break;
        }
    }
    if digits == 0 {
        // Literal `{`.
        *pos = save;
        return (1, Some(1));
    }
    if p.get(*pos) == Some(&b'}') {
        *pos += 1;
        return (m, Some(m));
    }
    if p.get(*pos) != Some(&b',') {
        *pos = save;
        return (1, Some(1));
    }
    *pos += 1;
    if p.get(*pos) == Some(&b'}') {
        *pos += 1;
        return (m, None);
    }
    let mut n: usize = 0;
    let mut digits = 0;
    while let Some(c) = p.get(*pos) {
        if c.is_ascii_digit() {
            n = n.saturating_mul(10).saturating_add((c - b'0') as usize);
            digits += 1;
            *pos += 1;
        } else {
            break;
        }
    }
    if digits == 0 || p.get(*pos) != Some(&b'}') {
        *pos = save;
        return (1, Some(1));
    }
    *pos += 1;
    // `{m,n}` with n < m: invalid interval. glibc errors; surface it.
    // (Represented via a sentinel the matcher rejects.)
    if n < m {
        return (1, Some(0));
    }
    (m, Some(n))
}

fn parse_class(p: &[u8], pos: &mut usize) -> Result<Rx, InvalidPattern> {
    let mut neg = false;
    if p.get(*pos) == Some(&b'^') {
        neg = true;
        *pos += 1;
    }
    let mut ranges: Vec<(u8, u8)> = Vec::new();
    let mut first = true;
    loop {
        match p.get(*pos) {
            None => return Err(InvalidPattern),
            Some(b']') if !first => {
                *pos += 1;
                break;
            }
            Some(_) => {
                let lo = parse_class_char(p, pos)?;
                if p.get(*pos) == Some(&b'-')
                    && *pos + 1 < p.len()
                    && p.get(*pos + 1) != Some(&b']')
                {
                    *pos += 1;
                    let hi = parse_class_char(p, pos)?;
                    if hi < lo {
                        return Err(InvalidPattern);
                    }
                    ranges.push((lo, hi));
                } else {
                    ranges.push((lo, lo));
                }
            }
        }
        first = false;
    }
    Ok(Rx::Class { neg, ranges })
}

fn parse_class_char(p: &[u8], pos: &mut usize) -> Result<u8, InvalidPattern> {
    match p.get(*pos) {
        None => Err(InvalidPattern),
        Some(b'\\') => {
            *pos += 1;
            match p.get(*pos) {
                Some(c) => {
                    *pos += 1;
                    Ok(*c)
                }
                None => Err(InvalidPattern),
            }
        }
        Some(c) => {
            *pos += 1;
            Ok(*c)
        }
    }
}

/// All end positions reachable matching `node` at `ti` (backtracking).
fn rx_match(node: &Rx, t: &[u8], ti: usize) -> Vec<usize> {
    match node {
        Rx::AnchorStart => {
            if ti == 0 {
                vec![0]
            } else {
                vec![]
            }
        }
        Rx::End => {
            if ti == t.len() {
                vec![ti]
            } else {
                vec![]
            }
        }
        Rx::Lit(c) => {
            if t.get(ti) == Some(c) {
                vec![ti + 1]
            } else {
                vec![]
            }
        }
        Rx::Dot => {
            if ti < t.len() {
                vec![ti + 1]
            } else {
                vec![]
            }
        }
        Rx::Class { neg, ranges } => match t.get(ti) {
            Some(c) => {
                let hit = ranges.iter().any(|(lo, hi)| lo <= c && c <= hi);
                if hit != *neg {
                    vec![ti + 1]
                } else {
                    vec![]
                }
            }
            None => vec![],
        },
        Rx::Seq(nodes) => {
            let mut cur = vec![ti];
            for n in nodes {
                let mut next = Vec::new();
                for p in cur {
                    next.extend(rx_match(n, t, p));
                }
                cur = next;
                if cur.is_empty() {
                    break;
                }
            }
            cur
        }
        Rx::Alt(alts) => {
            let mut out = Vec::new();
            for a in alts {
                out.extend(rx_match(a, t, ti));
            }
            out
        }
        Rx::Rep { node, min, max } => {
            // Invalid `{m,n}` (n < m) never matches.
            if let Some(max) = max {
                if *max < *min {
                    return vec![];
                }
            }
            // Track (end, count) pairs so `min` filters exactly.
            let mut cur = vec![(ti, 0usize)];
            let mut ends = if *min == 0 { vec![ti] } else { vec![] };
            loop {
                let mut next: Vec<(usize, usize)> = Vec::new();
                for (p, c) in &cur {
                    if let Some(max) = max {
                        if *c >= *max {
                            continue;
                        }
                    }
                    for e in rx_match(node, t, *p) {
                        // No zero-width loops.
                        if e != *p && !next.iter().any(|(x, _)| *x == e) {
                            next.push((e, c + 1));
                        }
                    }
                }
                if next.is_empty() {
                    break;
                }
                for (e, c) in &next {
                    if *c >= *min {
                        ends.push(*e);
                    }
                }
                cur = next;
                // Cap pathological blowup on tiny inputs.
                if ends.len() > 512 {
                    break;
                }
            }
            ends
        }
    }
}
