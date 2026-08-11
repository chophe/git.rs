# Phase 5 — Diff

## Goal

Reimplement the diff engine and its output formats: the xdiff algorithm family,
the diffcore passes (rename detection, pickaxe, break, order, rotate), and the
user-facing diff formatting. Enables `git diff`, `diff-files`, `diff-index`,
`diff-tree`, and `log -p`.

## Module → C-source mapping

| Rust crate/module | C reference |
|---|---|
| `git-diff::Xdiff` — Myers + `XDF_*` flags, histogram | `xdiff/` (`xdiffi.c`, `xprepare.c`, `xutils.c`, `xemit.c`), `xdiff-interface.c` |
| `git-diff::Xmerge` — 3-way merge of lines (used by Phase 8) | `xdiff/xmerge.c` |
| `git-diff::Diffcore` — rename (exact + similarity), pickaxe, break, order, rotate | `diffcore-rename.c`, `diffcore-pickaxe.c`, `diffcore-break.c`, `diffcore-order.c`, `diffcore-rotate.c`, `diffcore-delta.c` |
| `git-diff::DiffOutput` — unified, word-diff, `--stat/--numstat/--summary`, color | `diff.c`, `diff.h`, `diff-merges.c`, `quote.c`, `ws.c`, `color.c` |
| `git-diff::UserDiff` — textconv drivers | `userdiff.c` |
| `git-diff::CombineDiff` | `combine-diff.c` |
| `git-diff::NoIndex` | `diff-no-index.c` |

## On-disk formats / surfaces

- Unified diff hunks (`@@ -a,b +c,d @@`), context lines, rename/summary lines,
  mode-change records.
- `--stat`/`--numstat`/`--shortstat` formatting.
- Word-diff and color output (config-driven).
- Whitespace options: `-w`, `-b`, `--ignore-blank-lines`, `--ws-error-highlight`.

## Commands enabled

- `git diff`, `git diff-files`, `git diff-index`, `git diff-tree`
- `git log -p`, `git show` (diff form)
- `git diff --stat/--numstat/--summary/--word-diff/--color`

## Fully automated test plan

### Unit
- **Algorithms:** Myers and histogram produce identical output to fixture diffs;
  flag combinations (`XDF_NEED_MINIMAL`, `XDF_IGNORE_WHITESPACE`, …) behave like
  C; binary detection; line-ending handling.
- **Rename detection:** exact renames via hash, similarity scoring identical to
  C, `-C`/`-M` thresholds, broken renames.
- **Output formats:** hunk headers, context count, `--stat` alignment, `--numstat`,
  `--summary`, rename records, color + `--color-moved`.
- **Whitespace:** `-w`, `-b`, `--ignore-blank-lines`, error highlighting.

### Property
- Random file pairs → Rust diff → apply the patch back with C `git apply` →
  resulting tree == original tree (round-trip property).
- Random rename/delete/add sets → rename detection result == C
  (`git diff --name-status -M` differential property).

### Differential
- `git diff` / `git diff-tree` / `git diff --stat` / `--numstat` /
  `--word-diff` byte-identical to C under the same config on fixture + synthetic
  repos.

### Fuzz
- Diff engine and `--word-diff` tokenizer; no panic on arbitrary inputs, defined
  results.

### `git-test` additions
- `test-diff` subset (or reuse `test-userdiff`), `test-wildmatch` (pathspec
  helpers).

## Gate criteria

- `cargo xtask test`, differential, scoreboard all green.
- Coverage ≥ 90% on git-diff.
- **t/ scripts pass 100%:** `t4000`–`t4060`, `t4015` (whitespace), `t4020`–
  `t4045`, `t4051` (rename), `t4040` (rename detection basics).

## Risks

- Byte-for-byte diff output parity is demanding (context, hunk splitting,
  stat column math) — differential tests are the gate.
- Rename similarity scoring must match C's exact hash/ratio computation or
  `-M` results diverge.
- xdiff is ~4K lines of tight, correctness-critical C; port carefully with the
  round-trip property as the safety net.
