# B6 — `git status` Full

**Item**: `git status` full: long/short, `-z`, `--branch`, rename detection, `--ignored`, stat-based scan + racy-clean
**Crate**: `git-command/status.rs`, `git-index`
**Gate**: `t/t7508`, `t/t7010`

## Description

Complete the `git status` command with full option parity. This includes long
and short output formats, branch information, rename detection, ignored file
handling, and stat-based change detection.

## Requirements

### Long format (default)
- Show branch information (on branch, detached HEAD, etc.)
- Show changes to be committed (staged)
- Show changes not staged for commit (modified but not added)
- Show untracked files
- Support `--branch` / `-b` to show branch info even in short format
- Support `--show-stash` to show stash count

### Short format (`-s`, `--short`)
- Two-column output: XY PATH
- XY status codes: M, A, D, R, C, U, ?, !
- Support `--porcelain[=<version>]` for machine-parseable output
- Support `-z` for NUL-terminated output

### Additional options
- Support `--ignored[=<mode>]` to show ignored files (normal, matching, no)
- Support `--untracked-files[=<mode>]` (normal, all, no)
- Support `--ignore-submodules[=<when>]` (all, dirty, untracked, none)
- Support `--column[=<options>]` / `--no-column` for columnar output
- Support `--no-ahead-behind` / `--ahead-behind` for ahead/behind counts
- Support `--renames` / `--no-rename` for rename detection
- Support `--find-renames[=<n>]` for rename detection threshold
- Support `-u` / `--untracked-files` shorthand
- Support `--porcelain` v1 output format

### Performance
- Implement stat-based change detection (skip hashing when mtime unchanged)
- Handle racy-clean files (timestamp resolution issues)
- Use untracked cache extension when available
- Use fsmonitor when available

## Test Gate

- `t/t7508-status.sh` passes through the shim
- `t/t7010-parse-options.sh` passes through the shim
- Crosswise suite: byte-identical status output

## Dependencies

- Phase A8 (diff engine for change detection)
- Phase A11 (gitignore for ignored files)
- Phase A2 (repository context)
- B2 (index extensions)
- B3 (`git add` for staging awareness)

## Implementation Notes

- Match C git's `cmd_status()` in `builtin/status.c`
- Handle submodule status reporting
- Handle sparse checkout correctly
- Support `status.relativePaths` config
- Support `status.short` and `status.branch` config
- Support `status.aheadBehind` config
- Support `status.rename` config
