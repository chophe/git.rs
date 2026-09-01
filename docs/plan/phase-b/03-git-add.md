# B3 — `git add`

**Item**: `git add` (pathspec, ignore integration, refresh, stat handling, `-p` interactive later)
**Crate**: `git-command/add.rs`
**Gate**: `t/t3700`, `t/t3701`

## Description

Port the `git add` command for staging changes. This includes pathspec handling,
gitignore integration, stat-based change detection, and index update operations.

## Requirements

- Support pathspec patterns for selective adding
- Integrate with gitignore engine (Phase A11) to skip ignored files
- Support `-u` / `--update` to stage modifications to tracked files only
- Support `-A` / `--all` to stage all changes including deletions
- Support `-n` / `--dry-run` for dry run mode
- Support `-f` / `--force` to add ignored files
- Support `-i` / `--interactive` (basic mode, full interactive later)
- Support `-p` / `--patch` (interactive patch mode, can be deferred)
- Handle stat-based change detection (skip unchanged files)
- Support `--refresh` option to re-stat without adding
- Handle binary files correctly
- Support `--chmod=+x` / `--chmod=-x` for executable bit
- Handle submodules correctly (don't recurse by default)

## Test Gate

- `t/t3700-rm-and-add.sh` passes through the shim
- `t/t3701-add-interactive.sh` passes through the shim (basic cases)
- Crosswise suite: byte-identical index state after add operations

## Dependencies

- Phase A11 (gitignore + attributes engine)
- Phase A2 (repository context)
- B2 (index extensions)

## Implementation Notes

- Match C git's `cmd_add()` in `builtin/add.c`
- Use `git-update-index` style index manipulation
- Handle the "racy git" problem with stat timestamp resolution
- Support `--intent-to-add` (`-N`) for new files
