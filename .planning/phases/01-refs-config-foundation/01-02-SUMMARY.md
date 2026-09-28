---
phase: 01-refs-config-foundation
plan: 02
subsystem: config
tags: [config, scopes, includeIf, wildmatch, file-editing, multivar, git-config, git-core]

# Dependency graph
requires: []
provides:
  - C-order scope loader (`ConfigSet::load_repo_scopes`: system → XDG → global → local → gated worktree, env overrides honored)
  - Six-condition `includeIf` engine with wildmatch, tilde/`./` resolution, depth-10 C-exact die, remote-URL pre-pass + forbid path
  - Layout-preserving scope-file editor (`git-config::file`: set/add/replace-all/unset-all/rename-section/remove-section, dot-lock atomic write)
  - Shared `--type=` canonicalizer (bool/int/path/expiry-date/bool-or-int) with C-exact error texts
  - `Repository::discover_from` loading full scopes through the include-resolving path
  - `scope_include.rs` integration target (9 tests) + 4 new property tests
affects: [01-04, config-command, crosswise-parity]

# Actuals (#2632)
actuals:
  tokens: 27800
  tasks: 3
  commits: 3

# Tech tracking
tech-stack:
  added: []
  patterns: [c-text-by-probe, single-writer-helpers, path-based-cycle-detection, pre-pass-remote-collection]

key-files:
  created:
    - crates/git-config/src/file.rs
    - crates/git-config/tests/scope_include.rs
  modified:
    - crates/git-config/src/lib.rs
    - crates/git-core/src/lib.rs

key-decisions:
  - "Worktree file applies only with extensions.worktreeConfig AND an explicit core.repositoryformatversion (probed: flag without version key is ignored)"
  - "GIT_CONFIG_COUNT/-c overlays stay in RepoContext::repository() (layering: git-core cannot see -c pairs); end-to-end order still files then COUNT then -c, matching the C sequence"
  - "hasconfig: remote URLs pre-collected across all scope files before the main load, so conditions match remotes defined later (t/t1300 first test); forbid enforced on hasconfig-reachable subtrees in the same pass"
  - "Cycle error (canonical-path repeat) kept distinct from the depth-10 C-exact die per plan acceptance; diamonds re-process via pop-on-exit like C"
  - "Value patterns use a REG_EXTENDED subset engine (no new deps allowed); malformed { intervals fall back to literal like glibc, n<m is invalid"
  - "Bool-or-int int branch parses as i32 (C formats %d) while --type=int parses as i64"

patterns-established:
  - "C-text-by-probe: every user-facing error string was captured from the tree binary (2.55.0.552) before encoding — depth die, forbid text, bad-line, rewrite bytes, append positions"
  - "No production unwrap() in new code (swap_remove / map_or instead)"

requirements-completed: [CONF-01]

# Coverage metadata (#1602)
coverage:
  - id: D1
    description: "Scope loader layers system, XDG, global, local, gated worktree with C precedence; CLI overlays win"
    requirement: "CONF-01"
    verification:
      - kind: integration
        ref: "crates/git-config/tests/scope_include.rs#scope_precedence_local_beats_global_beats_system_with_cli_last"
        status: pass
      - kind: unit
        ref: "cargo test -p git-config scope + cargo test -p git-core"
        status: pass
    human_judgment: false
  - id: D2
    description: "All six includeIf conditions behave per C; relative includes resolve from the including file"
    requirement: "CONF-01"
    verification:
      - kind: integration
        ref: "crates/git-config/tests/scope_include.rs#includeif_matrix_through_discovery"
        status: pass
      - kind: unit
        ref: "cargo test -p git-config include (15 tests: gitdir/onbranch/hasconfig/worktree/unknown)"
        status: pass
    human_judgment: false
  - id: D3
    description: "Depth overflow and cycles fail distinctly and C-exactly (probed text)"
    requirement: "CONF-01"
    verification:
      - kind: unit
        ref: "git-config#include_depth_overflow_dies_c_exactly + include_direct_cycle_reports_cycle_error"
        status: pass
      - kind: other
        ref: "tree-binary probe: A↔B cycle and 11-deep chain both die with the exact advice text, exit 128"
        status: pass
    human_judgment: false
  - id: D4
    description: "Writes preserve layout byte-for-byte outside edited lines; single-hunk set diff"
    requirement: "CONF-01"
    verification:
      - kind: integration
        ref: "crates/git-config/tests/scope_include.rs#layout_set_changes_only_that_line"
        status: pass
      - kind: other
        ref: "tree-binary probe: edited line becomes tab-indented, trailing comment dropped, rest identical"
        status: pass
    human_judgment: false
  - id: D5
    description: "Multivar ops and typed canonicalization match C (incl. k/m/g suffixes, base-0 ints, error texts)"
    requirement: "CONF-01"
    verification:
      - kind: integration
        ref: "crates/git-config/tests/scope_include.rs#multivar_add_replace_all_unset_all + typed_canonicalizer_matches_c"
        status: pass
    human_judgment: false
  - id: D6
    description: "Byte-identical config command surface vs C git (crosswise)"
    requirement: "CONF-01"
    verification: []
    human_judgment: true
    rationale: "The config command surface is plan 01-04's scope; this plan built the storage engine only. Crosswise parity deferred per plan verification section."

