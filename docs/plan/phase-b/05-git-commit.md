# B5 — `git commit`

**Item**: `git commit` (`-a`, `--amend`, `--allow-empty`, author/committer plumbing, editor, signoff)
**Crate**: `git-command/commit.rs`
**Gate**: `t/t7501`, `t/t7502`

## Description

Port the `git commit` command for creating commits. This includes message
handling, author/committer identity, parent handling, and various commit modes.

## Requirements

- Create commit object from index tree and parent(s)
- Support `-m <message>` for inline message
- Support `-F <file>` for message from file
- Support `--file` as alias for `-F`
- Support `-a` / `--all` to stage modified/deleted files before commit
- Support `--amend` to amend the previous commit
- Support `--allow-empty` to allow empty commits
- Support `--allow-empty-message` for empty commit messages
- Support `-s` / `--signoff` to add Signed-off-by trailer
- Support `--no-signoff` to suppress signoff
- Support `-v` / `--verbose` to show diff in editor
- Support `-q` / `--quiet` for quiet mode
- Support `--cleanup=<mode>` (strip, whitespace, verbatim, scissors)
- Support `--author=<author>` to override author
- Support `--date=<date>` to override author date
- Support `--gpg-sign[=<keyid>]` / `-S` for signing (basic, can defer full GPG)
- Support `--no-gpg-sign` to suppress signing
- Support `--reedit-message=<commit>` / `-c` to reuse/edit message
- Support `--reuse-message=<commit>` / `-C` to reuse message as-is
- Support `--fixup=<commit>` and `--squash=<commit>` for fixup/squash commits
- Support `--reset-author` to reset author to committer
- Support `--trailer <token>:<value>` for custom trailers
- Honor `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`, `GIT_AUTHOR_DATE` env vars
- Honor `GIT_COMMITTER_NAME`, `GIT_COMMITTER_EMAIL`, `GIT_COMMITTER_DATE` env vars
- Launch editor for message input when no `-m` / `-F` provided
- Honor `GIT_EDITOR`, `EDITOR`, `VISUAL` env vars for editor selection
- Handle template file (`commit_template`)
- Support `--no-edit` to skip editor (for `--amend`)
- Support `--dry-run` to show what would be committed
- Support `--status` / `--no-status` for status in editor
- Support `--pathspec-from-file` and `--pathspec-file-nul`

## Test Gate

- `t/t7501-commit.sh` passes through the shim
- `t/t7502-commit.sh` passes through the shim
- Crosswise suite: byte-identical commit objects

## Dependencies

- Phase A2 (repository context)
- Phase A12 (local timezone for dates)
- B3 (`git add` for `-a` flag)
- B4 (`git write-tree` for tree creation)
- `git-object` for commit object format
- `git-date` for date handling

## Implementation Notes

- Match C git's `cmd_commit()` in `builtin/commit.c`
- Commit object format: tree, parent(s), author, committer, message
- Handle merge commits (multiple parents)
- Handle initial commit (no parents)
- Handle commit encoding (UTF-8 default, honor `i18n.commitEncoding`)
- Strip comments from commit message (lines starting with `#`)
