# Phase 6 — Summary

Status: **partial** (index v2 read/write + `ls-files`, `update-index`,
`status --porcelain` done and cross-verified; cache-tree, split/sparse index,
checkout, reset, `diff-files`/`diff-index`, racy-clean stat handling deferred).

## What was implemented

### `git-index` crate (new)
- Index **version 2** read/write (`read-cache.c` v2 port):
  - header (`DIRC`, version, count), 40-byte stat fields, raw oid, 16-bit
    flags (`CE_VALID` assume-valid, stage bits, capped name length),
    name + NUL, 8-byte entry alignment (`(62 + namelen + 8) & ~7` for sha1),
    trailing checksum.
  - Long names (> 0xFFF) handled via the NUL-scan path.
  - Version 3/4 rejected explicitly (deferred).

### `git-core`
- `Repository::index_file()`, `Repository::resolve_head()` (loose refs).

### Commands (`git-command`)
- `git ls-files [--stage]` — list index paths (stage-0 default) or all stages.
- `git update-index --add|--remove <paths>` — stat + write blobs + upsert
  entries, keeps the index sorted, writes atomically.
- `git status --porcelain` — column X (index vs HEAD tree, resolved via
  `resolve_head`; empty base with no commit) and column Y (worktree vs index),
  plus `??` untracked files.

## Verified against real git (automated)

`git-command/tests/phase6_crosswise.rs` (3 tests):
- **our** `update-index`-written index is read identically by real
  `git ls-files --stage`,
- **real** `git update-index`-written index is read identically by our
  `ls-files --stage`,
- `status --porcelain` is byte-identical to real git for a repo with a
  modified tracked file, an added file, a removed file, and an untracked file.

## Test suite

- 156/156 tests pass (up from 148); zero build warnings.
- `cargo run -p xtask -- scoreboard` now includes the phase6 crosswise suite.

## Deferred / known limitations

- **Cache-tree** (`TREE` extension), **split index** (`link`), **sparse index**
  — not implemented.
- **Version 3/4** index files — rejected on read (not written).
- **Racy-clean / stat-dirty** handling — `status` compares content, which is
  correct but slower.
- **checkout / checkout-index / reset** — not implemented.
- **`diff-files` / `diff-index`** — not implemented (status covers the common
  worktree-vs-index case).
- **`update-index`** lacks `--cacheinfo`, `--refresh`, `--assume-unchanged`,
  `--skip-worktree`, `--verbose`, `-z`.
- **`status`** lacks long format, `--branch`, `--short`, `-z`, renamed
  detection, and `--ignored`; untracked walk is simple (no `.gitignore`).