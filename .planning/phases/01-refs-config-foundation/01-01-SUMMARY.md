---
phase: 01-refs-config-foundation
plan: 01
subsystem: refs
tags: [refs, reflog, transactions, packed-refs, file-locking, git-refs, git-command]

# Dependency graph
requires: []
provides:
  - C-exact dot-lock lifecycle (`git-refs::lock::LockFile`)
  - Centralized reflog format/parse/gate/append (`git-refs::reflog`)
  - Atomic packed-refs writer with peeled lines (`git-refs::packed`)
  - Two-phase ref transaction engine (`git-refs::transaction`)
  - `reflog show` builtin wired through dispatch (show-only slice)
  - Wave-0 integration scaffold (`git-refs/tests/ref_tx.rs`)
  - Probe-pinned reflog expire-default constants (30d total / 90d unreachable)
affects: [01-02, 01-03, 01-04, config-scopes, update-ref-stdin, reflog-surface]

# Actuals (#2632)
actuals:
  tokens: 18909
  tasks: 3
  commits: 3

# Tech tracking
tech-stack:
  added: []
  patterns: [storage-logic-in-git-refs-composition-in-git-command, probed-not-assumed-c-texts]

key-files:
  created:
    - crates/git-refs/src/lock.rs
    - crates/git-refs/src/reflog.rs
    - crates/git-refs/src/packed.rs
    - crates/git-refs/src/transaction.rs
    - crates/git-command/src/reflog.rs
    - crates/git-refs/tests/ref_tx.rs
  modified:
    - crates/git-refs/src/lib.rs
    - crates/git-command/src/lib.rs
    - crates/git-command/src/update_ref.rs

key-decisions:
  - "RefStore::update reimplemented on a single-op transaction (deref=false): one error-composition site, zero caller behavior change"
  - "O1 expire defaults resolved empirically: total=30d, unreachable=90d verbatim from REFLOG_EXPIRE_OPTIONS_INIT; unreachable default is masked by the total ceiling"
  - "t/t3210-pack-refs.sh does not exist; t/t0601-reffiles-pack-refs.sh + t/pack-refs-tests.sh recorded as the real packed-refs oracles for the 01-04 gate plan"
  - "reflog ships as show-only slice; six future subcommands return usage errors; no shim-git change until 01-03 owns the full surface"
  - "reflog show resolves the revision before reading the log (unborn ref dies even when a log file exists) — probed C behavior"
  - "Single-delete D/F exit-1 and packed-prune-on-delete deferred to 01-04 with t/t1404 verification"

patterns-established:
  - "C-text-by-probe: every user-facing error string in this plan was captured from the built C binary before being encoded, never transcribed from source alone"
  - "No rename inside validation: Transaction::prepare acquires all locks and validates everything; commit publishes in a separate pass"

requirements-completed: [REFS-01, REFS-02]

# Coverage metadata (#1602)
coverage:
  - id: D1
    description: "C-exact dot-lock lifecycle with contention errors naming the lock file"
    requirement: "REFS-02"
    verification:
      - kind: unit
        ref: "crates/git-refs/src/lock.rs#second_holder_gets_contention_naming_lock"
        status: pass
      - kind: other
        ref: "update-ref contention stderr byte-compared against both system git 2.50.1 and tree binary 2.55, exit 128 both sides"
        status: pass
    human_judgment: false
  - id: D2
    description: "reflog show HEAD byte-identical to C git on a probe repo"
    requirement: "REFS-01"
    verification:
      - kind: other
        ref: "diff of Rust vs C reflog show HEAD and refs/heads/master on identical fixture repos: zero diff lines"
        status: pass
      - kind: unit
        ref: "crates/git-command/src/reflog.rs#show_head_renders_newest_first"
        status: pass
    human_judgment: false
  - id: D3
    description: "Failed multi-ref batches leave every ref byte-unchanged"
    requirement: "REFS-02"
    verification:
      - kind: unit
        ref: "crates/git-refs/src/transaction.rs#bad_old_oid_leaves_everything_byte_identical"
        status: pass
      - kind: integration
        ref: "crates/git-refs/tests/ref_tx.rs#transaction_abort_leaves_set_unchanged"
        status: pass
    human_judgment: false
  - id: D4
    description: "packed-refs written by the port is sorted, carries the peeled header, and is readable by C git"
    requirement: "REFS-02"
    verification:
      - kind: unit
        ref: "crates/git-refs/src/packed.rs#render_matches_c_byte_contract"
        status: pass
      - kind: integration
        ref: "crates/git-refs/tests/ref_tx.rs#packed_write_read_round_trip_with_peeled (shells out to C git show-ref, exit 0)"
        status: pass
    human_judgment: false
  - id: D5
    description: "Reflog expire-default magnitudes pinned to observed C binary behavior (30d total / 90d unreachable)"
    requirement: "REFS-01"
    verification:
      - kind: other
        ref: "expire dry-run verbose probe over 100d/40d/20d reachable-vs-orphan entries, defaults and explicit --expire-unreachable"
        status: pass
    human_judgment: true
    rationale: "The unreachable-default magnitude is behaviorally masked by the total ceiling, so no probe can observe it directly; it is encoded verbatim from reflog.h and the verifier should confirm that reading."

