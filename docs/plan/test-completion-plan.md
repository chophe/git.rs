# Test Completion Plan

Roadmap from the current partial test state to the full shared acceptance
criteria defined in `docs/plan/README.md` §"Shared acceptance criteria".

## 1. Current state (as of 2026-08-29)

### What exists and is green

| Layer | Status | Detail |
|---|---|---|
| Crosswise suites | **19/19 passing** | `scoreboard.json` all-green; registered in `xtask::suites()` |
| Unit tests | **268 `#[test]`** across 57 files | Inline `#[cfg(test)]` modules in every crate |
| Proptests | **6 crates** | git-varint, git-hash, git-config, git-odb, git-revision, git-pretty |
| xtask commands | **4 of 5** | `test`, `differential`, `gen-fixtures`, `scoreboard` (missing `fuzz`) |
| shim-git | **29 commands routed** | Dispatcher ready for `t/` scoreboard |
| Golden fixtures | **DONE** | `tests/fixtures` + `.checksums` via `cargo xtask gen-fixtures` |
| CI workflow | **DONE** | `.github/workflows/rust-port.yml` (unit, differential, scoreboard) |

### What is broken or missing

| Gap | Severity | Detail |
|---|---|---|
| **Compilation errors** | **BLOCKER** | 4 errors in `git-command` prevent `cargo test --workspace` from running |
| `git-test` crate (test-tool replacement) | High | Needed for real `t/` scoreboard; 20 subcommands listed in test-infrastructure.md |
| Fuzz targets | Medium | `cargo-fuzz` targets not created; `cargo xtask fuzz` not implemented |
| Coverage gate | Medium | `cargo llvm-cov --fail-under-lines 90` not wired |
| Real `t/` scoreboard | High | Current scoreboard runs differential suites only, not the C `t/` scripts via shim |
| Phase A unfinished | Medium | A8 (diff options), A9 (userdiff), A11 (gitignore), A13 (pack delta) planned but not implemented |
| Per-phase remaining work | High | Phases 3–10 all partially done (see FOLLOWUPS.md §D) |

### Compilation errors (immediate blocker)

| File | Line | Error |
|---|---|---|
| `git-command/src/patch.rs` | 315, 349 | Literal newline in char constant — should be `'\n'` not an actual newline |
| `git-command/src/diff.rs` | 135 | `Index::new(algo)` — no `new` constructor; `Index` has `Default` impl, use `Index::default()` |
| `git-command/src/diff.rs` | 308–313 | Borrow checker: `changes.iter_mut()` + `changes.iter()` — need to collect indices first |

---

## 2. Plan — ordered work items

### Phase 0: Fix the build (prerequisite for everything)

| Step | Task | Files |
|---|---|---|
| 0.1 | Fix literal newline chars in `patch.rs` lines 315, 349 | `git-command/src/patch.rs` |
| 0.2 | Replace `Index::new(algo)` with `Index::default()` (or add `new` to `git-index`) | `git-command/src/diff.rs`, `git-index/src/lib.rs` |
| 0.3 | Fix borrow conflict in `diff.rs` rename detection — collect candidate indices before the `iter_mut` loop | `git-command/src/diff.rs` |
| 0.4 | Verify `cargo test --workspace` passes | — |

### Phase 1: Complete test infrastructure

These are the "B" items from FOLLOWUPS.md that are still **NOT DONE**.

#### 1a. `git-test` crate (Rust test-tool replacement)

The `t/` suite calls `test-tool` for low-level primitives. Without a Rust
replacement, the scoreboard can only run differential suites, not the real
`t/` scripts. Build `crates/git-test` with these subcommands in dependency
order:

| Batch | Subcommands | Used by phases |
|---|---|---|
| 1 | `test-sha1`, `test-sha256`, `test-date`, `test-config` | 0 |
| 2 | `test-varint`, `test-zlib`, `test-delta`, `test-pack-deltas` | 1–2 |
| 3 | `test-find-pack`, `test-read-midx`, `test-read-graph`, `test-bloom` | 2–3 |
| 4 | `test-revision-walking`, `test-reach` | 4 |
| 5 | `test-read-cache`, `test-write-cache`, `test-dump-cache-tree`, `test-dump-split-index` | 6 |
| 6 | `test-ref-store`, `test-reftable` | 7 |
| 7 | `test-wildmatch`, `test-path-utils` | 5–6 |

Each subcommand: read the C `t/helper/test-*.c` for exact protocol, port the
I/O loop, add a crosswise test comparing Rust `test-tool` output to C
`test-tool` on the same stdin.

**Gate**: `cargo xtask scoreboard` runs the real `t/` suite through the shim
with `GIT_TEST_INSTALLED` pointing at the Rust binary + `git-test`.

