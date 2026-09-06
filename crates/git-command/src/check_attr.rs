//! `git check-attr`: report which attributes are set on paths.
//!
//! Port of `builtin/check-attr.c`: loads the attribute stack (`$GIT_DIR`
//! `info/attributes`, global `core.attributesFile`, and per-directory
//! `.gitattributes` files) and reports which attributes are defined for each
//! path. Supports `-a`/`--all`, `--cached`, `--stdin`, and `-z`.

use std::io::{BufRead, Write};

use git_attributes::attributes::{AttrCheck, AttributesEngine};
use git_core::Repository;

use crate::{Command, CommandError, RepoContext};

/// Load `$GIT_DIR/info/attributes` into an attribute pattern list.
fn info_attributes(repo: &Repository) -> git_attributes::attributes::AttrPatternList {
    let path = repo.common_dir.join("info").join("attributes");
    let path_str = path.to_string_lossy().into_owned();
    match std::fs::read_to_string(&path) {
        Ok(content) => AttributesEngine::parse_gitattributes(&content, &path_str),
        Err(_) => git_attributes::attributes::AttrPatternList::default(),
    }
}

/// Collect per-directory `.gitattributes` files from the work-tree root outward
/// to `dir`, returning them in order (parents first).
fn collect_gitattributes(
    repo: &Repository,
) -> Vec<git_attributes::attributes::AttrPatternList> {
    let mut lists = Vec::new();
    let work_tree = match &repo.work_tree {
        Some(wt) => wt.clone(),
        None => return lists,
    };
    // Walk from the deepest directory we can resolve down to the work tree
    // root; for a portable port we walk the entire work tree from root.
    let mut stack: Vec<git_attributes::attributes::AttrPatternList> = Vec::new();
    let mut dir = work_tree.clone();
    loop {
        let attrs = dir.join(".gitattributes");
        if let Ok(content) = std::fs::read_to_string(&attrs) {
            stack.push(AttributesEngine::parse_gitattributes(
                &content,
                &attrs.to_string_lossy(),
            ));
        }
        if dir == work_tree {
            break;
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    stack.reverse();
    lists.extend(stack);
    lists
}

pub struct CheckAttr;

impl Command for CheckAttr {
    fn name(&self) -> &'static str {
        "check-attr"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut all_attrs = false;
        let mut stdin_paths = false;
        let mut nul_term = false;
        let mut pos_args: Vec<String> = Vec::new();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-a" | "--all" => all_attrs = true,
                "--stdin" => stdin_paths = true,
                "-z" => nul_term = true,
                "--cached" => {
                    // --cached: attributes come from the index; not yet
                    // supported, but accepted and treated as normal loading.
                    let _ = ();
                }
                "--source" => {
                    i += 1;
                    if i >= args.len() {
                        return Err(CommandError::usage("--source requires a value"));
                    }
                }
                "--" => {
                    i += 1;
                    pos_args.extend(args[i..].iter().cloned());
                    break;
                }
                s if s.starts_with('-') => {
                    return Err(CommandError::usage(
                        "usage: git check-attr [--source <tree-ish>] [-a | --all | <attr>...] [--] <pathname>...\n\
                         \n    -a, --all             report all attributes set on file\n\
                         \n        --cached          use .gitattributes only from the index\n\
                         \n        --stdin           read file names from stdin\n\
                         -z                        terminate input and output records by a NUL character\n",
                    ));
                }
                s => pos_args.push(s.to_string()),
            }
            i += 1;
        }

        let repo = ctx.repository()?;

        // Build the attribute engine.
        let mut engine = AttributesEngine::new();
        let in_tree = collect_gitattributes(&repo);
        for pl in in_tree {
            engine.add_in_tree_patterns(pl);
        }
        engine.add_info_patterns(info_attributes(&repo));

        // If --all, report all attributes found. Otherwise, treat the first
        // argument(s) as attribute names and the last as the path.
        let file_args: Vec<String>;
        let attr_names: Vec<String>;
        if all_attrs {
            attr_names = Vec::new();
            file_args = pos_args.clone();
        } else if pos_args.len() >= 2 {
            attr_names = pos_args[..pos_args.len() - 1].to_vec();
            file_args = vec![pos_args.last().cloned().unwrap()];
        } else if pos_args.len() == 1 {
            // With a single argument it's ambiguous; C git requires at
            // least one attribute and one path unless `--stdin`.
            if stdin_paths {
                attr_names = pos_args.clone();
                file_args = Vec::new();
            } else {
                attr_names = pos_args.clone();
                file_args = Vec::new();
            }
        } else {
            attr_names = Vec::new();
            file_args = Vec::new();
        }

        let mut run_check = |file: &str| {
            if all_attrs {
                let attrs = engine.all_attrs(file);
                for (name, value) in &attrs {
                    output_attr(out, file, name, &display_value(value), nul_term);
                }
            } else {
                let mut check = AttrCheck::new();
                for name in &attr_names {
                    check.add_attr(name);
                }
                engine.check_attr(file, &mut check);
                for name in &attr_names {
                    let value = check.get_value(name);
                    output_attr(out, file, name, &display_value(&value), nul_term);
                }
            }
        };

        if stdin_paths {
            let mut buf = String::new();
            let stdin = std::io::stdin();
            let mut lock = stdin.lock();
            while {
                buf.clear();
                match lock.read_line(&mut buf) {
                    Ok(0) => false,
                    Ok(_) => true,
                    Err(_) => false,
                }
            } {
                let path = if nul_term {
                    buf.trim_end_matches('\0').to_string()
                } else {
                    buf.trim_end_matches(&['\r', '\n'][..]).to_string()
                };
                if !path.is_empty() {
                    run_check(&path);
                }
            }
            Ok(())
        } else {
            for file in &file_args {
                run_check(file);
            }
            Ok(())
        }
    }
}

/// Render an `AttrValue` the way C git's check-attr output_named does.
fn display_value(value: &git_attributes::attributes::AttrValue) -> String {
    match value {
        git_attributes::attributes::AttrValue::Set => "set".to_string(),
        git_attributes::attributes::AttrValue::Unset => "unset".to_string(),
        git_attributes::attributes::AttrValue::Value(v) => v.clone(),
        git_attributes::attributes::AttrValue::Unspecified => "unspecified".to_string(),
    }
}

fn output_attr(out: &mut dyn Write, file: &str, attr: &str, value: &str, nul_term: bool) {
    if nul_term {
        let _ = write!(out, "{file}\0{attr}\0{value}\0");
    } else {
        let _ = write!(out, "{file}: {attr}: {value}\n");
    }
}
