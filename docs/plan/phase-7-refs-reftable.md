# Phase 7 — Refs & Reftable

## Goal

Implement the ref store: the files backend (loose refs, packed-refs, reflogs),
ref transactions with atomicity/locking, symbolic refs, namespaces, and the
reftable backend (read/write). This enables `rev-parse`, `update-ref`,
`for-each-ref`, `show-ref`, `branch`, and `tag` plumbing.

## Module → C-source mapping

| Rust crate/module | C reference |
|---|---|
| `git-refs` — `RefStore` (files backend) | `refs.c`, `refs/files-backend.c`, `refs/files-backend.h` |
| `git-refs` — `PackedRefs` | `refs/packed-backend.c`, `packed-refs.c` |
| `git-refs` — `Reflog` | `reflog.c` |
| `git-refs` — `RefTransaction` (locking/atomicity) | `refs.c` (transaction API), `refs/files-backend.c` |
| `git-refs` — symbolic refs, namespaces, worktree refs | `refs.c`, `refs.h`, `refs/refs-internal.h` |
| `git-refs::reftable` — reader/writer (blocks, index, log) | `reftable/*` (~7.1K LOC) |
| `git-refs` — `RefFilter` (sorting, formats) | `ref-filter.c` |

## On-disk formats / surfaces

- Loose refs: `refs/heads/<name>`, `refs/tags/<name>`, `refs/remotes/...`,
  symref files (`ref: <target>`), `HEAD`.
- Packed-refs: `# pack-refs with: peeled fully-peeled sorted`, peeled lines
  `^<oid>`, `refs/` prefix trimming.
- Reflog: `logs/<ref>` lines `<old> <new> <committer> <msg>`.
- Reftable: table format with block index, object section, log records,
  restart points, footer; single-file and stacked (tables + `tables.list`).

## Commands enabled

- `git rev-parse`, `git update-ref`, `git symbolic-ref`, `git show-ref`,
  `git for-each-ref`, `git branch -l/-r`, `git tag -l`, `git reflog`

## Fully automated test plan

### Unit
- **Ref name validation** (exact C rules — `t1402` covers the table).
- **Loose refs:** read/write, symref resolution, `HEAD` handling.
- **Packed-refs:** parse (incl. peeled lines, sorted/unsorted), write,
  packed+loose precedence, `packed-refs` lock/rewrite.
- **Reflog:** parse/write, entries ordering, `@{N}`/`@{date}` lookup rules.
- **Transactions:** locking, atomicity across multiple refs, rollback on
  failure, no partial state (crash-safety via temp-file+rename).
- **Reftable:** block parse, index traversal, log records, write→read
  round-trip, iterator ordering, `tables.list` stacking.

### Property
- Random ref names/updates → transaction replay (Rust `update-ref` sequence)
  results in the same ref state as the equivalent C `git update-ref` sequence.
- Random reftable entry sets → write→read round-trip; Rust-written table read
  by C reftable reader and vice versa.

### Differential
- `for-each-ref` / `show-ref` / `rev-parse` / `update-ref` / `reflog` on
  fixture repos; byte-identical output.
- Crosswise: Rust-written packed-refs/reftable → C `git for-each-ref` +
  `git fsck --refs` reads and verifies.

### `git-test` additions
- `test-ref-store`, `test-reftable`.

## Gate criteria

- `cargo xtask test`, differential, scoreboard all green.
- Coverage ≥ 90% on git-refs.
- **t/ scripts pass 100%:** `t1400`–`t1416` (update-ref, reflog), `t1500`–
  `t1520` (rev-parse), `t3210`/`t3211` (packed-refs), `t6300`/`t6301`
  (for-each-ref), `t0600`–`t0612` (reftable).

## Risks

- Reflog timestamp/identity handling and `@{...}` suffix resolution are subtle;
  differential against C on generated histories.
- Transaction atomicity and lock-file protocol must be byte/behavior
  compatible (other tools also write refs).
- Reftable is a separate, large format — treat it as its own sub-project with
  the `t0600`–`t0612` suite as the gate.