# Metrics
duration: 1h 46m
completed: 2026-09-28
status: complete
---

# Phase 01 Plan 02: Config Storage Summary

**C-faithful config storage engine: scope layering with probed worktree gating, six-condition includeIf with wildmatch, layout-preserving file editor with multivar ops, and a shared typed canonicalizer**

## Performance

- **Duration:** 1h 46m
- **Started:** 2026-09-28T14:33:36Z
- **Completed:** 2026-09-28T16:19:59Z
- **Tasks:** 3
- **Files modified:** 4 (2 created, 2 modified)

## Accomplishments

- `ConfigSet::load_repo_scopes` layers system → XDG → global → local → worktree in C order, honoring `GIT_CONFIG_NOSYSTEM`/`GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`; missing scopes are empty, corrupt ones die `BadLine` (fatal 128)
- Six `includeIf` conditions (`gitdir`, `gitdir/i`, `onbranch`, `hasconfig:remote.*.url:`, `worktree`, `worktree/i:`) with unknown keywords silently false; hand-ported wildmatch (`WM_PATHNAME` + casefold + classes) covers every `t/t1305` + `t/t1300` pattern shape
- Depth cap 10 with the probed C-exact die, distinct from the canonical-path cycle error; diamond includes re-process (no false cycles); missing includes skipped like C
- Remote-URL pre-pass: `hasconfig:` sees remotes defined later in the same file and in later scope files; the forbid path dies with C's exact text on smuggled remote URLs (T-01-07)
- `git-config::file` splicer: single-hunk canonical rewrites, append-at-section-end, comment-gated empty-section removal, `rename-section`/`remove-section`, dot-lock atomic replace with contention refusal (T-01-08)
- Shared `canonicalize_typed` for bool/int/path/expiry-date/bool-or-int reusing `parse_bool`/int parsing; base-0 ints with `k`/`m`/`g` suffixes, asymmetric bounds, C-exact bad-value texts
- `Repository::discover_from` loads full scopes (with worktree hint + verbatim git-dir for symlink `gitdir:` patterns) through the include-resolving path
- `scope_include.rs` (9 integration tests) + 4 new property tests (edit round-trip, set idempotence, from_file/matcher never-panic)

## Task Commits

Each task was committed atomically:

1. **Task 1: Scope loader with C precedence plus discovery wiring** - `5a143ef6d3` (feat)
2. **Task 2: Full conditional includeIf plus path resolution plus cycle guard** - `91bfe2576b` (feat)
3. **Task 3: Layout-preserving scope-file editor plus multivar ops plus typed canonicalizer** - `83380fe4fa` (feat)

**Plan metadata:** (this SUMMARY commit, recorded after push)

## Files Created/Modified

- `crates/git-config/src/file.rs` - Layout-preserving editor, multivar ops, REG_EXTENDED-subset matcher, dot-lock atomic writer (T-01-06..09)
- `crates/git-config/src/lib.rs` - Scope loader, six-condition includeIf + wildmatch, typed canonicalizer, `BadValue`/`LockDenied` errors
- `crates/git-config/tests/scope_include.rs` - Scope/include/layout/multivar/canonicalizer/atomic-write integration tests
- `crates/git-core/src/lib.rs` - `discover_from` via scope loader; worktree hint with local bare pre-check; verbatim git-dir threading

## Decisions Made

- **Worktree gate is flag AND explicit version:** probed that `extensions.worktreeConfig=true` without any `core.repositoryformatversion` key ignores `config.worktree` (v0 handling in `setup.c` notwithstanding); the port gates on version-key presence, any value.
- **Overlays stay in `RepoContext::repository()`:** `git-core` cannot see `-c` pairs without signature churn, and moving `GIT_CONFIG_COUNT` parsing would duplicate error paths; end-to-end order (files → COUNT → `-c`) already equals the C sequence, so the split is observationally identical.
- **Pre-pass remote collection:** a single pass cannot see remotes defined after a condition (the `t/t1300` first test); collecting across all scope files first is equivalent to C's lazy `populate_remote_urls` re-read, including forbid coverage.
- **Bool-or-int int branch is i32** (C formats `%d`) while `--type=int` is i64; negative/overflow bounds ported asymmetrically from `git_parse_signed`.
- **REG_EXTENDED subset, std-only** (no-new-deps rule): literals, `.`, `*`/`+`/`?`, `{m,n}`, classes, groups, `|`/`^`/`$`, escapes; malformed `{` falls back to literal (glibc), `n<m` is invalid.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] `parse_bool` is now case-insensitive**
- **Found during:** Task 1 (worktree-gate wiring)
- **Issue:** C `git_parse_maybe_bool` accepts `TRUE`/`Yes`/`OFF`; ours only matched lowercase, so `worktreeConfig = True` would silently disable the gate.
- **Fix:** Lowercase before matching in `parse_bool`.
- **Files modified:** crates/git-config/src/lib.rs
- **Verification:** `bool_parsing_is_case_insensitive_like_c` unit test.
- **Committed in:** 5a143ef6d3 (Task 1 commit)

