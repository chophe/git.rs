# Follow-up Work Items

Actionable backlog for the git.rs port. Each item is tied to a plan doc /
module so it can be picked up without re-deriving context. Priorities are
relative within each section. Items marked **DONE** are implemented and
tested.

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

1. **Differential harness** — **DONE** (`cargo run -p xtask -- differential`).
   Runs the crosswise suites in `git-odb/tests/` (`pack_crosswise`,
   `graph_midx_crosswise`) against the system git.
2. **t/ scoreboard** — **PARTIAL**. `cargo run -p xtask -- scoreboard` runs the
   differential suites and updates `crates/scoreboard.json`, failing on
   regression. Running the real `t/` suite additionally needs a C `test-tool`
   binary (not shipped by system git); `scripts/shim-git` is ready and
   dispatches ported commands to the Rust binary.
3. **`git-test` crate** (Rust `test-tool` replacement) — **NOT DONE**.
   - Port the subcommands the core tests use:
     `test-sha1`, `test-sha256`, `test-date`, `test-config`, `test-varint`,
     `test-zlib`, `test-delta`, `test-pack-deltas`, `test-find-pack`,
     `test-read-midx`, `test-read-graph`, `test-bloom`, `test-revision-walking`,
     `test-reach`, `test-read-cache`, `test-write-cache`, `test-dump-cache-tree`,
     `test-dump-split-index`, `test-ref-store`, `test-reftable`, `test-wildmatch`.
4. **Fuzz targets** (`cargo-fuzz`) — **NOT DONE**. pack, idx, midx, commit-graph,
   bitmap, index, config, reftable, loose-object header, xdiff.
   - Seed corpora from `t/t5302`, `t/t5303`, `t/t5313` and `tests/fixtures`.
5. **CI workflow** — **DONE** (`.github/workflows/rust-port.yml`): unit+clippy,
   differential, scoreboard jobs.
6. **Golden fixtures + `cargo xtask gen-fixtures`** — **DONE**.
   `cargo run -p xtask -- gen-fixtures` writes `crates/tests/fixtures`
   (golden repo, `.checksums`) with the pinned system git.
7. **Coverage gate** — **NOT DONE**. `cargo llvm-cov --fail-under-lines 90`
   on core crates; add a CI job.
8. **proptest** — **DONE**. Property tests in git-varint, git-hash, git-config,
   git-odb (round-trips, no-panic, incremental==oneshot, pack round-trip).

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

## D. Phase status

- **Phase 3 (partially done)** — `git-commitgraph` (chunk-format,
  commit-graph read/verify, bloom parse) and `git-odb::midx` (read/verify/write)
  are implemented and cross-verified with real git. Summary:
  `docs/plan/phase-3-summary.md`. Remaining Phase 3 work:
  - `git commit-graph write` (needs commit parsing/walking, Phase 4).
  - Bloom **query** (changed-path hashing) — parse/verify only so far.
  - Pack bitmaps (`pack-bitmap` / MIDX bitmap, EWAH) — not started.
  - Cruft packs / pack-mtimes — not started.
  - Commit-graph **chains** (base graphs) — rejected at parse for now.
  - Incremental MIDX chains — rejected (base-layer count must be 0).
  - MIDX optional chunks `RIDX` (revindex), `BTMP` (bitmapped packs),
    `BASE` — not read/written.
  - MIDX `--preferred-pack` selection — not implemented (first sorted pack wins).

- **Phase 4 (partially done)** — object model + basic revision walking.
  Summary: `docs/plan/phase-4-summary.md`. Implemented: `git-object` tree/
  commit/tag parsing, `git-revision` RevWalk, and commands `cat-file`,
  `ls-tree`, `mktree`, `rev-list`, `log`. Cross-verified. Remaining Phase 4:
  - `HEAD`/ref resolution for command args (needs refs, Phase 7).
  - Pretty-printing (`pretty.c`), `--format`, trailers, mailmap.
  - Revision ordering for merges (`--topo-order`/`--date-order`), path
    limiting, `--first-parent`, grafts/replace, `--all`, `rev-list --objects`
    / `--count` / `-n`.
  - `log` default `Date:` line (author-date parsing).
  - Commit-graph-driven walks + bloom query.

- **Phase 5 (partially done)** — diff. Summary: `docs/plan/phase-5-summary.md`.
  Implemented: `git-diff` (Myers, unified renderer, tree comparison) and
  commands `diff-tree`, `diff`, `diff --no-index`. Cross-verified byte-identical.
  Remaining Phase 5:
  - Rename/copy detection (`diffcore-rename`), pickaxe, break/order/rotate.
  - Output formats: `--stat`, `--numstat`, `--summary`, `--word-diff`, color,
    `--diff-filter`.
  - Function-context hunk headers (userdiff drivers) — unified output matches
    git only for text without them.
  - `git apply`/`am` (Phase 10); working-tree diff (`diff-files`/`diff-index`,
    Phase 6); binary files; no-newline-at-EOF markers.
  - `diff-tree --exit-code` (always exits 0 currently).

- **Phase 6 (partially done)** — index & status. Summary:
  `docs/plan/phase-6-summary.md`. Implemented: `git-index` (v2 read/write) and
  commands `ls-files`, `update-index --add|--remove`, `status --porcelain`.
  Cross-verified both index directions + status. Remaining Phase 6:
  - Cache-tree (`TREE` ext), split index, sparse index; index versions 3/4.
  - checkout / checkout-index / reset; `diff-files` / `diff-index`.
  - Racy-clean / stat-dirty handling (status content-compares today).
  - `update-index --cacheinfo/--refresh/--assume-unchanged/--skip-worktree/-z`.
  - `status` long format, `--branch`, `--short`, `-z`, renames, `--ignored`,
    `.gitignore`.

- **Phase 7 (partially done)** — refs. Summary: `docs/plan/phase-7-summary.md`.
  Implemented: `git-refs` (files backend, packed-refs read) and commands
  `rev-parse`, `show-ref`, `for-each-ref`, `update-ref`, `symbolic-ref`,
  `branch`, `tag -l`; ref names now accepted by `rev-list`/`log` (via
  `resolve_arg`). Cross-verified. Remaining Phase 7:
  - Reflog (`logs/<ref>`), reftable backend, packed-refs writing.
  - Ref transactions / locking (multi-ref atomicity, `lock-ref`).
  - Symref writes; worktree-specific refs.
  - `rev-parse` options (`--abbrev-ref`, `--short`, `@{...}`, ranges `A..B`).
  - `for-each-ref` extra format specifiers, sorting, filtering.
  - `update-ref --stdin`, `--no-deref`, reflog options.

- **Phase 8 (partially done)** — merge & reachability. Summary:
  `docs/plan/phase-8-summary.md`. Implemented: `git-merge` (merge-bases,
  3-way line merge) and commands `merge-base`, `merge-file`. Cross-verified.
  Remaining Phase 8:
  - `git merge` (merge-ort): rename detection, dir/file conflicts, recursive
    criss-cross bases, index merging.
  - `merge-tree`, `cherry-pick`/`revert`, `merge-base --octopus` /
    `--independent` / `--is-ancestor`.
  - Conflict clustering (adjacent changes as conflicts, matching xdl_merge).
  - `merge-file --diff3`, `-L` labels, `--marker-size`; binary/mode/symlink
    merges.

## E. Next phases

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
