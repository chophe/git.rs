//! Safety gate: zero unjustified `unsafe` in first-party code (FR-028).
//!
//! Scans every `.rs` file under `crates/` (excluding `crates/target/`) for
//! the `unsafe` keyword outside comments and string/char literals. Hits that
//! match the documented allowlist below print as `ALLOWED` with their reason;
//! anything else prints as `VIOLATION` naming file:line and fails the gate.
//! Only std is used.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level under the workspace root")
        .to_path_buf()
}

/// (file-name substring, line-content substring, reason) for benign hits.
fn allowlist() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        (
            "userdiff.rs",
            "unsafe|sealed|abstract|partial",
            "C# keyword list inside a userdiff regex literal (mirrors C git userdiff patterns)",
        ),
        (
            "userdiff.rs",
            "async|const|unsafe|extern",
            "Rust keyword list inside a userdiff regex literal",
        ),
    ]
}

#[derive(Clone, Copy, PartialEq)]
enum Scan {
    Code,
    LineComment,
    BlockComment(usize),
    Str,
    StrEscape,
    StrContinued,
    Char,
    CharEscape,
    RawStr(usize), // number of '#' delimiters
}

/// Strip comments and string/char literals from one line, threading scanner
/// state across lines (multi-line strings via `\` continuation, raw strings,
/// block comments). Returns the code-only remainder of the line.
fn strip_line(line: &str, state: &mut Scan) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    if *state == Scan::LineComment {
        *state = Scan::Code;
    }
    if *state == Scan::StrContinued {
        *state = Scan::Str;
    }
    while i < bytes.len() {
        let b = bytes[i];
        match *state {
            Scan::Code => {
                if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    *state = Scan::LineComment;
                    break;
                } else if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    *state = Scan::BlockComment(1);
                    i += 2;
                } else if b == b'"' {
                    *state = Scan::Str;
                    i += 1;
                } else if b == b'r' && i + 1 < bytes.len() && (bytes[i + 1] == b'"' || bytes[i + 1] == b'#') {
                    let mut j = i + 1;
                    let mut hashes = 0;
                    while j < bytes.len() && bytes[j] == b'#' {
                        hashes += 1;
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b'"' {
                        *state = Scan::RawStr(hashes);
                        i = j + 1;
                    } else {
                        out.push(b as char);
                        i += 1;
                    }
                } else if b == b'\'' {
                    *state = Scan::Char;
                    i += 1;
                } else {
                    out.push(b as char);
                    i += 1;
                }
            }
            Scan::LineComment => break,
            Scan::BlockComment(depth) => {
                if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    *state = Scan::BlockComment(depth + 1);
                    i += 2;
                } else if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    *state = if depth > 1 { Scan::BlockComment(depth - 1) } else { Scan::Code };
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Scan::Str => {
                if b == b'\\' {
                    *state = Scan::StrEscape;
                    i += 1;
                } else if b == b'"' {
                    *state = Scan::Code;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            Scan::StrEscape => {
                *state = Scan::Str;
                i += 1;
            }
            Scan::StrContinued => {
                *state = Scan::Str;
            }
            Scan::Char => {
                if b == b'\\' {
                    *state = Scan::CharEscape;
                    i += 1;
                } else if b == b'\'' {
                    *state = Scan::Code;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            Scan::CharEscape => {
                *state = Scan::Char;
                i += 1;
            }
            Scan::RawStr(hashes) => {
                if b == b'"' {
                    let mut j = i + 1;
                    let mut seen = 0;
                    while seen < hashes && j < bytes.len() && bytes[j] == b'#' {
                        seen += 1;
                        j += 1;
                    }
                    if seen == hashes {
                        *state = Scan::Code;
                        i = j;
                    } else {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
    // A `\` at end of line inside a string continues it on the next line.
    if *state == Scan::Str && line.as_bytes().last() == Some(&b'\\') {
        *state = Scan::StrContinued;
    }
    out
}

fn is_ident_boundary(code: &str, start: usize, end: usize) -> bool {
    let left = code[..start].chars().last().map_or(true, |c| !(c.is_alphanumeric() || c == '_'));
    let right = code[end..].chars().next().map_or(true, |c| !(c.is_alphanumeric() || c == '_'));
    left && right
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            collect_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

pub fn run() -> bool {
    let root = workspace_root();
    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    files.sort();

    let mut ok = true;
    let mut hits = 0;
    for path in &files {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let mut state = Scan::Code;
        let rel = path.strip_prefix(&root).unwrap_or(path).display().to_string();
        for (n, line) in text.lines().enumerate() {
            let code = strip_line(line, &mut state);
            let mut search = 0;
            while let Some(pos) = code[search..].find("unsafe") {
                let start = search + pos;
                let end = start + "unsafe".len();
                if is_ident_boundary(&code, start, end) {
                    hits += 1;
                    let allowed = allowlist().iter().find(|(f, sub, _)| {
                        rel.contains(f) && line.contains(sub)
                    });
                    match allowed {
                        Some((_, _, reason)) => {
                            println!("ALLOWED {}:{}: {reason}", rel, n + 1);
                        }
                        None => {
                            println!("VIOLATION {}:{}: unjustified `unsafe` (add justification + safe wrapper + tests, or remove)", rel, n + 1);
                            ok = false;
                        }
                    }
                    search = end;
                } else {
                    search = end;
                }
            }
        }
    }
    println!(
        "safety: {} files scanned, {} `unsafe` hits: {}",
        files.len(),
        hits,
        if ok { "PASS" } else { "FAIL" }
    );
    ok
}
