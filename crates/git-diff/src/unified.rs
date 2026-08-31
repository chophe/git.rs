//! Unified-diff rendering, matching git's output for files without a
//! userdiff function-context driver (e.g. plain text).

use super::myers::{split_lines, Op};
use super::userdiff::CompiledDriver;
use std::fmt::Write as _;

/// Number of context lines around each change (git's default).
pub const CONTEXT: usize = 3;

/// Render `b` as a unified diff against `a`, given the edit script.
pub fn render_unified(a: &[&[u8]], b: &[&[u8]], ops: &[Op]) -> Vec<u8> {
    render_unified_ctx(a, b, ops, CONTEXT, None)
}

/// Like [`render_unified`] with an explicit context width (`-U<n>`) and optional driver.
pub fn render_unified_ctx(
    a: &[&[u8]],
    b: &[&[u8]],
    ops: &[Op],
    context: usize,
    driver: Option<&CompiledDriver>,
) -> Vec<u8> {
    // Find runs of change ops.
    let mut extents: Vec<(usize, usize)> = Vec::new();
    let mut k = 0usize;
    while k < ops.len() {
        if ops[k] != Op::Keep {
            let s = k;
            while k < ops.len() && ops[k] != Op::Keep {
                k += 1;
            }
            extents.push((s, k));
        } else {
            k += 1;
        }
    }

    // Merge extents separated by a gap of at most 2*context kept lines.
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in extents {
        if let Some(last) = merged.last_mut() {
            if s - last.1 <= 2 * context {
                last.1 = e;
                continue;
            }
        }
        merged.push((s, e));
    }

    let mut out = Vec::new();
    let mut prev_func_limit = -1isize;
    for (s, e) in merged {
        let lo = s.saturating_sub(context);
        let hi = (e + context).min(ops.len());

        // Line positions at the start of the hunk.
        let (mut old_pos, mut new_pos) = (0usize, 0usize);
        for &op in &ops[..lo] {
            match op {
                Op::Keep => {
                    old_pos += 1;
                    new_pos += 1;
                }
                Op::Delete => old_pos += 1,
                Op::Insert => new_pos += 1,
            }
        }
        // Counts within the hunk.
        let (mut old_count, mut new_count) = (0usize, 0usize);
        for &op in &ops[lo..hi] {
            match op {
                Op::Keep => {
                    old_count += 1;
                    new_count += 1;
                }
                Op::Delete => old_count += 1,
                Op::Insert => new_count += 1,
            }
        }

        // Hunk header: `@@ -a,b +c,d @@`; counts omitted when 1. For a
        // side with zero lines the start points at the line just before
        // (0 at file start), matching C git.
        let old_start = if old_count == 0 { old_pos } else { old_pos + 1 };
        let new_start = if new_count == 0 { new_pos } else { new_pos + 1 };
        let mut hdr = String::new();
        let _ = write!(hdr, "@@ -{old_start}");
        if old_count != 1 {
            let _ = write!(hdr, ",{old_count}");
        }
        let _ = write!(hdr, " +{new_start}");
        if new_count != 1 {
            let _ = write!(hdr, ",{new_count}");
        }
        // Section header: the last kept line before the hunk, appended
        // after the closing `@@` (C git prints it regardless of width).
        hdr.push_str(" @@");
        if old_pos > 0 {
            if let Some(ctx) = find_funcname(a, old_pos - 1, prev_func_limit, driver) {
                let _ = write!(hdr, " {}", String::from_utf8_lossy(ctx));
            }
        }
        prev_func_limit = old_pos as isize - 1;

        hdr.push('\n');
        out.extend_from_slice(hdr.as_bytes());

        let (mut i, mut j) = (old_pos, new_pos);
        for &op in &ops[lo..hi] {
            match op {
                Op::Keep => {
                    out.push(b' ');
                    out.extend_from_slice(a[i]);
                    i += 1;
                    j += 1;
                }
                Op::Delete => {
                    out.push(b'-');
                    out.extend_from_slice(a[i]);
                    i += 1;
                }
                Op::Insert => {
                    out.push(b'+');
                    out.extend_from_slice(b[j]);
                    j += 1;
                }
            }
            // Unterminated last line: C git prints a newline and a
            // backslash marker after the content.
            if !out.ends_with(b"\n") {
                out.push(b'\n');
                out.extend_from_slice(b"\\ No newline at end of file\n");
            }
        }
    }
    out
}

/// Search backwards from `start` for the first line that matches the driver's funcname pattern.
fn find_funcname<'a>(
    a: &[&'a [u8]],
    start: usize,
    limit: isize,
    driver: Option<&CompiledDriver>,
) -> Option<&'a [u8]> {
    for i in (limit.max(0) as usize..=start).rev() {
        let line = a[i];
        let mut match_line = line;
        while !match_line.is_empty()
            && (match_line[match_line.len() - 1] == b'\n'
                || match_line[match_line.len() - 1] == b'\r')
        {
            match_line = &match_line[..match_line.len() - 1];
        }

        if let Some(d) = driver {
            if let Some(m) = d.find_match(match_line) {
                return Some(m);
            }
        } else if let Some(m) = super::userdiff::find_default_match(match_line) {
            return Some(m);
        }
    }
    None
}

/// Produce a unified diff of two blobs.
pub fn diff_blobs(old: &[u8], new: &[u8]) -> Vec<u8> {
    diff_blobs_ctx(old, new, CONTEXT, None)
}

/// Produce a unified diff of two blobs with an explicit context width and optional driver.
pub fn diff_blobs_ctx(
    old: &[u8],
    new: &[u8],
    context: usize,
    driver: Option<&CompiledDriver>,
) -> Vec<u8> {
    let a = split_lines(old);
    let b = split_lines(new);
    let ops = super::myers::diff(&a, &b);
    render_unified_ctx(&a, &b, &ops, context, driver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_newline_marker() {
        let old = b"a\nb\n";
        let new = b"a\nb\nc\n";
        let d = String::from_utf8(diff_blobs(old, new)).unwrap();
        assert!(!d.contains("No newline"));
    }

    #[test]
    fn unterminated_last_line_marks() {
        let old = b"a\nb\n";
        let new = b"a\nB\nd";
        let d = String::from_utf8(diff_blobs(old, new)).unwrap();
        assert!(d.contains("\\ No newline at end of file"), "got: {d}");
    }

    #[test]
    fn context_header_line() {
        let old = b"a\nb\nc\n";
        let new = b"a\nB\nc\nd";
        let d = String::from_utf8(diff_blobs_ctx(old, new, 0, None)).unwrap();
        assert!(d.contains("@@ -2 +2 @@ a"), "got: {d}");
        assert!(d.contains("@@ -3,0 +4 @@ c"), "got: {d}");
    }
}
