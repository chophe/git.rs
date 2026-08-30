//! Diff output renderers shared by `diff`, `diff-tree` and `diff-index`
//! sources: patch (with EOF/binary/rename handling), `--stat` (C git's
//! exact `show_stats` algorithm), `--shortstat`, `--numstat`, `--raw`,
//! `--name-only`, `--name-status`, and `--summary`.

use std::collections::HashMap;
use std::io::Write;

use crate::CommandError;
use git_diff::{split_lines, Op};
use git_hash::Oid;
use git_odb::Odb;

pub fn mode6(mode: &Option<String>) -> String {
    mode.clone().unwrap_or_else(|| "100644".to_string())
}

pub fn abbr7(oid: &Option<Oid>) -> String {
    match oid {
        Some(o) => o.to_string()[..7].to_string(),
        None => "0000000".to_string(),
    }
}

pub fn full_hex(oid: &Option<Oid>) -> String {
    match oid {
        Some(o) => o.to_string(),
        None => "0".repeat(40),
    }
}

/// An object source that first consults synthetic (worktree/index) blobs.
pub struct BlobSource<'a> {
    pub odb: &'a Odb,
    pub extra: &'a HashMap<Oid, Vec<u8>>,
}

impl BlobSource<'_> {
    pub fn read(&self, oid: &Oid) -> Option<Vec<u8>> {
        if let Some(data) = self.extra.get(oid) {
            return Some(data.clone());
        }
        self.odb.read(oid).ok().map(|o| o.data)
    }
}

fn blob_pair(c: &git_diff::Change, src: &BlobSource) -> (Vec<u8>, Vec<u8>) {
    let old = match c.old_oid {
        Some(o) => src.read(&o).unwrap_or_default(),
        None => Vec::new(),
    };
    let new = match c.new_oid {
        Some(o) => src.read(&o).unwrap_or_default(),
        None => Vec::new(),
    };
    (old, new)
}

pub fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(8000)].contains(&0)
}

/// Line-level adds/deletes for a change.
pub fn change_line_counts(c: &git_diff::Change, src: &BlobSource) -> (usize, usize) {
    let (old_data, new_data) = blob_pair(c, src);
    if is_binary(&old_data) || is_binary(&new_data) {
        return (0, 0);
    }
    let a = split_lines(&old_data);
    let b = split_lines(&new_data);
    let ops = git_diff::diff_lines(&a, &b);
    let mut adds = 0usize;
    let mut dels = 0usize;
    for op in ops {
        match op {
            Op::Keep => {}
            Op::Delete => dels += 1,
            Op::Insert => adds += 1,
        }
    }
    (adds, dels)
}

/// The display name for a change (`old => new` for renames).
pub fn display_name(c: &git_diff::Change) -> String {
    match (&c.old_path, &c.new_path) {
        (Some(o), Some(n)) if o != n => format!("{o} => {n}"),
        _ => c.path.clone(),
    }
}

/// `--numstat` line.
pub fn render_numstat(c: &git_diff::Change, src: &BlobSource, out: &mut dyn Write) -> Result<(), CommandError> {
    let (old_data, new_data) = blob_pair(c, src);
    if is_binary(&old_data) || is_binary(&new_data) {
        // Binary files render as `-<TAB>-<TAB>path` in --numstat.
        writeln!(out, "-	-	{}", display_name(c))
            .map_err(|e| CommandError::fatal(e.to_string()))?;
        return Ok(());
    }
    let (adds, dels) = change_line_counts(c, src);
    writeln!(out, "{}	{}	{}", adds, dels, display_name(c))
        .map_err(|e| CommandError::fatal(e.to_string()))
}

