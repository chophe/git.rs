//! Shared patch rendering for `git diff` / `git diff-tree -p`.

use std::io::Write;

use crate::CommandError;
use git_diff::{diff_blobs, Change};
use git_hash::Oid;
use git_odb::Odb;

/// A mode formatted as git does (6-digit, zero-padded).
pub fn mode6(mode: &Option<String>) -> String {
    match mode {
        Some(m) => format!("{m:0>6}"),
        None => "000000".to_string(),
    }
}

/// The abbreviated (7-hex) form of an oid, or 7 zeros when absent.
pub fn abbr7(oid: &Option<Oid>) -> String {
    match oid {
        Some(o) => o.to_string()[..7].to_string(),
        None => "0000000".to_string(),
    }
}

/// The full hex form of an oid, or all zeros when absent.
pub fn full_hex(oid: &Option<Oid>) -> String {
    match oid {
        Some(o) => o.to_string(),
        None => "0".repeat(40),
    }
}

/// Compute (added, deleted) line counts for a blob change.
pub fn change_line_counts(c: &Change, odb: &Odb) -> (usize, usize) {
    let (old_data, new_data) = match (c.old_oid, c.new_oid) {
        (Some(o), Some(n)) => {
            let old = match odb.read(&o) {
                Ok(x) => x.data,
                Err(_) => return (0, 0),
            };
            let new = match odb.read(&n) {
                Ok(x) => x.data,
                Err(_) => return (0, 0),
            };
            (old, new)
        }
        (None, Some(n)) => (Vec::new(), match odb.read(&n) {
            Ok(x) => x.data,
            Err(_) => return (0, 0),
        }),
        (Some(o), None) => (
            match odb.read(&o) {
                Ok(x) => x.data,
                Err(_) => return (0, 0),
            },
            Vec::new(),
        ),
        (None, None) => return (0, 0),
    };
    let a = git_diff::split_lines(&old_data);
    let b = git_diff::split_lines(&new_data);
    let ops = git_diff::diff_lines(&a, &b);
    let mut adds = 0usize;
    let mut dels = 0usize;
    for op in ops {
        match op {
            git_diff::Op::Keep => {}
            git_diff::Op::Delete => dels += 1,
            git_diff::Op::Insert => adds += 1,
        }
    }
    (adds, dels)
}

/// `--numstat` line: `adds\tdels\tpath`.
pub fn render_numstat(c: &Change, odb: &Odb, out: &mut dyn Write) -> Result<(), CommandError> {
    let (adds, dels) = change_line_counts(c, odb);
    writeln!(out, "{}\t{}\t{}", adds, dels, c.path)
        .map_err(|e| CommandError::fatal(e.to_string()))
}

/// Render a unified patch for one tree change (blob-level).
pub fn render_change_patch(c: &Change, odb: &Odb) -> Result<Vec<u8>, CommandError> {
    let mut out = Vec::new();
    writeln!(out, "diff --git a/{} b/{}", c.path, c.path).map_err(|e| CommandError::fatal(e.to_string()))?;

    match c.status {
        'A' => {
            let mode = mode6(&c.new_mode);
            writeln!(out, "new file mode {mode}").map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "index 0000000..{}", abbr7(&c.new_oid)).map_err(|e| CommandError::fatal(e.to_string()))?;
            out.extend_from_slice(b"--- /dev/null\n");
            writeln!(out, "+++ b/{}", c.path).map_err(|e| CommandError::fatal(e.to_string()))?;
            let new_oid = c.new_oid.ok_or_else(|| CommandError::error("add without new oid"))?;
            let new = odb.read(&new_oid).map_err(|e| CommandError::error(e.to_string()))?;
            out.extend_from_slice(&diff_blobs(b"", &new.data));
        }
        'D' => {
            let mode = mode6(&c.old_mode);
            writeln!(out, "deleted file mode {mode}").map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "index {}..0000000", abbr7(&c.old_oid)).map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "--- a/{}", c.path).map_err(|e| CommandError::fatal(e.to_string()))?;
            out.extend_from_slice(b"+++ /dev/null\n");
            let old_oid = c.old_oid.ok_or_else(|| CommandError::error("delete without old oid"))?;
            let old = odb.read(&old_oid).map_err(|e| CommandError::error(e.to_string()))?;
            out.extend_from_slice(&diff_blobs(&old.data, b""));
        }
        'M' | 'T' => {
            let mode = mode6(&c.new_mode);
            writeln!(out, "index {}..{} {mode}", abbr7(&c.old_oid), abbr7(&c.new_oid))
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "--- a/{}", c.path).map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "+++ b/{}", c.path).map_err(|e| CommandError::fatal(e.to_string()))?;
            let old_oid = c.old_oid.ok_or_else(|| CommandError::error("modify without old oid"))?;
            let new_oid = c.new_oid.ok_or_else(|| CommandError::error("modify without new oid"))?;
            let old = odb.read(&old_oid).map_err(|e| CommandError::error(e.to_string()))?;
            let new = odb.read(&new_oid).map_err(|e| CommandError::error(e.to_string()))?;
            out.extend_from_slice(&diff_blobs(&old.data, &new.data));
        }
        _ => {}
    }
    Ok(out)
}