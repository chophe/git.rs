//! `git commit-graph`: verify the commit-graph file.

use std::io::Write;

use crate::{Command, CommandError};
use git_commitgraph::CommitGraph;
use git_core::Repository;

pub struct CommitGraphCmd;

impl Command for CommitGraphCmd {
    fn name(&self) -> &'static str {
        "commit-graph"
    }

    fn run(&self, args: &[String], _out: &mut dyn Write) -> Result<(), CommandError> {
        let mut subcommand: Option<String> = None;
        for a in args {
            if a == "write" || a == "verify" {
                subcommand = Some(a.clone());
            } else if !a.starts_with('-') {
                return Err(CommandError::usage(format!("commit-graph: unknown argument '{a}'")));
            }
        }
        let subcommand =
            subcommand.ok_or_else(|| CommandError::usage("commit-graph: need a subcommand (write|verify)"))?;
        if subcommand == "write" {
            return Err(CommandError::fatal(
                "commit-graph write requires commit walking (not yet implemented); use verify",
            ));
        }

        let repo = Repository::discover()?;
        let path = repo.common_dir.join("objects/info/commit-graph");
        let data = std::fs::read(&path).map_err(|_| {
            CommandError::error(format!("could not open commit-graph '{}'", path.display()))
        })?;
        let graph = CommitGraph::parse(data, repo.hash_algo).map_err(CommandError::from)?;
        graph.verify().map_err(|e| CommandError::error(e.to_string()))?;
        Ok(())
    }
}
