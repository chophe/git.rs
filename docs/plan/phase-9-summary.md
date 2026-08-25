# Phase 9 — Summary

Status: **partial** (`git fsck` done and cross-verified; sha1↔sha256 object
conversion and the LMAP loose-object map deferred).

## What was implemented

### `git-odb`
- `LooseStore::iter_oids()` — enumerate the object ids of all loose objects on
  disk (fanout-directory walk), powering fsck's dangling scan.

### Commands (`git-command`)
- `git fsck` — object database verification:
  - walks all objects reachable from every ref + `HEAD` (commits → trees →
    blobs; tags → targets; gitlinks skipped),
  - validates each reached object parses (commit/tree/tag structure),
  - reports `missing <type> <oid>` for referenced-but-absent objects and
    `error:` lines for corrupt objects,
  - reports `dangling <type> <oid>` (sorted by type then oid) for present but
    unreachable objects across loose + packs,
  - exits 0 when clean, 2 when missing/corrupt objects were found (matching
    `git fsck`'s exit code).

## Verified against real git (automated)

`git-command/tests/phase9_crosswise.rs`: on a real repo — clean, dangling-blob,
and deleted-referenced-object cases — our `fsck` output and exit codes are
identical to system git (including the exit-2 behavior for missing objects).

## Test suite

- 170/170 tests pass (up from 169); zero build warnings.
- `cargo run -p xtask -- scoreboard` now includes the phase9 crosswise suite.

## Deferred / known limitations

- **sha1↔sha256 object conversion** (`compatObjectFormat`), signed-content
  (`gpgsig`↔`gpgsig-sha256`) rewriting, and the **LMAP** loose-object map —
  not started (t1016-compatObjectFormat gate remains open).
- **`git fsck`** options (`--strict`, `--connectivity-only`, `--no-dangling`,
  `--full`, `--lost-found`, `--name-objects`) — not implemented.
- **`git repack` / `git gc`**, `hash-object --literally`, `index-pack --stdin`
  — not implemented.
- fsck message catalog parity for tooling that parses `git fsck` output
  (`error:`/`warning:` wording beyond the covered cases).

## Status of the core-object-layer roadmap

Phases 0–9 are now each at least partially implemented with crosswise
verification against real git; the full `t/`-suite scoreboard still needs a C
`test-tool` binary (see `docs/plan/test-infrastructure.md`). Remaining work per
phase is tracked in `docs/plan/FOLLOWUPS.md`.