/// `--name-only` / `--name-status` lines.
pub fn render_name_line(c: &git_diff::Change, with_status: bool, out: &mut dyn Write) -> Result<(), CommandError> {
    if with_status {
        let status = match c.status {
            'R' => format!("R{}", c.score.unwrap_or(0) * 100 / git_diff::MAX_SCORE),
            s => s.to_string(),
        };
        match (&c.old_path, &c.new_path) {
            (Some(o), Some(n)) if o != n => {
                writeln!(out, "{status}	{o}	{n}")
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
            }
            _ => {
                writeln!(out, "{status}	{}", c.path)
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }
    } else {
        writeln!(out, "{}", c.path).map_err(|e| CommandError::fatal(e.to_string()))?;
    }
    Ok(())
}

/// `--raw` line: `:oldmode newmode oldoid newoid status	path`.
pub fn render_raw(c: &git_diff::Change, out: &mut dyn Write) -> Result<(), CommandError> {
    // C git abbreviates raw oids to the default 7 characters.
    fn hex7(oid: &Option<Oid>) -> String {
        match oid {
            Some(o) => o.to_string()[..7].to_string(),
            None => "0000000".to_string(),
        }
    }
    let status = match c.status {
        'R' => format!("R{}", c.score.unwrap_or(0) * 100 / git_diff::MAX_SCORE),
        s => s.to_string(),
    };
    let om = mode6(&c.old_mode);
    let nm = mode6(&c.new_mode);
    match (&c.old_path, &c.new_path) {
        (Some(o), Some(n)) if o != n => {
            writeln!(out, ":{om} {nm} {} {} {status}	{o}	{n}", hex7(&c.old_oid), hex7(&c.new_oid))
                .map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        _ => {
            writeln!(out, ":{om} {nm} {} {} {status}	{}", hex7(&c.old_oid), hex7(&c.new_oid), c.path)
                .map_err(|e| CommandError::fatal(e.to_string()))?;
        }
    }
    Ok(())
}

/// `--summary` lines.
pub fn render_summary(c: &git_diff::Change, out: &mut dyn Write) -> Result<(), CommandError> {
    match c.status {
        'A' => {
            writeln!(out, " create mode {} {}", mode6(&c.new_mode), c.path)
                .map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        'D' => {
            writeln!(out, " delete mode {} {}", mode6(&c.old_mode), c.path)
                .map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        'R' => {
            let o = c.old_path.clone().unwrap_or_default();
            let n = c.new_path.clone().unwrap_or_default();
            writeln!(out, " rename {} => {n} ({:02}%)", o, c.score.unwrap_or(0) * 100 / git_diff::MAX_SCORE)
                .map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        'M' | 'T' => {
            if c.old_mode != c.new_mode {
                writeln!(
                    out,
                    " mode change {} => {} {}",
                    mode6(&c.old_mode),
                    mode6(&c.new_mode),
                    c.path
                )
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn scale_linear(it: usize, width: usize, max_change: usize) -> usize {
    if it == 0 {
        return 0;
    }
    1 + (it * (width - 1) / max_change)
}

fn decimal_width(mut n: usize) -> usize {
    let mut w = 1;
    while n >= 10 {
        n /= 10;
        w += 1;
    }
    w
}

/// C git's `print_stat_summary_inserts_deletes`.
pub fn print_stat_summary(files: usize, adds: u64, dels: u64) -> String {
    if files == 0 {
        return String::new();
    }
    let mut s = format!(" {} file{} changed", files, if files == 1 { "" } else { "s" });
    if adds > 0 {
        s.push_str(&format!(", {} insertion{}(+)", adds, if adds == 1 { "" } else { "s" }));
    }
    if dels > 0 {
        s.push_str(&format!(", {} deletion{}(-)", dels, if dels == 1 { "" } else { "s" }));
    }
    s
}

/// The `--stat` block (C git's `show_stats`, width 80 for non-tty).
pub fn render_stat(changes: &[git_diff::Change], src: &BlobSource, out: &mut dyn Write) -> Result<(), CommandError> {
    if changes.is_empty() {
        return Ok(());
    }
    #[allow(dead_code)]
    struct FileStat {
        name: String,
        added: u64,
        deleted: u64,
        binary: bool,
        old_size: u64,
        new_size: u64,
    }
    let mut files: Vec<FileStat> = Vec::new();
    let mut max_change: u64 = 0;
    let mut max_len: usize = 0;
    let mut has_binary = false;
    for c in changes {
        let name = display_name(c);
        let (old_data, new_data) = blob_pair(c, src);
        let binary = is_binary(&old_data) || is_binary(&new_data);
        if binary {
            has_binary = true;
        }
        let (added, deleted) = if binary {
            (new_data.len() as u64, old_data.len() as u64)
        } else {
            let (a, d) = change_line_counts(c, src);
            (a as u64, d as u64)
        };
        max_len = max_len.max(name.len());
        if !binary {
            max_change = max_change.max(added + deleted);
        }
        files.push(FileStat {
            name,
            added,
            deleted,
            binary,
            old_size: old_data.len() as u64,
            new_size: new_data.len() as u64,
        });
    }

    let width = 80usize;
    // A binary file widens the count column to fit "Bin" (C git parity).
    let number_width = decimal_width(max_change as usize).max(if has_binary { 3 } else { 0 });
    let mut bin_width = 0usize;
    for f in &files {
        if f.binary {
            let w = 14 + decimal_width(f.added as usize) + decimal_width(f.deleted as usize);
            bin_width = bin_width.max(w);
        }
    }

    let graph_width_wanted = if max_change + 4 > bin_width as u64 {
        max_change as usize
    } else {
        bin_width - 4
    };
    let mut graph_width = graph_width_wanted;
    let mut name_width = max_len;
    if name_width + number_width + 6 + graph_width > width {
        if graph_width > width * 3 / 8 - number_width - 6 {
            graph_width = width * 3 / 8 - number_width - 6;
            if graph_width < 6 {
                graph_width = 6;
            }
        }
        if name_width > width - number_width - 6 - graph_width {
            name_width = width - number_width - 6 - graph_width;
        } else {
            graph_width = width - number_width - 6 - name_width;
        }
    }

    for f in &files {
        let mut line = String::new();
        let mut name = f.name.as_str();
        let mut len = name_width;
        let mut prefix = "";
        if name_width < name.len() {
            prefix = "...";
            len = len.saturating_sub(3);
            while name.len() > len && !name.is_empty() {
                name = &name[1..];
            }
            if let Some(slash) = name.find('/') {
                name = &name[slash..];
            }
        }
        let padding = len.saturating_sub(name.len());
        if f.binary {
            line.push_str(&format!(
                " {prefix}{name}{pad:>width$} | {:>nw$}",
                "Bin",
                pad = "",
                width = padding,
                nw = number_width,
            ));
            if f.added == 0 && f.deleted == 0 {
                line.push('\n');
            } else {
                line.push_str(&format!(" {} -> {} bytes
", f.deleted, f.added));
            }
        } else {
            let mut add = f.added as usize;
            let mut del = f.deleted as usize;
            if graph_width as u64 <= max_change {
                let total = scale_linear(add + del, graph_width, max_change as usize);
                let total = if total < 2 && add > 0 && del > 0 { 2 } else { total };
                if add < del {
                    add = scale_linear(add, graph_width, max_change as usize);
                    del = total - add;
                } else {
                    del = scale_linear(del, graph_width, max_change as usize);
                    add = total - del;
                }
            }
            line.push_str(&format!(
                " {prefix}{name}{pad:>width$} | {:>nw$}{}",
                f.added + f.deleted,
                if f.added + f.deleted > 0 { " " } else { "" },
                pad = "",
                width = padding,
                nw = number_width,
            ));
            for _ in 0..add {
                line.push('+');
            }
            for _ in 0..del {
                line.push('-');
            }
            line.push('\n');
        }
        out.write_all(line.as_bytes())
            .map_err(|e| CommandError::fatal(e.to_string()))?;
    }

    let total_files = files.len();
    let adds: u64 = files.iter().filter(|f| !f.binary).map(|f| f.added).sum();
    let dels: u64 = files.iter().filter(|f| !f.binary).map(|f| f.deleted).sum();
    let summary = print_stat_summary(total_files, adds, dels);
    writeln!(out, "{summary}").map_err(|e| CommandError::fatal(e.to_string()))?;
    Ok(())
}

/// `--shortstat`: only the summary line.
pub fn render_shortstat(changes: &[git_diff::Change], src: &BlobSource, out: &mut dyn Write) -> Result<(), CommandError> {
    let mut adds = 0u64;
    let mut dels = 0u64;
    let mut total = 0usize;
    for c in changes {
        let (old_data, new_data) = blob_pair(c, src);
        if is_binary(&old_data) || is_binary(&new_data) {
            total += 1;
            continue;
        }
        total += 1;
        let (a, d) = change_line_counts(c, src);
        adds += a as u64;
        dels += d as u64;
    }
    let summary = print_stat_summary(total, adds, dels);
    if !summary.is_empty() {
        writeln!(out, "{summary}").map_err(|e| CommandError::fatal(e.to_string()))?;
    }
    Ok(())
}

/// Render one change as a patch with context width, rename and binary
/// handling.
pub fn render_change_patch_ctx(
    c: &git_diff::Change,
    src: &BlobSource,
    context: usize,
) -> Result<Vec<u8>, CommandError> {
    let mut out: Vec<u8> = Vec::new();
    let (old_path, new_path) = match (&c.old_path, &c.new_path) {
        (Some(o), Some(n)) if o != n => (o.clone(), n.clone()),
        _ => (c.path.clone(), c.path.clone()),
    };
    writeln!(out, "diff --git a/{old_path} b/{new_path}")
        .map_err(|e| CommandError::fatal(e.to_string()))?;

    match c.status {
        'A' => {
            let mode = mode6(&c.new_mode);
            writeln!(out, "new file mode {mode}").map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "index 0000000..{}", abbr7(&c.new_oid))
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            let new_oid = c.new_oid.ok_or_else(|| CommandError::error("add without new oid"))?;
            let new = src.read(&new_oid).unwrap_or_default();
            if is_binary(&new) {
                writeln!(out, "Binary files /dev/null and b/{new_path} differ")
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
                return Ok(out);
            }
            out.extend_from_slice(b"--- /dev/null
");
            writeln!(out, "+++ b/{new_path}").map_err(|e| CommandError::fatal(e.to_string()))?;
            out.extend_from_slice(&git_diff::diff_blobs_ctx(b"", &new, context));
        }
        'D' => {
            let mode = mode6(&c.old_mode);
            writeln!(out, "deleted file mode {mode}").map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "index {}..0000000", abbr7(&c.old_oid))
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            let old_oid = c.old_oid.ok_or_else(|| CommandError::error("delete without old oid"))?;
            let old = src.read(&old_oid).unwrap_or_default();
            if is_binary(&old) {
                writeln!(out, "Binary files a/{old_path} and /dev/null differ")
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
                return Ok(out);
            }
            writeln!(out, "--- a/{old_path}").map_err(|e| CommandError::fatal(e.to_string()))?;
            out.extend_from_slice(b"+++ /dev/null
");
            out.extend_from_slice(&git_diff::diff_blobs_ctx(&old, b"", context));
        }
        'R' => {
            let score = c.score.unwrap_or(0) * 100 / git_diff::MAX_SCORE;
            writeln!(out, "similarity index {score}%")
                .map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "rename from {old_path}").map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "rename to {new_path}").map_err(|e| CommandError::fatal(e.to_string()))?;
            if c.old_oid == c.new_oid {
                return Ok(out);
            }
            writeln!(
                out,
                "index {}..{} {}",
                abbr7(&c.old_oid),
                abbr7(&c.new_oid),
                mode6(&c.new_mode)
            )
            .map_err(|e| CommandError::fatal(e.to_string()))?;
            let old = c.old_oid.and_then(|o| src.read(&o)).unwrap_or_default();
            let new = c.new_oid.and_then(|o| src.read(&o)).unwrap_or_default();
            if is_binary(&old) || is_binary(&new) {
                writeln!(out, "Binary files a/{old_path} and b/{new_path} differ")
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
                return Ok(out);
            }
            writeln!(out, "--- a/{old_path}").map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "+++ b/{new_path}").map_err(|e| CommandError::fatal(e.to_string()))?;
            out.extend_from_slice(&git_diff::diff_blobs_ctx(&old, &new, context));
        }
        'M' | 'T' => {
            if c.old_mode != c.new_mode {
                writeln!(out, "old mode {}
new mode {}", mode6(&c.old_mode), mode6(&c.new_mode))
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
            }
            writeln!(
                out,
                "index {}..{} {}",
                abbr7(&c.old_oid),
                abbr7(&c.new_oid),
                mode6(&c.new_mode)
            )
            .map_err(|e| CommandError::fatal(e.to_string()))?;
            let old_oid = c.old_oid.ok_or_else(|| CommandError::error("modify without old oid"))?;
            let new_oid = c.new_oid.ok_or_else(|| CommandError::error("modify without new oid"))?;
            let old = src.read(&old_oid).unwrap_or_default();
            let new = src.read(&new_oid).unwrap_or_default();
            if is_binary(&old) || is_binary(&new) {
                writeln!(out, "Binary files a/{old_path} and b/{new_path} differ")
                    .map_err(|e| CommandError::fatal(e.to_string()))?;
                return Ok(out);
            }
            writeln!(out, "--- a/{old_path}").map_err(|e| CommandError::fatal(e.to_string()))?;
            writeln!(out, "+++ b/{new_path}").map_err(|e| CommandError::fatal(e.to_string()))?;
            out.extend_from_slice(&git_diff::diff_blobs_ctx(&old, &new, context));
        }
        _ => {}
    }
    Ok(out)
}

/// Legacy single-change renderer used by `diff-tree` paths.
pub fn render_change_patch(c: &git_diff::Change, odb: &Odb) -> Result<Vec<u8>, CommandError> {
    let empty = HashMap::new();
    let src = BlobSource { odb, extra: &empty };
    render_change_patch_ctx(c, &src, 3)
}

/// Legacy numstat helper for callers without synthetic blobs.
pub fn render_numstat_odb(c: &git_diff::Change, odb: &Odb, out: &mut dyn Write) -> Result<(), CommandError> {
    let empty = HashMap::new();
    let src = BlobSource { odb, extra: &empty };
    render_numstat(c, &src, out)
}
