//! `git merge-file`: 3-way content merge of three files.

use std::io::Write;

use crate::{Command, CommandError};
use git_diff::split_lines;
use git_merge::{diff_changes, merge3};

pub struct MergeFile;

impl Command for MergeFile {
    fn name(&self) -> &'static str {
        "merge-file"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut to_stdout = false;
        let mut rest: Vec<String> = Vec::new();
        for a in args {
            match a.as_str() {
                "-p" => to_stdout = true,
                "-q" | "--diff3" => {}
                "-L" | "--marker-size" => {
                    // consume the next argument as the label value
                    let _ = rest.pop();
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("merge-file: option '{s}' not supported")));
                }
                s => rest.push(s.to_string()),
            }
        }
        if rest.len() != 3 {
            return Err(CommandError::usage("merge-file: requires <ours> <base> <theirs>"));
        }
        let (ours_path, base_path, theirs_path) = (&rest[0], &rest[1], &rest[2]);

        let ours_bytes = std::fs::read(ours_path).map_err(|e| CommandError::error(e.to_string()))?;
        let base_bytes = std::fs::read(base_path).map_err(|e| CommandError::error(e.to_string()))?;
        let theirs_bytes = std::fs::read(theirs_path).map_err(|e| CommandError::error(e.to_string()))?;

        let base = split_lines(&base_bytes);
        let ours = split_lines(&ours_bytes);
        let theirs = split_lines(&theirs_bytes);

        let oc = diff_changes(&base, &ours);
        let tc = diff_changes(&base, &theirs);
        let r = merge3(&base, &oc, &tc, ours_path, theirs_path);

        let result: Vec<u8> = r.lines.concat();
        if to_stdout {
            out.write_all(&result).map_err(|e| CommandError::fatal(e.to_string()))?;
        } else {
            std::fs::write(ours_path, &result).map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        // git exits 1 when the merge produced conflicts.
        if r.conflict {
            Err(CommandError::silent(1))
        } else {
            Ok(())
        }
    }
}