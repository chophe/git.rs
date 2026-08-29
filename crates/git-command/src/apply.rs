//! `git apply`: parse a unified diff patch and apply it to files.

use std::io::Write;

use crate::{Command, CommandError, RepoContext};

pub struct Apply;

impl Command for Apply {
    fn name(&self) -> &'static str {
        "apply"
    }

    fn run(&self, _ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut check = false;
        let mut stat = false;
        let mut strip = 1usize;
        let mut patch_file: Option<String> = None;
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--check" => check = true,
                "--stat" => stat = true,
                "--numstat" => {}
                s if s.starts_with("-p") && s.len() > 2 => {
                    strip = s[2..].parse().map_err(|_| CommandError::usage(format!("apply: bad -p value '{s}'")))?;
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("apply: option '{s}' not supported")));
                }
                s => patch_file = Some(s.to_string()),
            }
        }
        let patch_file = patch_file.ok_or_else(|| CommandError::usage("apply: missing <patch>"))?;
        let patch = std::fs::read_to_string(&patch_file)
            .map_err(|e| CommandError::error(format!("cannot read '{patch_file}': {e}")))?;

        let files = parse_patch(&patch).map_err(|e| CommandError::error(e))?;
        if stat {
            for fp in &files {
                writeln!(out, " {}", fp.new_path).map_err(|e| CommandError::fatal(e.to_string()))?;
            }
            return Ok(());
        }
        for fp in &files {
            apply_file_patch(fp, strip, check).map_err(|e| CommandError::error(e.to_string()))?;
        }
        Ok(())
    }
}

/// A line in a hunk.
#[derive(Debug, Clone)]
enum PatchLine {
    Context(Vec<u8>),
    Del(Vec<u8>),
    Add(Vec<u8>),
}

/// A parsed hunk.
#[derive(Debug, Clone)]
struct Hunk {
    old_start: usize,
    lines: Vec<PatchLine>,
}

/// A parsed per-file patch.
#[derive(Debug, Clone)]
struct FilePatch {
    old_path: String,
    new_path: String,
    is_new: bool,
    is_delete: bool,
    hunks: Vec<Hunk>,
}

fn parse_count(s: &str) -> Option<(usize, usize)> {
    // s like "1,8" or "1"
    let mut parts = s.split(',');
    let start: usize = parts.next()?.parse().ok()?;
    let count: usize = parts.next().map(|c| c.parse().ok()).unwrap_or(Some(1))?;
    Some((start, count))
}

/// Parse a unified diff into per-file patches.
fn parse_patch(text: &str) -> Result<Vec<FilePatch>, String> {
    let mut files: Vec<FilePatch> = Vec::new();
    let mut cur: Option<FilePatch> = None;

    // Split into lines keeping each trailing `\n` so content lines (context,
    // `-`, `+`) can match file lines, which include their newline.
    let content_line = |line: &str, has_nl: bool| -> Vec<u8> {
        let mut v = line.as_bytes().to_vec();
        if has_nl {
            v.push(b'\n');
        }
        v
    };
    for raw in split_incl_newline(text.as_bytes()) {
        let has_nl = raw.ends_with(b"\n");
        let line = raw.strip_suffix(b"\n").unwrap_or(&raw);
        let line = std::str::from_utf8(line).map_err(|_| "non-UTF-8 in patch".to_string())?;
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(fp) = cur.take() {
                files.push(fp);
            }
            let mut it = rest.splitn(2, ' ');
            let a = it.next().unwrap_or("").trim_start_matches("a/");
            let b = it.next().unwrap_or("").trim_start_matches("b/");
            cur = Some(FilePatch {
                old_path: a.to_string(),
                new_path: b.to_string(),
                is_new: false,
                is_delete: false,
                hunks: Vec::new(),
            });
        } else if let Some(fp) = cur.as_mut() {
            if line == "new file mode 100644" || line == "new file mode 100755" {
                fp.is_new = true;
            } else if line.starts_with("deleted file mode") {
                fp.is_delete = true;
            } else if let Some(rest) = line.strip_prefix("--- ") {
                if rest != "/dev/null" {
                    fp.old_path = rest.strip_prefix("a/").unwrap_or(rest).to_string();
                }
            } else if let Some(rest) = line.strip_prefix("+++ ") {
                if rest != "/dev/null" {
                    fp.new_path = rest.strip_prefix("b/").unwrap_or(rest).to_string();
                }
            } else if let Some(rest) = line.strip_prefix("@@ ") {
                // Hunk header: @@ -old +new @@
                let hdr = rest.split("@@").next().unwrap_or("").trim();
                let (old_part, new_part) = match hdr.split_once(' ') {
                    Some((o, n)) => (o, n),
                    None => return Err("malformed hunk header".to_string()),
                };
                let (old_start, _old_count) = parse_count(old_part.trim_start_matches('-'))
                    .ok_or_else(|| "malformed hunk old range".to_string())?;
                let (_new_start, _new_count) = parse_count(new_part.trim_start_matches('+'))
                    .ok_or_else(|| "malformed hunk new range".to_string())?;
                fp.hunks.push(Hunk {
                    old_start,
                    lines: Vec::new(),
                });
            } else if let Some(h) = fp.hunks.last_mut() {
                if line.starts_with(' ') {
                    h.lines.push(PatchLine::Context(content_line(&line[1..], has_nl)));
                } else if line.starts_with('-') {
                    h.lines.push(PatchLine::Del(content_line(&line[1..], has_nl)));
                } else if line.starts_with('+') {
                    h.lines.push(PatchLine::Add(content_line(&line[1..], has_nl)));
                }
                // "\\ No newline at end of file" is ignored (lines carry newlines).
            }
        }
    }
    if let Some(fp) = cur.take() {
        files.push(fp);
    }
    Ok(files)
}

