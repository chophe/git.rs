//! `git init`: create a new empty git repository.

use std::io::Write;

use crate::{Command, CommandError, RepoContext};

pub struct Init;

impl Command for Init {
    fn name(&self) -> &'static str {
        "init"
    }

    fn run(&self, _ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let mut bare = false;
        let mut template: Option<String> = None;
        let mut separate_git_dir: Option<String> = None;
        let mut default_branch: Option<String> = None;
        let mut quiet = false;
        let mut paths: Vec<String> = Vec::new();

        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--bare" => bare = true,
                "--template" => {
                    template = it.next().map(|s| s.to_string());
                }
                "--separate-git-dir" => {
                    separate_git_dir = it.next().map(|s| s.to_string());
                }
                "--initial-branch" | "--initial-branch=" => {
                    let v = if let Some(v) = a.strip_prefix("--initial-branch=") {
                        v.to_string()
                    } else {
                        it.next().map(|s| s.to_string()).unwrap_or_default()
                    };
                    default_branch = Some(v);
                }
                "--quiet" | "-q" => quiet = true,
                "--" => {
                    paths.extend(it.cloned());
                    break;
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    return Err(CommandError::usage(format!("init: unknown option '{s}'")));
                }
                p => paths.push(p.to_string()),
            }
        }

        let target = paths.first().cloned().unwrap_or_else(|| ".".to_string());
        let git_dir = if let Some(sgd) = &separate_git_dir {
            std::path::PathBuf::from(sgd)
        } else {
            std::path::PathBuf::from(&target).join(".git")
        };

        let repo_dir = if bare {
            git_dir.clone()
        } else {
            git_dir.clone()
        };

        std::fs::create_dir_all(&repo_dir).map_err(|e| CommandError::fatal(e.to_string()))?;
        let objects = repo_dir.join("objects");
        std::fs::create_dir_all(&objects).map_err(|e| CommandError::fatal(e.to_string()))?;
        std::fs::create_dir_all(objects.join("info")).map_err(|e| CommandError::fatal(e.to_string()))?;
        std::fs::create_dir_all(objects.join("pack")).map_err(|e| CommandError::fatal(e.to_string()))?;
        std::fs::create_dir_all(repo_dir.join("refs")).map_err(|e| CommandError::fatal(e.to_string()))?;
        std::fs::create_dir_all(repo_dir.join("refs").join("heads")).map_err(|e| CommandError::fatal(e.to_string()))?;
        std::fs::create_dir_all(repo_dir.join("refs").join("tags")).map_err(|e| CommandError::fatal(e.to_string()))?;

        let head_branch = default_branch.unwrap_or_else(|| "refs/heads/master".to_string());
        let head_content = format!("ref: {}\n", head_branch);
        std::fs::write(repo_dir.join("HEAD"), head_content)
            .map_err(|e| CommandError::fatal(e.to_string()))?;

        let config_content = format!(
            "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = {}\n",
            bare
        );
        std::fs::write(repo_dir.join("config"), config_content)
            .map_err(|e| CommandError::fatal(e.to_string()))?;

        let _ = template; // templates not implemented yet

        if !quiet {
            writeln!(out, "Initialized empty Git repository in {}", repo_dir.display())
                .map_err(|e| CommandError::fatal(e.to_string()))?;
        }
        Ok(())
    }
}
