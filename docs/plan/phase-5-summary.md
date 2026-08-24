# Phase 5 — Summary

Status: **partial** (tree diff + unified content diff for text without
function-context drivers done and cross-verified; rename detection, pickaxe,
stat/numstat, word-diff, color, `apply` deferred).

## What was implemented

### `git-diff` crate (new)
- `myers.rs` — classic Myers O(ND) diff (`split_lines`, `diff`). Tie-breaking
  matches git's `-old +new` ordering for single-line replacements.
- `unified.rs` — unified-diff rendering matching git byte-for-byte for files
  without a userdiff function-context driver: hunk merging (gap <= 2·context),
  `@@ -a,b +c,d @@` headers with count omitted when 1 and `-0,0`/`+0,0` for
  empty sides.
- `tree.rs` — recursive tree comparison → `Change { path, old/new mode+oid,
  status A/M/D/T }` using `compare_entry_names` for sorted merge walk.

### Commands (`git-command`)
- `git diff-tree [ -r | --name-status | --name-only | -p ] <t1> <t2>` —
  raw, name-status, name-only, and unified patch output.
- `git diff [--exit-code] <t1> <t2>` — unified patch between two trees
  (recursive), exits 1 only with `--exit-code` (matching git 2.50).
- `git diff --no-index <a> <b>` — file diff, exits 1 on differences.
- `patch.rs` — shared patch header rendering (A/M/D with `new file mode` /
  `deleted file mode`, abbreviated `index` lines).

## Verified against real git (automated)

`git-command/tests/phase5_crosswise.rs` (3 tests): `diff-tree` (raw, -r,
name-status, name-only, -p), `diff <t1> <t2>` (byte-identical patch + exit
codes incl. `--exit-code`), and `diff --no-index` are byte-identical to system
git on a repo with a modification, addition, and deletion.

## Test suite

- 148/148 tests pass (up from 137); zero build warnings.
- `cargo run -p xtask -- scoreboard` now includes the phase5 crosswise suite.

## Deferred / known limitations

- **Rename/copy detection** (`diffcore-rename`), pickaxe, break/order/rotate —
  not started.
- **Output formats**: `--stat`, `--numstat`, `--summary`, `--word-diff`,
  color, `--diff-filter`, `--summary` — not implemented.
- **Function-context headers** (`@@ ... @@ fn foo()`) require userdiff drivers —
  unified output matches git only for files without them (plain text).
- **`git apply` / `git am`** (apply.c) — not started (Phase 10).
- **Working-tree diff** (`git diff` with no args, `diff-files`, `diff-index`)
  needs the index/worktree (Phase 6).
- **Binary files** and **no-newline-at-EOF** markers — not handled.
- **`--exit-code`** for `diff-tree` — not implemented (always exits 0).