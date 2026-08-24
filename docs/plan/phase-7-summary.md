# Phase 7 — Summary

Status: **partial** (files-backend refs + packed-refs reading done and
cross-verified; reflog, reftable, ref transactions/locking, and packed-refs
writing deferred).

## What was implemented

### `git-refs` crate (new)
- `RefStore` (files backend):
  - `resolve(name)` — resolve a ref to an oid, following `ref: <target>` symrefs
    with a depth limit; checks loose refs (git_dir then common_dir) then
    `packed-refs`.
  - `list()` — merged loose + packed refs (loose overrides packed), sorted by
    refname.
  - `update(name, Some(oid)|None)` — atomic write (temp + rename) or delete,
    with parent-dir creation and ref-name validation.
  - `head_symbolic_target()`, `list_short` helpers.
- `validate_refname` — git's ref-name rules (subset).

### Commands (`git-command`)
- `git rev-parse --verify <ref|oid>` / `--git-dir` / `--git-common-dir`.
- `git show-ref`, `git for-each-ref [pattern]` (default `%(objectname)
  %(objecttype)\t%(refname)`).
- `git update-ref <ref> <oid>` / `-d <ref>`.
- `git symbolic-ref <name>` (`--short`).
- `git branch` (`* current`, 2-space indent) and `git tag -l`.
- `git-command::resolve_arg` — resolves full oids, `HEAD`, and shorthand
  `main`/`v1`; wired into `rev-list` and `log` so they accept ref names.

## Verified against real git (automated)

`git-command/tests/phase7_crosswise.rs` (3 tests):
- `rev-parse --verify`, `show-ref`, `for-each-ref`, `branch`, `tag -l` are
  byte-identical to system git,
- **our** `update-ref`-created ref is read by real `git rev-parse --verify`
  (and deletions are visible too).

Plus manual smoke: `rev-list HEAD`, `log --oneline HEAD`, and `log main`
(shorthand) now match real git.

## Test suite

- 163/163 tests pass (up from 156); zero build warnings.
- `cargo run -p xtask -- scoreboard` now includes the phase7 crosswise suite.

## Deferred / known limitations

- **Reflog** (`logs/<ref>`) — not implemented.
- **Reftable** backend — not started.
- **Packed-refs writing** (`git pack-refs`) — read-only so far.
- **Ref transactions / locking** (`lock-ref` protocol, multi-ref atomicity) —
  `update-ref` writes directly.
- **Symref writes** (`symbolic-ref <name> <target>`) — read-only.
- **`rev-parse`** options (`--abbrev-ref`, `--short`, `@{...}`, `--show-toplevel`
  output, ranges `A..B`) — not implemented.
- **`for-each-ref`** format specifiers beyond the default; sorting, filtering.
- **Worktree-specific refs** (`refs/bisect`, per-worktree `HEAD`) — HEAD handled
  via git_dir, others assumed in common_dir.
- **`update-ref --stdin`, `--no-deref`, reflog options** — not implemented.