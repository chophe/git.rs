//! `git switch`: switch branches with a stricter interface than checkout.
//!
//! Port of `builtin/checkout.c` (`cmd_switch`): branch-only switching with
//! `-c`/`-C`, `--detach`, `--orphan`, `--discard-changes`, `-q`, and the
//! in-progress-operation guard. Shares the implementation with `checkout`.

use std::io::Write;

use crate::checkout::{parse_checkout_args, run_checkout, Mode};
use crate::{Command, CommandError, RepoContext};

pub struct Switch;

impl Command for Switch {
    fn name(&self) -> &'static str {
        "switch"
    }

    fn run(&self, ctx: &RepoContext, args: &[String], out: &mut dyn Write) -> Result<(), CommandError> {
        let parsed = parse_checkout_args(args, "switch")?;
        if parsed.detach && parsed.new_branch.is_some() {
            return Err(CommandError::fatal(
                "fatal: '--detach' cannot be used with '-c/-C'",
            ));
        }
        run_checkout(ctx, &parsed, out, Mode::Switch)
    }
}
