# B1 — `git init`

**Item**: `git init` (templates, `--bare`, `--separate-git-dir`, default branch)
**Crate**: `git-command/init.rs`
**Gate**: `t/t0001`

## Description

Port the `git init` command with full option parity to C git. This creates a new
Git repository, including the `.git` directory structure, initial `HEAD` file,
and default configuration.

## Requirements

- Create standard `.git` directory structure (`objects/`, `refs/heads/`, `refs/tags/`)
- Support `--bare` flag for bare repositories
- Support `--separate-git-dir <path>` to place git directory elsewhere
- Support `--initial-branch=<name>` / `-b <name>` for default branch name
- Support template directory (`--template=<path>`) for custom initialization files
- Honor `GIT_DIR` / `GIT_WORK_TREE` environment variables
- Honor `init.defaultBranch` config setting
- Create initial `HEAD` file pointing to the default branch
- Write default config values (repository format version, filemode, etc.)
- Support `--quiet` / `-q` flag

## Test Gate

- `t/t0001-init.sh` passes through the shim
- Crosswise suite: byte-identical output with C git for all option combinations

## Dependencies

- Phase A2 (`--git-dir`/`--work-tree` threading) for repository discovery
- `git-config` for reading `init.defaultBranch`

## Implementation Notes

- Match C git's `init_db()` function behavior in `builtin/init.c`
- Handle shared repository permissions (`--shared` flag)
- Support reinitialization of existing repositories (idempotent behavior)
