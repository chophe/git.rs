# A6 — `rev-list` / `log` Options

Parent plan: [00-overview.md](00-overview.md). Backlog ref: FOLLOWUPS.md §D
Phase 4 "Remaining" list.

## Goal

Bring the revision walker and its two consumers (`rev-list`, `log`) to the
option surface `t/` scripts actually exercise: object listing, counting,
ordering, path limiting, and `--all`.

## Current Rust state (observed)

- `git-revision` has a basic `RevWalk` (Phase 4: commit parsing, linear
  walks, `resolve_arg` ref support).
- `log.rs` implements only `--oneline`; `rev_list.rs` does a basic walk.
- No ordering modes, no path limiting, no object listing.

## C reference

- `revision.c` (the `rev_info` option machine: ordering, `--all`, boundary,
  limiting), `list-objects.c` (`--objects`), `builtin/rev-list.c`,
  `builtin/log.c`.
- Gates: `t/t6001`–`t/t6019` (rev-list), `t/t4202` (log), `t/t6004`
  (path limiting).

## Deliverables

1. Output options for `rev-list`: `--objects` (commit → tree → blob walk in
   C's order, with edge roots), `--count`, `-n <count>`, `--max-age`/
   `--min-age`, `--no-walk[=(sorted|unsorted)]`, `--do-walk`.
2. Ordering: `--topo-order`, `--date-order`, `--reverse` (C's generation-
   and commit-date-based algorithms from `revision.c` `sort_in_topological_order`).
3. Selection: `--all`, `--branches[=pattern]`, `--tags[=pattern]`,
   `--remotes[=pattern]`, `--glob=`, `--exclude=`, `--first-parent`,
   `--merges`/`--no-merges`, `--min-parents`/`--max-parents`, `--author`/
   `--committer`/`--grep` filters, `--invert-grep`, `-i`.
4. Path limiting: `-- pathspec` filtering via tree-diff (reuses `git-diff`
   tree comparison from Phase 5; history simplification modes
   `--full-history`, `--simplify-merges` are C-parity critical — implement
   default mode first, flag the rest).
5. `--boundary`, `--not`, `^rev` exclusion, `A..B`/`A...B` (consumer side of
   A5's syntax).
6. `log`: `-p`, `--stat`, `--decorate[=short|full|no]`, `--graph`
   (dependencies: A7 pretty engine + A8 diff) — schedule these two flags
   after A7/A8 land; everything else in this item is independent.

## Sub-tasks (ordered)

1. `--count`, `-n`, `--no-walk`, `--all` + ref globs (cheap, unblocks many
   scripts).
2. Ordering algorithms (`--topo-order` with in-degree pass, `--date-order`,
   `--reverse`); proptest: topological output is a valid topological sort of
   the commit DAG, and un-reversing round-trips.
3. Parent filters + `--not`/`^`/ranges.
4. `--objects` walk + `--boundary`.
5. Path limiting (default simplification), then `--full-history`.
6. Content filters (`--author` etc., needs A7 regex/text handling).
7. `log -p/--stat/--graph` after A7/A8.

## Test gates

- `t/t6001`–`t/t6019`, `t/t6004`, `t/t4202` relevant subsets via the shim.
- Crosswise suite: identical repo, run C and Rust `rev-list` with each option
  matrix, assert identical OID sequences (ordering options included).

## Risks / notes

- Commit-date vs generation-number ordering differences with and without a
  commit-graph must match C; the walk currently ignores the commit-graph
  (Phase 3 leftover) — reading it is optional but ordering behavior must not
  depend on its presence (C uses it only as a cache).
- Path-limit history simplification is subtle; land it behind its own
  crosswise suite before any other item builds on it.