# Metrics
duration: 41min
completed: 2026-09-28
status: complete
---

# Phase 01 Plan 01: Refs Storage Tracer Summary

**End-to-end refs storage slice: C-exact dot-locks, centralized reflog with byte-identical `reflog show`, atomic packed-refs writer, and all-or-nothing transactions — with expire defaults pinned by probing the C binary**

## Performance

- **Duration:** 41 min
- **Started:** 2026-09-28T13:48:27Z
- **Completed:** 2026-09-28T14:29:45Z
- **Tasks:** 3
- **Files modified:** 9 (6 created, 3 modified)

## Accomplishments

- `git-refs::lock::LockFile` owns the full dot-lock lifecycle (`<ref>.lock` via `create_new`, write+fsync, atomic rename, rollback/drop unlink) with repo-escape rejection; contention errors name the lock file and render byte-identical to C (`Unable to create ... File exists` + stale-lock advisory, exit 128)
- `git-refs::reflog` centralizes the line formatter (no tab on empty message), parser, four-state `logallrefupdates` gating with the HEAD/heads/remotes/notes prefix rule, and append-only writer
- `reflog show [<ref>]` (default HEAD) renders newest-first `<abbrev> <ref>@{n}: <msg>` with zero diff lines vs C on fixture repos, including the ambiguous-argument fatal for unresolvable refs
- `git-refs::packed` writes sorted packed-refs under `packed-refs.lock` with the od-verified fixed header (`# pack-refs with: peeled fully-peeled sorted `, trailing space) and `^`-peeled tag continuations; C `show-ref` reads Rust-packed repos cleanly
- `git-refs::transaction` runs op queues (set/create/delete/verify) through prepare (names, duplicates, batch D/F, FS D/F both directions, old-oid/existence/symref rules — zero renames) then commit; `RefStore::update` is a single-op transaction so all error composition lives in one place with the `update_ref failed for ref` wrapper
- `git-refs/tests/ref_tx.rs` Wave-0 scaffold: 4 integration tests (lock, abort-unchanged, gating matrix + append, packed round-trip with C validation)
- O1 resolved: `DEFAULT_EXPIRE_TOTAL_DAYS=30`, `DEFAULT_EXPIRE_UNREACHABLE_DAYS=90` from the C-binary dry-run probe (40d entries prune, 20d kept; explicit `--expire-unreachable` confirms the reachability branch)

## Task Commits

Each task was committed atomically:

1. **Task 1: Tracer lock helper + centralized reflog + read path** - `1eb403f95f` (feat)
2. **Task 2: Packed-refs writer + two-phase transaction engine** - `8d54c5e036` (feat)
3. **Task 3: Wave-0 scaffold + expire-default probe** - `f9524ebb70` (feat)

## Files Created/Modified

- `crates/git-refs/src/lock.rs` - C-exact dot-lock lifecycle + repo-escape guard (T-01-01)
- `crates/git-refs/src/reflog.rs` - Line format/parse, gating enum, append/read, expire-default constants
- `crates/git-refs/src/packed.rs` - Sorted atomic packed-refs rewrite + post-commit loose collapse
- `crates/git-refs/src/transaction.rs` - Op queue, prepare/commit/abort, C-probed error texts
- `crates/git-command/src/reflog.rs` - Show-only `reflog` builtin (full matrix → usage errors until 01-03)
- `crates/git-refs/tests/ref_tx.rs` - Wave-0 integration tests incl. t3210-absent gate note
- `crates/git-refs/src/lib.rs` - Module re-exports, `LockContention`/`Transaction` variants, `update()` on transactions, HEAD/bare-@ /one-level refname rules
- `crates/git-command/src/lib.rs` - `reflog` module + dispatch arm
- `crates/git-command/src/update_ref.rs` - Store errors mapped with the `fatal:` prefix for byte-exact stderr

## Decisions Made

