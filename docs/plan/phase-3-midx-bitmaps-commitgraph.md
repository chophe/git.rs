# Phase 3 — MIDX, Bitmaps, Commit-graph, Cruft

## Goal

Read/write the multi-pack index, pack bitmaps (with pseudo-merge), pack-mtimes,
cruft packs, and the commit-graph with bloom filters and generation numbers.
These make large-repo reachability, `rev-list --objects`, and `gc` feasible and
are required for Phase 4's revision walk to be correct at scale.

## Module → C-source mapping

| Rust crate/module | C reference |
|---|---|
| `git-odb::midx` — `Midx` read/write | `midx.c`, `midx-write.c` |
| `git-odb::bitmap` — `PackBitmap`, `MidxBitmap`, `PseudoMerge` | `pack-bitmap.c`, `pack-bitmap-write.c`, `pseudo-merge.c` |
| `git-odb::bitmap` — EWAH/`BITMAP` extension | `ewah/`, `pack-bitmap.h` |
| `git-odb::pack` — `PackMtimes`, `CruftPack` | `pack-mtimes.c`, `repack-cruft.c`, `repack-midx.c` |
| `git-commitgraph` — `ChunkFormat`, `CommitGraph` (read/write) | `commit-graph.c`, `chunk-format.c`, `csum-file.c` |
| `git-commitgraph` — `Bloom`, generation numbers (corrected commit dates) | `bloom.c`, `commit-graph.h` |

## On-disk formats / surfaces

- MIDX: magic `MIDX`, version, `OIDF`/`OIDL` chunk layout, oid fanout, pack
  entries, checksum trailer.
- Pack bitmaps: header, RLE+EWAH-compressed bitmap chunks, optional
  `BITMAP`/`EOI`/`OOI` extensions; MIDX bitmaps; pseudo-merge bitmaps.
- Pack-mtimes (`MTIMES`) and cruft packs (`.mtimes`, cruft idx).
- Commit-graph: `CDAT` (generation + corrected commit date), `OIDF`/`OIDL`,
  bloom `BIDX`/`BDAT`, `GDAT`/`GDA2`, trailer; `GENERATION_NUMBER_MAX`
  semantics.

## Commands enabled

- `git rev-list --objects --use-bitmap-index`
- `git commit-graph write/verify`
- `git multi-pack-index write/verify`
- `git repack` (cruft-aware), `git gc` (write side)

## Fully automated test plan

### Unit
- **Chunk-format:** TOC parse, chunk offsets/sizes, missing-chunk handling.
- **MIDX:** header/fanout/entries/`OIDF`/`OIDL`, cross-pack object lookup,
  checksum verify.
- **Commit-graph:** parse, generation-number computation (v1 + corrected
  commit dates), `GENERATION_NUMBER_MAX`/overflow handling, bloom filter
  build/query with the documented false-positive tolerance.
- **Bitmaps:** RLE+EWAH decode, chunk traversal, `use-bitmap-index` object
  enumeration matches walking the same graph.
- **Pseudo-merge:** generation and query against the same commit set.
- **Pack-mtimes/cruft:** parse `.mtimes`, cruft idx semantics (recently-reachable
  vs unreachable).

### Property
- Random commit DAGs → Rust-written commit-graph → C `git commit-graph verify`
  passes; C-written → Rust reads with identical reachability answers.
- Random multi-pack setups → Rust-written MIDX → C
  `git multi-pack-index verify` passes; C-written → Rust reads identical.
- `rev-list --objects` with vs without `--use-bitmap-index`: identical output.

### Differential
- `git rev-list --objects --use-bitmap-index` Rust == C on fixture repos.
- `git commit-graph verify` / `git multi-pack-index verify` crosswise.
- Bloom `--changed-paths` filters behave identically (same commits pruned).

### Fuzz
- MIDX, commit-graph, bitmap, pack-mtimes parsers; corpus from `t5319`,
  `t5318`, `t5326`, `t5333` fixtures.

### `git-test` additions
- `test-read-midx`, `test-read-graph`, `test-bloom`.

## Gate criteria

- `cargo xtask test`, differential, scoreboard all green.
- Coverage ≥ 90% on git-commitgraph and the midx/bitmap modules.
- **t/ scripts pass 100%:** `t5318`, `t5319`, `t5325`–`t5329`, `t5330`–`t5335`,
  `t5310`–`t5312`, `t5326`, `t5327`.
- Crosswise `commit-graph verify` / `multi-pack-index verify` green.

## Risks

- Corrected-commit-date generation numbers and their use in reachability are
  subtle; mismatch causes wrong `rev-list` results (caught by differential +
  property tests).
- Bitmap semantics differ between pack bitmaps and MIDX bitmaps (reuse, lazy
  loading, missing chunks).
- Bloom false-positive tolerance must be compatible or `--changed-paths`
  answers diverge from C.
