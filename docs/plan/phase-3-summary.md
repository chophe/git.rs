# Phase 3 — Summary

Status: **partial** (commit-graph + multi-pack-index complete and cross-verified;
pack bitmaps, cruft packs, and commit-graph write are deferred).

## What was implemented

### `git-commitgraph` crate (new)
- `chunk_format.rs` — the shared chunk-file format (`chunk-format.c` port):
  table of contents `(chunk_id u32, offset u64)`, trailing terminator, trailing
  checksum, alignment enforcement.
- `commit_graph.rs` — commit-graph read + verify (`commit-graph.c` reader):
  header (`CGPH`, version, hash version, chunk count, base count), chunk
  validation, OID fanout/lookup, `CDAT` decode (tree, parents, topo level,
  34-bit commit date), generation v2 / corrected-commit-dates (with `GDO2`
  overflow), structural `verify()` (checksums, oid ordering, parent ranges).
- `bloom.rs` — `BDAT`/`BIDX` parse + validation (version, hash-count field,
  per-commit indexes, monotonicity). **Query is not implemented.**

### `git-odb::pack::midx`
- `midx.rs` — multi-pack-index read/verify/write:
  - parse: header (`MIDX`, version, hash version, chunk count, pack count),
    `PNAM`/`OIDF`/`OIDL`/`OOFF`/`LOFF`, oid lookup → `(pack_int_id, offset)`.
  - `verify()`: checksum (at parse), oid ordering, pack-id ranges.
  - `write_from_indexes()`: deduplicates objects across packs, sorts packs by
    name, emits the full chunk layout with `PNAM` padding and large-offset table.
- **Format detail learned by byte-diffing real git:** `PNAM` stores the pack
  file name **with the `.idx` extension** (e.g. `pack-<hex>.idx`), and the
  `PNAM` chunk size includes the 4-byte alignment padding.

### Commands (`git-command`)
- `git multi-pack-index write` / `verify`
- `git commit-graph verify`

## Verified against real git (both directions, automated)

- `git-odb/tests/graph_midx_crosswise.rs` (3 tests):
  - we `verify()` real git's commit-graph (incl. `--changed-paths` bloom graph),
  - we `verify()` real git's midx and every object is findable,
  - real `git multi-pack-index verify` accepts our written midx.
- Existing `pack_crosswise.rs` still green.

## Test suite

- 105/105 tests pass (up from 86); zero build warnings.
- proptest property tests added in git-varint/hash/config/odb — caught a real
  config-parser panic (non-char-boundary slice), now fixed.

## Deferred (see docs/plan/FOLLOWUPS.md)

- `git commit-graph write` (needs commit parsing/walking → Phase 4).
- Bloom changed-path **query** (hashing).
- Pack bitmaps (`pack-bitmap`/MIDX bitmap, EWAH) — not started.
- Cruft packs / pack-mtimes — not started.
- Commit-graph **chains** (base graphs) and incremental MIDX chains — rejected
  at parse (base count must be 0).
- MIDX optional chunks `RIDX`, `BTMP`, `BASE` — not read/written.