- **Single composition site:** `RefStore::update` queues one op (`deref: false`, preserving literal-path semantics for all existing callers) through `Transaction`, so contention/validation text is composed once and wrapped with C's `update_ref failed for ref` (refs.c:1574, DIE_ON_ERR). Batch callers in 01-04 surface details unwrapped, like C's `--stdin` path.
- **O1 by experiment, not by docs:** the documented 90/30-day family contradicts the header INIT (30/90) and the binary follows the header — 40d-reachable entries prune under defaults. Encoded verbatim with the masking analysis in the constant docs; `t/t1410` never asserts defaults so nothing contradicts the finding.
- **Gate-script correction:** `t/t3210-pack-refs.sh` (named in 01-CONTEXT.md) does not exist in this tree; recorded in `ref_tx.rs` header that `t/t0601-reffiles-pack-refs.sh` + `t/pack-refs-tests.sh` are the real oracles for the 01-04 gate plan.
- **Show-only slice is honest:** `list/exists/write/delete/drop/expire` return exit-129 usage errors naming the gap (owned by 01-03); unknown first words fall through to show-as-revision exactly like C's `cmd_reflog` fall-through (verified: `reflog frobnicate` dies byte-identically). No `shim-git` change — routing `t/` reflog tests at a show-only stub would be worse than not routing.
- **Resolve-then-read:** `show` resolves the revision first: an unborn HEAD dies with the ambiguous-argument fatal even when `logs/HEAD` exists (probed), while a resolving ref with no log prints nothing, exit 0 (probed on tags).
- **Threat mitigations applied (T-01-01..04):** `ensure_within` anchors every lock under the common dir (canonicalized both sides); locks are held from prepare through commit with no rename in the validation loop; reflog messages stay opaque bytes after the tab; foreign locks are never unlinked, only reported. T-01-05 accepted by design (ident parity).

## Deviations from Plan

None - plan executed exactly as written. All C texts were captured from the built tree binary before encoding (lock contention, D/F both directions, batch duplicates, old-oid outcomes, `unable to resolve reference`, show rendering incl. empty-message and error paths).

## Issues Encountered

- **Pre-existing crosswise failures (out of scope, not fixed):** `phaseB08_crosswise` has 3 failures (`clean_modes`, `rm_pathspec_file_formats`, `rm_pathspec_file_errors`) comparing Rust vs system Apple Git 2.50.1 while the tree is 2.55 — version-skew stderr texts in `rm`/`clean` pathspec code this plan never touches (diff scope verified: 9 refs/reflog files only). Fixing them would mean matching the *older* git — the wrong direction; belongs to whoever owns those suites, not this plan.
- **Pre-existing depcheck FAIL (out of scope, not fixed):** `git-odb (layer 2) -> git-compress (layer 4)` upward edge comes from committed commit `1d2254e87f`; this plan adds no new cross-crate edges (git-refs uses git-core/git-hash/std only).
- **Known gaps owned by later plans (documented in code, not deviations):** single-ref `update-ref -d` D/F conflicts surface as fatal/128 here while C's `-d` path reports error/1; deleting a packed-only ref leaves its stale packed entry (loose removal only); `update-ref <ref> <new> [<old>]` 3-arg form still returns usage-129. All three belong to the 01-04 `--stdin`/error-matrix work with `t/t1404` verification.
- **Self-caught during execution:** one bad edit briefly replaced a match scrutinee in `transaction.rs` (caught by immediate re-read, repaired before compiling); one test snapshotted refs before the failed transaction dropped its locks (reordered drop-before-snapshot). Neither reached any commit.

## Expire Probe Log (O1 evidence)

Tree binary `./git` (2.55.0.552) on repos with crafted `logs/HEAD` (entries at 100d/40d/20d, reachable c1 vs orphan `commit-tree` tip, plus fresh tip):

- Defaults: 100d reachable/unreachable prune; 40d reachable/unreachable prune; 20d reachable/unreachable keep; fresh tip kept.
- `--expire-unreachable=<10d-ago>`: 20d-unreachable prunes, 20d-reachable kept (reachability branch confirmed real).
- Conclusion: total window ≈30d (`REFLOG_EXPIRE_OPTIONS_INIT`), unreachable default 90d masked by the total-first check in `should_expire_reflog_ent`.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Ready for 01-02/01-03/01-04: lock, reflog, packed, and transaction primitives are in place with unit + integration coverage; the `reflog`/`update-ref`/`config` surfaces build on them.
- 01-04 planner inputs: use `t/t0601` + `t/pack-refs-tests.sh` (not t3210) for packed-refs gates; close the three known gaps above under `t/t1404`; wire `reflog|config` into `scripts/shim-git` when the surfaces are complete; migrate `checkout_core`'s legacy `.lock.<pid>` temp naming and the three ad-hoc reflog append call sites onto the new single writer.
- No blockers. Pre-existing `phaseB08`/`depcheck` findings are environmental and unrelated.

---
*Phase: 01-refs-config-foundation*
*Completed: 2026-09-28*

## Self-Check: PASSED

- All 6 created files exist on disk (verified via test runs executing them).
- All 3 task commits exist: `1eb403f95f`, `8d54c5e036`, `f9524ebb70` (verified via `git log`).
- `cargo test -p git-refs` (23 unit + 4 integration) and `cargo test -p git-command --lib` (16) green; tracer `reflog show HEAD` diff vs C clean on re-run.
- No stub patterns in new files; no production `unwrap()` outside tests.
