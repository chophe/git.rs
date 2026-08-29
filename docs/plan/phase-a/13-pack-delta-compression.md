# A13 — Pack Delta Compression on Write

Parent plan: [00-overview.md](00-overview.md). Backlog ref: FOLLOWUPS.md §A4.

## Goal

`git-odb`'s pack writer currently stores every object non-deltified. Add
delta selection so written packs are comparable in size to C git's, while
remaining byte-acceptable to C git (`verify-pack`, `fsck`).

## Current Rust state (observed)

- `git-odb/pack` can read packs (idx, delta resolution, revindex) and write
  packs that C git verifies — but every object is stored whole.
- The read side already understands deltas, so only the writer changes.

## C reference

- `builtin/pack-objects.c` (the whole machinery: `find_deltas`,
  try_delta, window/depth limits, `--window`, `--depth`,
  `--delta-base-offset`, write-order heuristics), `diff-delta.c` /
  `patch-delta.c` in xdiff-adjacent code (delta format create/apply),
  `Documentation/gitformat-pack.txt` for the delta encoding (already
  implemented on the read side).
- Gates: `t/t5300-pack-object.sh` (incl. `--window`/`--depth` behaviors),
  `t/t5310` is bitmaps (out of scope).

## Deliverables

1. Delta format *writer* (base → delta instruction stream) with the same
   copy/insert opcode encoding the read side parses.
2. Delta candidate search: object sort keys, sliding window (`--window`,
   default 10), max chain depth (`--depth`, default 50), similarity
   heuristic (C uses the min-size/bounded-attempt strategy in
   `try_delta()`), type + name-hash ordering to maximize base locality.
3. Recursion for tree objects (C deltas trees against trees) and blob
   dedup.
4. `--delta-base-offset` (REF_DELTA vs OFS_DELTA choice) with C's default
   (OFS_DELTA allowed) and `--thin` base handling for future `fetch`/`push`
   (E3/E4) — at minimum, error with C-parity message if a thin base is
   missing.
5. Options on `pack-objects`: `--window`, `--depth`, `--window-memory`,
   `--compression` (zlib level pass-through), `--non-empty`,
   `--revs`/`--stdin` (argument forms needed by `repack` in D6 — accept
   `--revs --stdin` now even if `repack` comes later).

## Sub-tasks (ordered)

1. Delta writer + round-trip proptest (write delta via writer, read via
   existing reader, assert object identity).
2. Window/depth machinery + type/name-hash sort; crosswise: pack the same
   object set with C and Rust, then `git verify-pack -v` both and compare
   object→base graphs (sizes need not be identical — C's heuristics are
   tuned — but every delta must verify).
3. Size sanity gate: on the golden fixtures, Rust pack size ≤ 2× C pack
   size (tune until comfortably under).
4. OFS_DELTA/REF_DELTA option handling.
5. `--revs`/`--stdin` input forms.

## Test gates

- `t/t5300-pack-object.sh` via the shim.
- Existing `pack_crosswise` (C git must still verify what we write) + new
  verify-pack -v crosswise for delta topology.

## Risks / notes

- The name-hash ordering trick (C's `pack_name_hash`) drives most of C's
  delta quality — port it, it's small.
- Performance: delta search is O(n·window); C bounds work via the
  window/depth limits and `try_delta` early-outs — replicate those bounds or
  large repos will hang the port.
