# A8 — Diff Engine Completion

Parent plan: [00-overview.md](00-overview.md). Backlog ref: FOLLOWUPS.md §D
Phase 5 "Remaining" list. Largest Phase A item.

## Goal

Take `git-diff` from "Myers + unified renderer, byte-identical for plain
text" to the option surface `t/` exercises: worktree/index sources, stat
formats, rename detection, exit-code semantics, binary and EOF handling.

## Current Rust state (observed)

- `git-diff` implements Myers diff, unified renderer, tree-vs-tree
  comparison; `diff --no-index` and `diff-tree` cross-verified byte-identical
  for that subset.
- `diff.rs` parses only `--exit-code` and `--no-index`; `diff-tree` always
  exits 0 even with `--exit-code`.

## C reference

- `diff.c` (option table + output machinery), `diffcore-rename.c` (rename/
  copy detection: similarity estimation, exact-match pass, limit logic),
  `diffcore-pickaxe.c` (`-S`/`-G`), `diffcore.h` (the queue machinery),
  `ws.c` (whitespace rules), `builtin/diff.c` (queue construction from
  index/worktree states).
- Gates: `t/t4001`–`t/t4014` (output formats), `t/t4002`–`t/t4006`
  (rename detection), `t/t4013` (diff formats matrix).

## Deliverables

1. **Sources**: `--cached`/`--staged` (HEAD vs index), `HEAD` implied
   (index+worktree vs HEAD), worktree vs index (diff-files semantics), plus
   `--merge-base`.
2. **Output formats**: `--stat[=<width>,<name-width>,<count>]`,
   `--numstat`, `--shortstat`, `--dirstat[=...]`, `--patch-with-stat`,
   `--patch-with-raw`, `--raw`, `--name-only`, `--name-status`, `--summary`,
   `-U<n>`, `--minimal`, `--patience`, `--histogram` (Myers variants — the
   C xdiff implements all; decide scope: at minimum match C's default
   myers+minimal outputs exactly), `--word-diff[=mode]`, `--color[=when]`,
   `--diff-algorithm=`.
3. **Rename/copy detection**: `-M`/`-C` with similarity thresholds, exact
   rename pass, rename limits (`diff.renameLimit`), `--find-renames`/
   `--find-copies[-harder]`, `--break-rewrites` (later), `--find-copies`
   interaction with `--stat` percentage output.
4. **Filters and selection**: `--diff-filter=[ACDMRTUXB...]`,
   `-s`/`--no-patch`, `--relative[=path]`, `--no-renames`,
   `--ignore-submodules[=when]`, `--ignore-cr-at-eol`, `--ignore-blank-lines`,
   whitespace family (`--check`, `--ws-error-highlight`).
5. **Correctness fixes**: `--exit-code` (diff-tree currently always exits 0),
   `\ No newline at end of file` markers, binary file handling
   (`--binary`, `--full-index`, `Binary files ... differ` line), mode-change
   and symlink diffs, empty-file edge cases.
6. **Pickaxe**: `-S<string>` / `-G<regex>` with `--pickaxe-regex` and
   `--pickaxe-all`.

## Sub-tasks (ordered)

1. EOF/binary/mode/symlink correctness + `--exit-code` (bug fixes first;
   cheap and everything else builds on correct output).
2. `--stat`/`--numstat`/`--shortstat`/`--summary`/`--raw`/`--name-*`
   (rendering-only, no algorithm risk).
3. Sources: `--cached`, worktree/index comparisons (needs `git-index` read —
   present; worktree scan needs the stat handling from B6 — coordinate).
4. Rename/copy detection with the exact-match pass first, then similarity
   estimation; proptest: detected renames are a subset of C's on generated
   repos, byte-equal output where both agree.
5. Whitespace options + `--diff-filter` + `--relative`.
6. `-U<n>`/algorithms/word-diff/color.
7. Pickaxe last.

## Test gates

- `t/t4001`–`t/t4014` incremental (start with `t/t4006-diff-mode.sh`,
  `t/t4013-diff-various.sh` as the smoke matrix).
- Crosswise suites per sub-task group; the existing phase-5 suite must stay
  green throughout.

## Risks / notes

- The stat-graph width computation (terminal-width scaling) has exact C
  logic in `diff.c` (`show_stats`) — port it, don't approximate.
- Word-diff and histogram are large; they may split into an A8b follow-up
  without blocking Phase B (B-items depend on A8's sources + renames, not on
  word-diff).
