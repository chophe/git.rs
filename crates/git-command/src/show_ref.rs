//! `git show-ref` and `git for-each-ref`: list references.

use std::io::Write;

use crate::{Command, CommandError};
use git_core::Repository;
use git_odb::Odb;
use git_refs::RefStore;

pub struct ShowRef;

impl Command for ShowRef {
    fn name(&self) -> &'static str {
        "show-ref"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        for a in args {
            if !a.starts_with('-') {
                return Err(CommandError::usage(format!("show-ref: unexpected argument '{a}'")));
            }
        }
        let repo = Repository::discover()?;
        let store = RefStore::from_repo(&repo);
        for (name, oid) in store.list() {
            writeln!(out, "{oid} {name}").map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}

pub struct ForEachRef;

impl Command for ForEachRef {
    fn name(&self) -> &'static str {
        "for-each-ref"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut pattern: Option<String> = None;
        let mut format = "%(objectname) %(objecttype)\t%(refname)".to_string();
        for a in args {
            if let Some(f) = a.strip_prefix("--format=") {
                format = f.to_string();
            } else if a.starts_with('-') && a.len() > 1 {
                return Err(CommandError::usage(format!("for-each-ref: option '{a}' not supported")));
            } else {
                pattern = Some(a.clone());
            }
        }

        let repo = Repository::discover()?;
        let store = RefStore::from_repo(&repo);
        let odb = Odb::from_repo(&repo).map_err(CommandError::from)?;
        let refs = store.list();

        for (name, oid) in refs {
            if let Some(p) = &pattern {
                if !name.starts_with(p.as_str()) {
                    continue;
                }
            }
            let kind = odb
                .read(&oid)
                .map(|o| o.kind.as_str().to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            let line = format
                .replace("%(objectname)", &oid.to_string())
                .replace("%(objecttype)", &kind)
                .replace("%(refname)", &name);
            writeln!(out, "{line}").map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}

/// List refs under a prefix, printing the short name (used by `branch` and
/// `tag`). `mark_head` prefixes `* ` to the current branch.
pub fn list_short(
    out: &mut dyn Write,
    prefix: &str,
    mark_head: bool,
) -> Result<(), CommandError> {
    let repo = Repository::discover()?;
    let store = RefStore::from_repo(&repo);
    let head_target = if mark_head {
        store.head_symbolic_target()
    } else {
        None
    };
    for (name, _oid) in store.list() {
        if !name.starts_with(prefix) {
            continue;
        }
        let short = name[prefix.len()..].to_string();
        if mark_head {
            if Some(&name) == head_target.as_ref() {
                writeln!(out, "* {short}").map_err(|e| CommandError::fatal(e.to_string()))?;
            } else {
                writeln!(out, "  {short}").map_err(|e| CommandError::fatal(e.to_string()))?;
            }
        } else {
            writeln!(out, "{short}").map_err(|e| CommandError::fatal(e.to_string()))?;
        }
    }
    Ok(())
}

pub struct Branch;

impl Command for Branch {
    fn name(&self) -> &'static str {
        "branch"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        for a in args {
            match a.as_str() {
                "-l" | "--list" | "-a" | "-r" => {}
                _ => return Err(CommandError::usage(format!("branch: option '{a}' not supported"))),
            }
        }
        list_short(out, "refs/heads/", true)
    }
}

pub struct Tag;

impl Command for Tag {
    fn name(&self) -> &'static str {
        "tag"
    }

    fn run(&self, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        for a in args {
            match a.as_str() {
                "-l" | "--list" => {}
                _ => return Err(CommandError::usage(format!("tag: option '{a}' not supported"))),
            }
        }
        list_short(out, "refs/tags/", false)
    }
}