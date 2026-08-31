# Follow-up Work Items

Actionable backlog for the git.rs port. Each item is tied to a plan doc /
module so it can be picked up without re-deriving context. Priorities are
relative within each section. Items marked **DONE** are implemented and
tested.

## A. Deferred implementation (code)

1. **DONE (A1)** — Collision-detecting SHA-1 (sha1dc) — `crates/git-hash`
   - Pure-Rust port of the vendored `sha1dc/` algorithm in
     `git-hash/src/sha1dc.rs`; wired behind `CryptoHasher::Sha1`;
     `is_safe()` now returns true for SHA-1.
   - Collision-block test mirrors `t/t0013-sha1dc.sh` (SHAttered PDF is
     detected, digest `38762cf7...` reported like C git); detection threads
     through `hash-object` and loose-object writes.

2. **`git index-pack` command** — **DONE** (`git-command/src/index_pack.rs`)
   - Builds an `.idx` from a `.pack` (entry walk + `write_idx`); `--verify`
     cross-checks an existing idx. Crosswise: real `git index-pack --verify`
     accepts our idx and our `verify-pack` verifies real git's idx
     (`followups_crosswise.rs`).

3. **`git cat-file --batch` / `--batch-check`** — **DONE** (`cat_file.rs`)
   - Reads names from stdin (refs and oids both resolve), echoes the resolved
     oid, `missing` for absent objects. Crosswise-verified.

4. **Pack delta compression in `pack-objects`** — **NOT DONE**
   - `write_pack` still stores objects non-deltified.

5. **DONE (A10, re-verified)** — `git count-objects -v` size fields and
   garbage semantics (fanout cruft + pack-dir pairing, `st_size`-based
   size-garbage, `-H` human-readable, plain-mode output) are byte-identical
   to C git per `phaseA10_crosswise.rs`.
   - `size` = loose `st_blocks*512/1024`; `size-pack` = `(.pack+.idx bytes)/1024`;
     `prune-packable` (loose also in a pack); `garbage`/`size-garbage`
     (unrecognized files in `objects/pack`). Crosswise-verified.

6. **Local-timezone / calendar parity in `git-date`** — **NOT DONE** (see C).
7. **Ident offset uses UTC** — **NOT DONE** (depends on A6).

8. **`merge-base --is-ancestor`** — **DONE** (`merge_base.rs`), crosswise-verified
   (exit-code parity; fixed the reachability direction: A is reachable from B).

9. **`rev-parse --short` / `--abbrev-ref`** — **DONE** (`rev_parse.rs`),
   crosswise-verified (7-char abbrev matches git on small repos).

10. **`diff --numstat`** — **DONE** (`diff.rs` + `patch::change_line_counts`),
    crosswise-verified.

11. **`status --short`** — **DONE** (`status.rs`; also fixed entry ordering:
    git merges index + untracked into one path-sorted list).

12. **`branch` / `tag` create + delete** — **DONE** (`show_ref.rs`):
    `git branch <name>|-d <name>` (refuses deleting the checked-out branch),
    `git tag <name> [<oid>]|-d <name>` (lightweight). Crosswise-verified.

13. **`cat-file --batch` input via refs** — DONE as part of A3.

14. **DONE (A8)** — Diff/patch engine completion (`git diff`, `diff-tree`,
    `patch`). Implemented in `git-command/src/{diff,diff_tree,patch}.rs` +
    `git-diff/src/{unified,tree}.rs`:
    - `-U<n>` context, `\ No newline at end of file`, section-context in
      `@@` headers, zero-count hunk ranges; `--stat`/`--shortstat`/
      `--numstat`/`--name-only`/`--name-status`/`--raw`/`--summary`/
      `--patch-with-stat`, `-s`, `--cached/--staged`,
      `-M/--find-renames[=n]`, `--no-renames`, `--diff-filter=`,
      `--exit-code/--quiet`, `--no-index`, worktree + index + HEAD/tree
      sources, rename detection (exact + line-similarity), path limiting.
    - `diff-tree` gains `--exit-code`/`--quiet` and resolves commit-ish
      arguments to trees.
    - Crosswise suite `phaseA08_crosswise.rs` (registered `phaseA08-crosswise`),
      byte-identical vs system git.
    - **Deferred for A8**: word-diff (`--word-diff`), `--color`, `--patience`/
      `--histogram` algorithms, `--dirstat`, whitespace family (`-w`/`-b`/
      `--ignore-blank-lines`), `--relative`, `-S`/`-G` pickaxe, and stat width
      hard-coded to 80 columns (like C git on a non-tty).
    - **Known deviations**: exact rename scoring is a line-similarity
      approximation vs C's `diffcore-rename` for post-edit renames;
      `--diff-filter` lowercase is implemented as exclude (C's lowercase
      is undocumented/unsupported); `status` field for `T` (typechange)
      is classified from mode changes.

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
- **DONE (A2)** — `git-command` uses `std::env::set_current_dir`-free design;
  a shared `RepoContext` threads `--git-dir`/`--work-tree`/`--bare`/`-C`/`-c`
  CLI overrides and `GIT_*` env vars through every command
  (see `phase-a/02-repo-discovery-env.md` and `phase-a/PROGRESS.md`).
- **DONE (A4)** — `commit-tree` now resolves tree/parent arguments through
  the shared `git_revision::Resolver` (abbreviated oids, `~`/`^` peels,
  `<rev>:<path>`), with C git's ambiguity diagnostics.

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

- **Phase 9 (partially done)** — fsck. Summary: `docs/plan/phase-9-summary.md`.
  Implemented: `git fsck` (reachability walk, missing/corrupt reporting,
  dangling scan, exit-code parity) + `LooseStore::iter_oids`. Cross-verified.
  Remaining Phase 9:
  - sha1↔sha256 object conversion (`compatObjectFormat`), signed-content
    (`gpgsig`↔`gpgsig-sha256`) rewriting, and the **LMAP** loose-object map
    (gate: `t1016-compatObjectFormat`).
  - `fsck` options (`--strict`, `--connectivity-only`, `--no-dangling`,
    `--full`, `--lost-found`); fsck message-catalog parity.
  - `repack`/`gc`, `hash-object --literally`, `index-pack --stdin`.

- **Phase 10+ stretch (partially done)** — `git apply`. Summary:
  `docs/plan/phase-10-summary.md`. Implemented: `git apply [--check] [-p<n>]
  [--stat]` (unified-diff parsing, hunk application with context verification,
  new/deleted files). Cross-verified against real git. Remaining stretch:
  - `git am`, `format-patch`; 3-way apply (`--3way`), `--index`, `--reject`,
    whitespace options; `\ No newline at end of file`; binary patches.
  - Submodules, notes, blame/line-log, attributes + clean/smudge filters,
    stash, bisect.
  - Network/transport (git://, ssh, smart HTTP, protocol v2, fetch/push/clone,
    credential helpers, daemon, `gc`/`maintenance`).

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
