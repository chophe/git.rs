# B8 — `git rm`, `mv`, `clean`

**Item**: `git rm`, `mv`, `clean`
**Crate**: `git-command`
**Gate**: `t/t3600`, `t/t7001`, `t/t7300`

## Description

Port the file removal, moving, and cleaning commands. These commands manage
tracked files in the working tree and index.

## Requirements

### `git rm`
- Remove files from index and working tree
- Support `--cached` to remove from index only
- Support `--ignore-unmatch` to ignore missing files
- Support `-r` for recursive directory removal
- Support `-f` / `--force` to force removal (override up-to-date check)
- Support `-n` / `--dry-run` for dry run
- Support `-q` / `--quiet` for quiet mode
- Support `--pathspec-from-file` and `--pathspec-file-nul`
- Handle submodule removal correctly
- Prevent removal of files with staged changes (unless `-f`)

### `git mv`
- Move/rename files in index and working tree
- Support `-f` / `--force` to force overwrite
- Support `-k` to skip moves that would fail
- Support `-n` / `--dry-run` for dry run
- Handle directory moves
- Handle case-only renames (on case-insensitive filesystems)
- Detect and handle submodule moves

### `git clean`
- Remove untracked files from working tree
- Support `-n` / `--dry-run` for dry run
- Support `-f` / `--force` to force removal
- Support `-d` to remove untracked directories
- Support `-x` to remove ignored files too
- Support `-X` to remove only ignored files
- Support `-i` / `--interactive` for interactive mode
- Support `-q` / `--quiet` for quiet mode
- Support `--exclude=<pattern>` for exclusion patterns
- Honor `clean.requireForce` config

## Test Gate

- `t/t3600-rm.sh` passes through the shim
- `t/t7001-mv.sh` passes through the shim
- `t/t7300-clean.sh` passes through the shim
- Crosswise suite: byte-identical working tree and index state

## Dependencies

- Phase A2 (repository context)
- Phase A11 (gitignore for clean command)
- B2 (index extensions)
- B3 (`git add` for index manipulation)

## Implementation Notes

- Match C git's `cmd_rm()` in `builtin/rm.c`
- Match C git's `cmd_mv()` in `builtin/mv.c`
- Match C git's `cmd_clean()` in `builtin/clean.c`
- Handle `.gitignore` patterns for clean command
- Support `git clean` directory traversal efficiently