**2. [Rule 2 - Missing Critical] Parser now accepts `[section]key = value` on one line**
- **Found during:** Task 2 (`t/t1305` writes conditional includes exactly this way)
- **Issue:** The parser dropped everything after `]` on a header line, so `t/t1305`-style includes would silently vanish.
- **Fix:** Parse the remainder as a key-value line (trailing `#`/`;` comments still ignored), probed against the tree binary including comment edges.
- **Files modified:** crates/git-config/src/lib.rs
- **Verification:** `single_line_section_header_with_value` unit test + tree-binary probe.
- **Committed in:** 91bfe2576b (Task 2 commit)

**3. [Rule 1 - Bug] Unterminated quotes now report `BadLine`**
- **Found during:** Task 2 (binary probing)
- **Issue:** The parser returned a bespoke `UnterminatedQuote` text; C dies `bad config line N in file F` (probed both mid-file and header-line forms).
- **Fix:** Map unquote failures to `BadLine{line, file}` in both parse paths.
- **Files modified:** crates/git-config/src/lib.rs
- **Verification:** `unterminated_quote_is_bad_line_like_c` unit test.
- **Committed in:** 91bfe2576b (Task 2 commit)

**4. [Rule 1 - Bug] Diamond includes no longer false-positive as cycles**
- **Found during:** Task 2 (include engine rework)
- **Issue:** The `seen` vector was never popped, so `a → {b, c}`, `b → d`, `c → d` died as a cycle on the second visit to `d`; C re-processes (no memoization).
- **Fix:** Path-based detection (push on entry, pop on exit) in both collection and load passes.
- **Files modified:** crates/git-config/src/lib.rs
- **Verification:** `include_diamond_no_false_cycle` unit test (also asserts C-like double processing).
- **Committed in:** 91bfe2576b (Task 2 commit)

**5. [Rule 1 - Bug] Scope-append insertion point skipped the trailing split artifact**
- **Found during:** Task 3 (integration run)
- **Issue:** Appending to a newline-terminated file inserted after the phantom final element, producing a stray blank line.
- **Fix:** `section_insert_at` inserts before a trailing empty element.
- **Files modified:** crates/git-config/src/file.rs
- **Verification:** `multivar_add_replace_all_unset_all` integration test.
- **Committed in:** 83380fe4fa (Task 3 commit)

---

**Total deviations:** 5 auto-fixed (2 missing-critical, 3 bugs)
**Impact on plan:** All required for C correctness/parity; no scope creep. No architectural changes.

## Issues Encountered

- **Pre-existing `phaseB08_crosswise` failures (out of scope, not fixed):** `clean_modes`, `rm_pathspec_file_formats`, `rm_pathspec_file_errors` fail identically to the 01-01 record — `clean`/`rm` pathspec traversal skew vs system Apple Git 2.50.1 (tree is 2.55). The failure output is a file-listing difference, untouched by this plan's config-only diff. Fixing them means matching the older git — wrong direction.
- **Two test-expectation bugs caught at authoring time (not deviations):** `o.*` as a replace-all matcher also matches `two` (C `regexec` agrees — switched to anchored patterns); direct `wildmatch("FOO/", …)` in the unit matrix skipped the `**` affixes that `prepare_condition_pattern` adds (fixed the test, not the matcher).
- **Known 01-04 follow-ups (documented in code, not deviations):** `BadLine` origin paths are absolute (canonicalized discovery) where C prints CWD-relative `.git/config` — needs path-display normalization in the surface plan for crosswise byte-parity; `-c`/`GIT_CONFIG_COUNT` overlay entries carrying `include.path` are inert (C resolves absolute ones / fatals relative ones) — surface plan to wire or explicitly diverge.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Ready for 01-04 (config command surface): the storage engine covers scopes, includes, writes, multivar, and typing with unit + integration + property coverage; the surface is pure parsing plus rendering on top.
- 01-04 planner inputs: reuse the probe log in this SUMMARY (worktree gate, rewrite bytes, append positions, error texts); close the two known follow-ups above under `t/t1300` + `t/t1308` verification; register the crosswise suite and `reflog|config` shim routing when the surface lands.
- No blockers. CONF-01 storage work is complete; command UX remains.

---
*Phase: 01-refs-config-foundation*
*Completed: 2026-09-28*

## Self-Check: PASSED

- All 4 files exist on disk (verified via `[ -f ]`).
- All 3 task commits exist: `5a143ef6d3`, `91bfe2576b`, `83380fe4fa` (verified via `git log`).
- `cargo test -p git-config` (40 unit + 9 integration) and `cargo test -p git-core` (14) green; `cargo test -p git-refs` (23 + 4) green.
- No stub patterns in new files; no production `unwrap()` in new code (one pre-existing `expect` in `parse_into` untouched).