#### 1b. Real `t/` scoreboard

Upgrade `cargo xtask scoreboard` to:
1. Build the Rust binary (`cargo build --workspace`).
2. Set `GIT_TEST_INSTALLED` to the `scripts/shim-git` wrapper.
3. Run `make -C t test` (or selected `t/t*.sh` scripts).
4. Parse `t/test-results/` for per-script pass/fail counts.
5. Write `scoreboard.json` with per-script results + Rust-coverage %.
6. Fail on regression vs committed baseline.

**Gate**: `scoreboard.json` contains real `t/t*.sh` results, not just
differential suite names.

#### 1c. Fuzz targets

Create `crates/*/fuzz/` targets (via `cargo-fuzz`) for each parser:

| Crate | Target | Seed corpus |
|---|---|---|
| git-odb | `fuzz_pack`, `fuzz_idx`, `fuzz_midx` | `t/t5302`, `t/t5303`, `t/t5313` |
| git-commitgraph | `fuzz_commit_graph` | `tests/fixtures` |
| git-index | `fuzz_index` | `t/t1000` outputs |
| git-config | `fuzz_config` | random config files |
| git-refs | `fuzz_reftable` | `t/t0032` fixtures |
| git-object | `fuzz_loose_object` | object headers |
| git-diff | `fuzz_xdiff` | random text pairs |

Add `cargo xtask fuzz` to run each target for a short CI budget.

**Gate**: `cargo xtask fuzz` runs all targets for N minutes without panics.

#### 1d. Coverage gate

1. Add `cargo-llvm-cov` to CI.
2. Run `cargo llvm-cov --workspace --fail-under-lines 90` on core crates.
3. Exclude test code via `--ignore-filename-regex`.
4. Add a CI job that fails if coverage drops below 90%.

**Gate**: CI enforces ≥90% line coverage on all core crates.

---

### Phase 2: Deepen per-phase crosswise tests

Each phase has remaining items (FOLLOWUPS.md §D). For each, add crosswise
tests before implementing the feature (test-first where possible).

#### Phase 3 remaining

| Item | Test to add |
|---|---|
| `commit-graph write` | Crosswise: Rust writes a graph, C `commit-graph verify` passes |
| Bloom query (changed paths) | Crosswise: `test-bloom` subcommand parity |
| Pack bitmaps (EWAH) | Crosswise: `rev-list --objects --use-bitmap-index` parity |
| Cruft packs / pack-mtimes | Crosswise: `pack-objects` with cruft objects |
| Commit-graph chains | Crosswise: chained graph verify + read |
| MIDX optional chunks (RIDX, BTMP, BASE) | Crosswise: MIDX with revindex read/write |

#### Phase 4 remaining

| Item | Test to add |
|---|---|
| `--topo-order` / `--date-order` | Crosswise: merge-fixture rev-list with ordering flags |
| Path limiting | Crosswise: `rev-list -- <path>` on multi-commit repo |
| `--first-parent` | Crosswise: merge-fixture first-parent walk |
| `rev-list --objects --count -n` | Crosswise: count + limit parity |
| `--all` / `--branches` / `--tags` | Crosswise: multi-ref repos |
| Commit-graph-driven walks | Crosswise: graph-accelerated rev-list |
| `log` default `Date:` line | Crosswise: `log` with author-date |

#### Phase 5 remaining

| Item | Test to add |
|---|---|
| Rename/copy detection | Crosswise: `diff --find-renames` on moved-file repo |
| `--stat` / `--shortstat` | Crosswise: diff stat output byte-identical |
| `--word-diff` | Crosswise: word-level diff parity |
| `--diff-filter` | Crosswise: filter by status letter |
| Function-context hunk headers | Crosswise: diff on C/Python/Rust source files |
| `diff-tree --exit-code` | Crosswise: exit code parity on identical/differing trees |
| Binary file diff | Crosswise: binary blob diff output |
| No-newline-at-EOF | Crosswise: diff with missing trailing newline |

#### Phase 6 remaining

| Item | Test to add |
|---|---|
| Cache-tree (`TREE` ext) | Crosswise: `write-tree` + `ls-files` with cache-tree |
| Split index | Crosswise: `test-dump-split-index` parity |
| Index v3/v4 | Crosswise: `update-index --index-version` read/write |
| `checkout` / `reset` | Crosswise: working-tree state after checkout |
| `diff-files` / `diff-index` | Crosswise: worktree-vs-index diff parity |
| `status` long format / `--branch` | Crosswise: `status` full output |
| `.gitignore` | Crosswise: `status --ignored` with ignore patterns |
| `update-index --cacheinfo --refresh` | Crosswise: cacheinfo update parity |

