# Phase 4 — Summary

Status: **partial** (object model + basic revision walking done and cross-verified;
pretty-printing, trailers, mailmap, path limiting, and commit-graph-driven walks
deferred).

## What was implemented

### `git-object`
- `tree.rs` — `parse_tree`/`serialize_tree` + `compare_entry_names`
  (`base_name_compare` port: dirs sort as `name/`, `'\0' < '.' < '/'`).
  Verified byte-for-byte against real git (a tree with `foo.txt`, `foo`(dir),
  `foo0`, `foo2` sorts as `foo.txt, foo, foo0, foo2`).
- `commit.rs` — `parse_headers` (continuation handling for `gpgsig` etc.),
  `parse_commit` (tree, parents, author, committer, message), `parse_tag`.

### `git-revision` (new crate)
- `RevWalk` — BFS commit walk from tips, first-parent or all-parents, deduped.

### Commands (`git-command`)
- `git cat-file -t|-s|-p|-e` and `<type> <object>` (reads loose + packs via `Odb`).
- `git ls-tree [-r] [-t] [--name-only] <tree>` (recursive, git-compatible dir
  handling and `040000` mode formatting).
- `git mktree` (rebuilds a tree from `ls-tree` output; modes canonicalized to
  octal without leading zeros).
- `git rev-list [--parents] <commit>` (all-parent walk).
- `git log --oneline` + default format (first-parent walk).

## Verified against real git (automated)

`git-command/tests/phase4_crosswise.rs` (3 tests): `ls-tree` (plain/-r/-r -t/
--name-only), `cat-file` (-t/-s/-p for tree/commit/blob), `rev-list` (plain and
--parents), and `log --oneline` are byte-identical to system git on a real repo
with linear history. `mktree` round-trips `ls-tree` output to the same tree oid
as git's own `mktree`.

## Test suite

- 137/137 tests pass (up from 105); zero build warnings.
- `cargo run -p xtask -- scoreboard` now includes the phase4 crosswise suite.

## Deferred / known limitations

- **`HEAD` / ref resolution** — commands require full-length object ids;
  `cat-file -p HEAD` etc. need refs (Phase 7).
- **Pretty-printing** (`pretty.c`): `%` format specifiers, `--format`, trailers,
  mailmap — not started.
- **Revision-walk ordering**: our walk is insertion (BFS) order, matching real
  git for linear history but not for merges (`--topo-order`/`--date-order`
  differences). Path limiting, `--first-parent`, grafts, replace, `--all`,
  commit-graph-driven walks — deferred.
- **`rev-list --objects` / `--count` / `-n`** and other flags — not supported.
- **`log` default format** omits the `Date:` line (needs author-date parsing).
- **Bloom query** and commit-graph walk integration — deferred (see Phase 3).
