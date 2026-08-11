# Phase 6 — Index & Worktree

## Goal

Read and write the index (v2–v4, split, sparse), maintain the cache-tree,
recompute stat information, and drive worktree operations (checkout, entry
writing, status). This phase makes `git status`, `git checkout`, `git reset`,
and `git ls-files` correct against the working tree.

## Module → C-source mapping

| Rust crate/module | C reference |
|---|---|
| `git-index` — `Index` (v2–v4 read/write) | `read-cache.c`, `read-cache.h`, `read-cache-ll.h` |
| `git-index` — `CacheTree` | `cache-tree.c` |
| `git-index` — `SplitIndex` | `split-index.c` |
| `git-index` — `SparseIndex` | `sparse-index.c` |
| `git-index` — `ResolveUndo` | `resolve-undo.c` |
| `git-index` — `StatInfo` (racy-clean, stat-dirty) | `statinfo.c`, `name-hash.c` |
| `git-core::worktree` — `Checkout`, `Entry`, `UnpackTrees` | `checkout.c`, `entry.c`, `unpack-trees.c` |
| `git-core::worktree` — `WtStatus` | `wt-status.c` |

## On-disk formats / surfaces

- Index format: header (DIRC, version, entries), extensions: `TREE` (cache
  tree), `link` (split index), `sdir`/`REUC` (resolve undo), `sdir` (sparse),
  `UNTR` (untracked cache), `EOIE`, `IEOT`, `FSMN` (fsmonitor, optional), and
  the trailing SHA.
- Stat cache semantics: mtime/ctime nanoseconds, racily-clean handling,
  `assume-unchanged`, `skip-worktree`, `intent-to-add`, `--untracked` cache.
- Worktree write semantics: modes, exec bits, symlinks, racy timestamps,
  checkout of directories vs files, `core.autocrlf` (with `convert.c` deferred
  — Phase 10 note).

## Commands enabled

- `git update-index`, `git ls-files`, `git status` (core), `git checkout`
  (basic), `git reset` (soft/mixed), `git checkout-index`

## Fully automated test plan

### Unit
- **Index:** parse/serialize round-trip per version (v2/v3/v4), entry sorting,
  extension parse (TREE, link, REUC, sparse, EOIE/IEOT), trailing-SHA verify.
- **CacheTree:** recompute from a tree == C's cache-tree dump
  (`test-tool dump-cache-tree`).
- **StatInfo:** racily-clean timestamps (write-then-read within the same
  second), stat-dirty transitions, `assume-unchanged`/`skip-worktree` behavior.
- **SplitIndex:** `link` extension, base-index referencing, merge on write.
- **SparseIndex:** sparse↔full conversions, sparse directory entries.
- **Checkout:** entry modes, symlinks, exec bits, directory/file replacement,
  racy-timestamp-aware writes.
- **Status:** porcelain output computation for clean/dirty/untracked/deleted/
  renamed/modified states.

### Property
- Random file-set worktrees → `git status --porcelain` Rust == C.
- `git ls-files --debug` equals C (stat-cache bytes, flags).
- Written index → C `git status` / `git fsck` reads it fine (crosswise).

### Differential
- status / ls-files / update-index / checkout / reset on snapshot worktrees;
  byte-identical output and identical resulting worktree + index.

### `git-test` additions
- `test-read-cache`, `test-write-cache`, `test-dump-cache-tree`,
  `test-dump-split-index`, `test-cache-tree`.

## Gate criteria

- `cargo xtask test`, differential, scoreboard all green.
- Coverage ≥ 90% on git-index and the worktree modules.
- **t/ scripts pass 100%:** `t2100s` (update-index), `t3000s` (ls-files),
  `t7000s` (checkout), `t7510` (status), `t1700` (split index), `t1092`
  (sparse-index), `t2000s` (checkout-index).

## Risks

- Racy-timestamp and stat-cache semantics are time-dependent — tests must pin
  mtimes deterministically (test-helper `test-chmtime` equivalent).
- Cache-tree must be recomputed identically or `status`/`checkout` divergence
  appears only on large repos (crosswise + dump tests catch this).
- Sparse-index conversions must preserve all entry metadata.
