//! The `git` command-line dispatcher.

/// The version reported by `git --version`, tracking the C git version this
/// port is based on.
pub const VERSION: &str = "2.55.0-540";

/// Exit code for usage errors, matching C git (129).
pub const EXIT_USAGE: i32 = 129;

/// Exit code for "command not found".
pub const EXIT_NOT_FOUND: i32 = 1;

/// Run the `git` command with the given arguments (including the program name
/// at index 0). Returns the process exit code.
pub fn run<I>(args: I) -> i32
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = args.into_iter().collect();
    match args.get(1).map(|s| s.as_str()) {
        None | Some("") => {
            eprintln!("usage: git <command> [<args>]");
            EXIT_USAGE
        }
        Some("-v") | Some("--version") | Some("version") => {
            println!("git version {VERSION}");
            0
        }
        Some("--exec-path") => {
            let exe = std::env::current_exe().unwrap_or_default();
            let dir = exe.parent().unwrap_or(std::path::Path::new("."));
            println!("{}", dir.display());
            0
        }
        Some("-h") | Some("--help") | Some("help") => {
            println!("usage: git <command> [<args>]");
            0
        }
        Some(_) => {
            // Parse global options (before the subcommand) from the rest.
            let rest: Vec<String> = args.iter().skip(1).cloned().collect();
            let (ctx, cmd_args) = match git_command::RepoContext::from_global_args(&rest) {
                Ok(v) => v,
                Err(e) => {
                    if !e.message.is_empty() {
                        eprintln!("{}", e.message);
                    }
                    return e.code;
                }
            };
            let Some(cmd) = cmd_args.first().cloned() else {
                eprintln!("usage: git <command> [<args>]");
                return EXIT_USAGE;
            };
            let sub: Vec<String> = cmd_args.iter().skip(1).cloned().collect();
            match git_command::dispatch_with(&ctx, &cmd, &sub, &mut std::io::stdout()) {
                Some(Ok(())) => 0,
                Some(Err(e)) => {
                    if !e.message.is_empty() {
                        eprintln!("{}", e.message);
                    }
                    e.code
                }
                None => {
                    eprintln!("git: '{cmd}' is not a git command. See 'git --help'.");
                    EXIT_NOT_FOUND
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_prints_version() {
        let out = run(["git".to_string(), "--version".to_string()]);
        assert_eq!(out, 0);
    }

    #[test]
    fn no_args_is_usage_error() {
        let out = run(vec!["git".to_string()]);
        assert_eq!(out, EXIT_USAGE);
    }

    #[test]
    fn unknown_command_not_found() {
        let out = run(["git".to_string(), "definitely-not-a-command".to_string()]);
        assert_eq!(out, EXIT_NOT_FOUND);
    }

    #[test]
    fn version_aliases() {
        for flag in ["-v", "version"] {
            let out = run(["git".to_string(), flag.to_string()]);
            assert_eq!(out, 0, "flag {flag}");
        }
    }
}
