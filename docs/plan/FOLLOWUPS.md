# Follow-up Work Items

Actionable backlog for the git.rs port. Each item is tied to a plan doc /
module so it can be picked up without re-deriving context. Priorities are
relative within each section.

## A. Deferred implementation (code)

1. **Collision-detecting SHA-1 (sha1dc)** — `crates/git-hash`
   - Today `git-hash` uses a hand-written standard SHA-1; `CryptoHasher::is_safe()`
     returns `false` for SHA-1 (see `git-hash/src/lib.rs`).
   - Port the in-tree `sha1dc/` algorithm to pure Rust behind the existing
     `CryptoHasher::Sha1` variant, then set `is_safe()` true for SHA-1.
   - Gate: `src/hash.rs` had this behavior; add a collision-block test (mirror
     `t/t0013-sha1dc.sh`).
   - Phase 0 doc: `docs/plan/phase-0-foundation.md` ("Risks").

2. **`git index-pack` command** — `crates/git-command`
   - Build an `.idx` from a `.pack` (reuse `git_odb::pack::write_idx` + entry
     resolution, including thin-pack base resolution via the loose store).
   - Support `git index-pack --verify <pack>` (cross-check an existing idx).
   - Gate: our idx must pass real `git index-pack --verify` (pattern already in
     `git-odb/tests/pack_crosswise.rs`).
   - Phase 2 doc: `docs/plan/phase-2-packs-idx.md`.

3. **`git cat-file --batch` / `--batch-check`** — `crates/git-command`
   - Read objects by id from loose + packs via `git_odb::Odb` (already works).
   - Batch protocol: `oid <type> <size>\n`, `oid <type> <size>\0<content>`,
     `missing` lines; `%(objectname) %(objecttype) ...` formats.
   - Gate: `t/t1006-cat-file.sh`.

4. **Pack delta compression in `pack-objects`** — `git-odb::pack::write`
   - `write_pack` currently stores objects non-deltified. Add delta selection
     (similarity scoring like `diffcore-rename`/`pack-objects`) so packs shrink.
   - Packs written must remain verifiable by real `git verify-pack`
     (the crosswise tests already enforce this).
   - Phase 2 doc: `phase-2-packs-idx.md` ("write side" + Risks).

5. **`git count-objects -v` size fields** — `crates/git-command/count_objects.rs`
   - `size`/`size-pack`/`prune-packable`/`garbage` are stubbed to `0`; compute
     real values (`size = loose inodes`, `size-pack = pack bytes`, `garbage`
     = stray files in `objects/`).

6. **Local-timezone / calendar parity in `git-date`** — `crates/git-date`
   - Dates without an explicit offset are treated as UTC; real git uses the
     local timezone. `month`/`year` relative units are approximated (30/365 d).
   - Add a timezone source (offset from the OS) and calendar-aware relative math.
   - Phase 0 doc: `phase-0-foundation.md` (scope note in `git-date`).

7. **Ident offset uses UTC** — `crates/git-command/ident.rs`
   - `now_utc()` writes `+0000`; real git writes the local offset. Reuse the
     timezone work from item A6.

## B. Test infrastructure not yet built

All defined in `docs/plan/test-infrastructure.md`; none implemented yet:

1. **Differential harness** (`tests/differential/`, `cargo xtask differential`)
   - Compare Rust CLI byte-for-byte with a C git build on fixture inputs.
   - Forerunner exists: `git-odb/tests/pack_crosswise.rs` (shells to `/usr/bin/git`).

2. **t/ scoreboard** (`scripts/shim-git`, `cargo xtask scoreboard`, `scoreboard.json`)
   - Shim `git` dispatches ported commands to the Rust binary, others to C git;
     run `t/`, track per-script pass/fail, fail on regression.

3. **`git-test` crate** (Rust `test-tool` replacement) — `crates/git-test/`
   - Port the subcommands the core tests use:
     `test-sha1`, `test-sha256`, `test-date`, `test-config`, `test-varint`,
     `test-zlib`, `test-delta`, `test-pack-deltas`, `test-find-pack`,
     `test-read-midx`, `test-read-graph`, `test-bloom`, `test-revision-walking`,
     `test-reach`, `test-read-cache`, `test-write-cache`, `test-dump-cache-tree`,
     `test-dump-split-index`, `test-ref-store`, `test-reftable`, `test-wildmatch`.

4. **Fuzz targets** (`cargo-fuzz`) — pack, idx, midx, commit-graph, bitmap,
   index, config, reftable, loose-object header, xdiff.
   - Seed corpora from `t/t5302`, `t/t5303`, `t/t5313` and `tests/fixtures`.

5. **CI workflow** — unit, differential, scoreboard, fuzz jobs (per
   `test-infrastructure.md` §9).

6. **Golden fixtures + `cargo xtask gen-fixtures`** — pinned C git version,
   committed fixtures + checksums (`tests/fixtures/.checksums`).

7. **Coverage gate** — `cargo llvm-cov --fail-under-lines 90` on core crates.

8. **proptest** — property tests for hash (random-length differential),
   varint round-trips, config parser, pack parser (no-panic), delta
   apply/generate round-trip. `proptest` is already cached in the registry.

## C. Known deviations to revisit

- **`git-date`**: UTC-only for tz-less inputs; month/year relative approx.
  (see A6).
- **`pack-objects`**: non-deltified packs (see A4).
- **`hash-object`** outside a repo always hashes with SHA-1 (matches git
  default; fine, but confirm `-t`/`--stdin` parity against `t1007`).
- **`git-command`** uses `std::env::set_current_dir`-free design; repo discovery
  always starts at CWD — matches git, but `--git-dir`/`--work-tree` CLI
  overrides are not yet threaded through commands.
- **`commit-tree`** requires full-length object ids for tree/parents (no
  abbreviation resolution yet; that needs `rev-parse`/refs, Phase 7).

## D. Next phases

- **Phase 3 — MIDX, bitmaps, commit-graph, cruft** — `docs/plan/phase-3-*.md`
  (needs `chunk-format`, `pack-bitmap`, `commit-graph`/bloom, generation
  numbers; gate `t5310-t5335`).
- **Phase 4 — Object model & revision walking** — `docs/plan/phase-4-*.md`
  (tree/commit/tag parse, pretty, `rev-list`/`log`; gate `t6000-t6019`).
- **Phase 5 — Diff** — `docs/plan/phase-5-*.md`.
- **Phase 6 — Index & worktree** — `docs/plan/phase-6-*.md`.
- **Phase 7 — Refs & reftable** — `docs/plan/phase-7-*.md`.
- **Phase 8 — Merge & reachability** — `docs/plan/phase-8-*.md`.
- **Phase 9 — Object conversion & fsck** — `docs/plan/phase-9-*.md`.
- The `t/` gate lists for every phase are in the phase docs; the shared
  acceptance criteria are in `docs/plan/README.md` §"Shared acceptance criteria".
