# B9 — `git show`, `shortlog`, `describe`, `name-rev`, `whatchanged`

**Item**: `git show`, `shortlog`, `describe`, `name-rev`, `whatchanged`
**Crate**: `git-command`
**Gate**: `t/t4000`, `t/t4201`, `t/t6120`

## Description

Port the object display and commit information commands. These commands show
various representations of commits, tags, and other objects.

## Requirements

### `git show`
- Show commit objects (log message + diff)
- Show tree objects (ls-tree format)
- Show blob objects (raw content)
- Show tag objects (tag info + dereferenced object)
- Support `--pretty=<format>` / `--format=<format>`
- Support `--no-patch` / `-s` to suppress diff
- Support `--stat` / `--numstat` / `--shortstat` for diff stats
- Support `--diff-merges=<style>` for merge diff style
- Support `--output=<file>` to write to file
- Support `--output-indicator-new` / `--output-indicator-old`
- Support multiple objects
- Support `--` to separate paths

### `git shortlog`
- Summarize commit log by author
- Support `-n` / `--numbered` to sort by commit count
- Support `-s` / `--summary` to suppress descriptions
- Support `-e` / `--email` to show email addresses
- Support `--format=<format>` for custom format
- Support `--group=<group>` for grouping (author, committer, trailer)
- Support `--committer` to group by committer
- Support stdin input (log output)
- Support revision ranges

### `git describe`
- Find most recent tag reachable from commit
- Support `--tags` to use any tag, not just annotated
- Support `--all` to use any ref
- Support `--abbrev=<n>` for SHA abbreviation length
- Support `--candidates=<n>` for max candidates
- Support `--exact-match` for exact match only
- Support `--dirty[=<mark>]` for dirty state
- Support `--long` for always long format
- Support `--match <pattern>` for tag pattern
- Support `--exclude <pattern>` for exclusion
- Support `--always` to show SHA on failure
- Support `--first-parent` for first-parent traversal

### `git name-rev`
- Find symbolic names for SHAs
- Support `--name-only` to show name only
- Support `--tags` to use lightweight tags
- Support `--refs=<pattern>` for ref pattern
- Support `--no-undefined` to skip undefined
- Support `--always` to show SHA on failure
- Support stdin input
- Support `--exclude=<pattern>`

### `git whatchanged`
- Show log with diffs (legacy format)
- Support `--no-merges` / `--max-count` etc.
- Support `--pretty=raw` style output
- Support path limiting
- Support revision ranges

## Test Gate

- `t/t4000-diff.sh` passes through the shim (show-related)
- `t/t4201-shortlog.sh` passes through the shim
- `t/t6120-describe.sh` passes through the shim
- Crosswise suite: byte-identical output

## Dependencies

- Phase A6 (rev-list/log options)
- Phase A7 (pretty-printing engine)
- Phase A8 (diff engine)
- Phase A4 (abbreviation resolution)
- `git-object` for object display

## Implementation Notes

- Match C git's `cmd_show()` in `builtin/show.c`
- Match C git's `cmd_shortlog()` in `builtin/shortlog.c`
- Match C git's `cmd_describe()` in `builtin/describe.c`
- Match C git's `cmd_name_rev()` in `builtin/name-rev.c`
- Match C git's `cmd_whatchanged()` in `builtin/whatchanged.c`
- Use pretty engine for format strings
