# A10 — `count-objects -v` Close-Out

Parent plan: [00-overview.md](00-overview.md). Backlog ref: FOLLOWUPS.md §A5.

## Goal

Verify and formally close out the `count-objects -v` size fields. **Status
note (observed while preparing this plan)**: `crates/git-command/src/
count_objects.rs` now computes `size`, `size-pack`, `prune-packable`,
`garbage`, and `size-garbage` — FOLLOWUPS §A5 is stale. This item is
therefore a verification-and-close-out task, not an implementation task,
unless the crosswise check finds divergences.

## Current Rust state (observed)

- `-v` output: `count`, `size` (loose on-disk KiB via `st_blocks`),
  `in-pack`, `packs`, `size-pack` (pack+idx bytes / 1024), `prune-packable`
  (loose OIDs also present in packs), `garbage`/`size-garbage` (unrecognized
  files under `objects/pack`).
- Non-verbose mode prints the loose count only.

## C reference

- `builtin/count-objects.c` — check three behaviors the Rust version may not
  match yet:
  1. **Garbage scan scope**: C scans *all* of `.git/objects` (via
     `report_refs`-style iteration) for non-directory, non-fanout files, not
     only `objects/pack`; compare against the Rust `scan_garbage` scope.
  2. **`size` accounting** uses `on_disk_bytes`/KiB rounding identical to
     `st_blocks * 512` — confirm parity on sparse files.
  3. **`--verbose` vs `-v`** and invalid-argument handling (`-H`? there is
     none) — C usage errors exit 129 with its exact message.
- Gate: `t/t1450` exercises `count-objects -v` indirectly; there is no
  dedicated script, so add crosswise cases instead.

## Deliverables

1. A crosswise suite: fixture repo with loose objects, a pack, a
   prune-packable object, and planted garbage files (stray file in
   `objects/`, stray file in `objects/pack/`, bogus fanout dir contents);
   assert byte-identical `-v` output and exit codes.
2. Fix any divergence found (garbage scope is the most likely).
3. Mark FOLLOWUPS §A5 DONE with a pointer to the suite.

## Test gates

- New crosswise suite (suggest `phaseA10_crosswise.rs`); scoreboard
  no-regression.

## Risks / notes

- Garbage definition differs between loose-root and pack-dir scanning in C;
  do not hand-wave — run C git on the fixture and copy its numbers.