#### Phase 7 remaining

| Item | Test to add |
|---|---|
| Reflog read/write | Crosswise: `reflog` command parity |
| Packed-refs writing | Crosswise: `pack-refs` + read-back |
| Ref transactions / locking | Crosswise: `update-ref --stdin` atomicity |
| Symref writes | Crosswise: `symbolic-ref` write + read-back |
| `rev-parse @{...}` | Crosswise: reflog/shorthand expansion |
| `for-each-ref` format specifiers | Crosswise: `%(upstream)`, `%(objecttype)`, etc. |

#### Phase 8 remaining

| Item | Test to add |
|---|---|
| `git merge` (merge-ort) | Crosswise: 3-way merge with renames |
| `merge-tree` | Crosswise: `merge-tree` output parity |
| `cherry-pick` / `revert` | Crosswise: single-commit cherry-pick |
| `merge-base --octopus` / `--independent` | Crosswise: multi-parent merge-base |
| Conflict clustering | Crosswise: adjacent-change conflict markers |
| `merge-file --diff3` / `-L` labels | Crosswise: diff3-style conflicts |

#### Phase 9 remaining

| Item | Test to add |
|---|---|
| sha1↔sha256 conversion | Crosswise: `t/t1016-compatObjectFormat.sh` via shim |
| `fsck --strict` / `--connectivity-only` | Crosswise: fsck option parity |
| `repack` / `gc` | Crosswise: repack output + fsck after |
| `index-pack --stdin` | Crosswise: pipe pack to index-pack |

#### Phase 10+ remaining

| Item | Test to add |
|---|---|
| `git am` / `format-patch` | Crosswise: apply mailbox patches |
| `git apply --3way` / `--index` | Crosswise: 3-way apply with conflict |
| Binary patches | Crosswise: apply binary patch |
| Submodules | Crosswise: submodule add + status |

---

### Phase 3: Finish Phase A items

Items A8, A9, A11, A13 are planned but not implemented. Each should land with
a crosswise suite.

| Item | Description | Crosswise suite |
|---|---|---|
| A8 | Diff options completion (`--cached`, `--stat`, `-U<n>`, color, `--diff-filter`) | `phaseA08_crosswise.rs` |
| A9 | Userdiff hunk headers (function-context for C/Python/etc.) | `phaseA09_crosswise.rs` |
| A11 | `.gitignore` + attributes engine | `phaseA11_crosswise.rs` |
| A13 | Pack delta compression on write | `phaseA13_crosswise.rs` |

---

### Phase 4: Close known deviations (FOLLOWUPS.md §C)

| Item | Test to add |
|---|---|
| `git-date` local-timezone / calendar parity (A6) | Crosswise: `test-date` under TZ variations |
| `pack-objects` delta compression (A4) | Crosswise: `verify-pack` + `index-pack --verify` on deltified pack |
| `hash-object` `-t`/`--stdin` parity vs `t1007` | Crosswise: `t1007` via shim |

---

## 3. Execution order

```
Phase 0 (fix build)              ← do first, unblocks everything
   └─▶ Phase 1a (git-test)       ← enables real t/ scoreboard
         └─▶ Phase 1b (t/ scoreboard)
   └─▶ Phase 1c (fuzz)           ← independent, can parallelize
   └─▶ Phase 1d (coverage gate)  ← independent, can parallelize
   └─▶ Phase 2 (deepen tests)    ← per-phase, in dependency order
         Phase 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10+
   └─▶ Phase 3 (finish Phase A)  ← A8/A9/A11/A13
   └─▶ Phase 4 (close deviations)← A6/A4/t1007
```

## 4. Success criteria (per item)

Every work item is **done** when:

1. `cargo test --workspace` passes (unit + doc + proptest).
2. The item's crosswise suite is byte-identical vs C git.
3. The suite is registered in `xtask::suites()`.
4. `scoreboard.json` baseline is updated with the new passing suite.
5. No regression in any existing suite.
6. (If applicable) `t/` scripts for the feature pass through the shim.

## 5. Estimated effort

| Phase | Items | Rough effort |
|---|---|---|
| 0 — Fix build | 3 code fixes | Small |
| 1a — git-test | 20 subcommands in 7 batches | Large |
| 1b — t/ scoreboard | 1 xtask upgrade | Medium |
| 1c — Fuzz | 8 targets + xtask command | Medium |
| 1d — Coverage gate | CI wiring | Small |
| 2 — Deepen tests | ~40 crosswise test additions | Large (ongoing) |
| 3 — Finish Phase A | 4 items | Medium |
| 4 — Close deviations | 3 items | Medium |