/// Strip `n` leading path components.
fn strip_path(path: &str, n: usize) -> String {
    let mut parts: Vec<&str> = path.split('/').collect();
    for _ in 0..n {
        if parts.len() > 1 {
            parts.remove(0);
        }
    }
    parts.join("/")
}

/// Apply a single file patch.
fn apply_file_patch(fp: &FilePatch, strip: usize, check: bool) -> Result<(), String> {
    if fp.is_new {
        let path = strip_path(&fp.new_path, strip);
        let content = hunks_to_content(&fp.hunks)?;
        if !check {
            if let Some(dir) = std::path::Path::new(&path).parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, content).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    if fp.is_delete {
        let path = strip_path(&fp.old_path, strip);
        if !check {
            let _ = std::fs::remove_file(&path);
        }
        return Ok(());
    }

    let path = strip_path(&fp.old_path, strip);
    let data = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let mut lines: Vec<Vec<u8>> = split_incl_newline(&data);
    let mut offset: i64 = 0;
    for hunk in &fp.hunks {
        let start = hunk.old_start as i64 - 1 + offset;
        if start < 0 || start as usize > lines.len() {
            return Err(format!("hunk at {path} out of range"));
        }
        let start = start as usize;
        // Verify context/deletion lines match.
        let old_count = hunk
            .lines
            .iter()
            .filter(|l| !matches!(l, PatchLine::Add(_)))
            .count();
        if start + old_count > lines.len() {
            return Err(format!("hunk does not apply to {path}"));
        }
        let mut li = start;
        for pl in &hunk.lines {
            match pl {
                PatchLine::Context(expected) | PatchLine::Del(expected) => {
                    if lines.get(li).map(|l| l == expected).unwrap_or(false) {
                        li += 1;
                    } else {
                        return Err(format!(
                            "hunk context mismatch at {path}:{}",
                            li + 1
                        ));
                    }
                }
                PatchLine::Add(_) => {}
            }
        }
        // Replace [start..start+old_count] with the hunk's new-side lines
        // (context and additions, in order; deletions drop out).
        let new_lines: Vec<Vec<u8>> = hunk
            .lines
            .iter()
            .filter_map(|l| match l {
                PatchLine::Context(b) | PatchLine::Add(b) => Some(b.clone()),
                PatchLine::Del(_) => None,
            })
            .collect();
        lines.splice(start..start + old_count, new_lines.iter().cloned());
        let new_count = hunk.lines.iter().filter(|l| !matches!(l, PatchLine::Del(_))).count();
        offset += new_count as i64 - old_count as i64;
    }

    if !check {
        let content: Vec<u8> = lines.concat();
        std::fs::write(&path, content).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn hunks_to_content(hunks: &[Hunk]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for h in hunks {
        for pl in &h.lines {
            if let PatchLine::Add(b) = pl {
                out.extend_from_slice(b);
            }
        }
    }
    Ok(out)
}

/// Split bytes into lines, each including its trailing `\n` (last may lack it).
fn split_incl_newline(data: &[u8]) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    for (i, &b) in data.iter().enumerate() {
        if b == b'\n' {
            lines.push(data[start..=i].to_vec());
            start = i + 1;
        }
    }
    if start < data.len() {
        lines.push(data[start..].to_vec());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_applies_a_modification() {
        let patch = "\
diff --git a/f.txt b/f.txt
index 71ac1b5..38346b2 100644
--- a/f.txt
+++ b/f.txt
@@ -1,2 +1,3 @@
 a
+X
 b
";
        let files = parse_patch(patch).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].new_path, "f.txt");
        assert!(!files[0].is_new && !files[0].is_delete);
        assert_eq!(files[0].hunks.len(), 1);
    }

    #[test]
    fn strips_one_component() {
        assert_eq!(strip_path("a/f.txt", 1), "f.txt");
        assert_eq!(strip_path("a/sub/f.txt", 2), "f.txt");
        assert_eq!(strip_path("f.txt", 0), "f.txt");
    }
}