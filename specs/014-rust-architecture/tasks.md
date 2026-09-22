# Tasks: Rust Workspace Architecture

**Input**: Design documents from `specs/014-rust-architecture/` (plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md)

**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/

**Tests**: This spec explicitly requests drills, gates, and property checks as acceptance (User Stories 1–6 Independent Tests, SC-002/SC-003/SC-004/SC-006). Drill/gate tasks below ARE the acceptance tests — not optional extras.

**Organization**: Tasks grouped by user story; each story independently implementable and testable after Foundational gates land. Run all cargo commands from `crates/` (the real workspace; repo-root `Cargo.toml` is stale).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Confirm green baseline before any structural work

- [X] T001 Verify clean workspace build with `cargo build --workspace` in `crates/` and record output in task log
- [X] T002 [P] Verify baseline test suite with `cargo test --workspace` in `crates/` and record pre-existing failures (if any) before structural changes
- [X] T003 [P] Verify warning-free MSRV build per `crates/Cargo.toml` (`rust-version = "1.74"`, edition 2021) and record toolchain output
- [X] T004 Confirm scoreboard baseline is clean (`git status` shows no `crates/scoreboard.json` diff) and `cargo xtask scoreboard` passes in `crates/`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Mechanical gates all later stories depend on (contracts/gates.md)

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T005 Implement dependency-check gate module in `crates/xtask/src/depcheck.rs` enforcing `specs/014-rust-architecture/contracts/dependency-rules.md` (cycle + layer-violation detection, names offending change)
- [X] T006 Implement safety-gate scan module in `crates/xtask/src/safety.rs` enforcing zero unjustified `unsafe` in first-party code under `crates/` (excludes `crates/target/`, allows documented regex-literal matches)
- [X] T007 [P] Implement fault-injection drill harness in `crates/xtask/src/drills.rs` covering one fault per layer (bad object bytes, missing ref, corrupt index, bad config) asserting layer attribution + exit-code class per `specs/014-rust-architecture/contracts/error-contract.md`
- [X] T008 [P] Implement oversized-fixture generator in `crates/xtask/src/fixtures.rs` (hundred-MB blob, deep delta chain, wide tree) with peak-RSS comparison against system C git per `specs/014-rust-architecture/research.md` R-04
- [X] T009 [P] Implement placement-probe harness in `crates/xtask/src/placement.rs` asserting each of the 21 boundaries in `specs/014-rust-architecture/contracts/placement.md` resolves to exactly one owner with zero overlaps/gaps
- [X] T010 Wire all gates into a single `cargo xtask gates` entry point in `crates/xtask/src/main.rs` (depends on T005–T009)

**Checkpoint**: Foundation ready — `cargo xtask gates` runs green on the untouched tree; user story work can now begin in parallel

---

## Phase 3: User Story 1 - Add a command without touching unrelated components (Priority: P1) 🎯 MVP

**Goal**: Probe-command drill proves new commands land as one module + dispatch entry with zero edits to existing libraries

**Independent Test**: Add a probe command depending on a single library; change set touches only the new module, dispatch table, shim list, and suite; dependency check reports exactly one new edge set and zero new library-to-library edges

- [X] T011 [P] [US1] Write probe-command contract test in `crates/git-command/tests/probe_command.rs` asserting a new command compiles against existing store/ref interfaces with zero changes to those components
- [X] T012 [US1] Run the probe-command drill end to end (scaffold probe in `crates/git-command/src/probe_drill.rs`, dispatch entry in `crates/git-command/src/lib.rs`, shim entry in `scripts/shim-git`) and assert change-set scope plus single new edge set via `cargo xtask gates` (depends on T010, T011), then remove the probe
- [X] T013 [US1] Add plumbing-output stability check in `crates/git-command/tests/plumbing_stability.rs` proving a display-text change cannot alter machine-readable output (porcelain/plumbing split edge case)

**Checkpoint**: User Story 1 fully functional and testable independently — command throughput no longer risks whole-workspace regressions

---

## Phase 4: User Story 2 - Test a component in isolation (Priority: P1)

**Goal**: Every component's tests pass stand-alone in a bare directory with scrubbed environment; parsers hold no-panic/round-trip properties

**Independent Test**: Run each foundation/mid/store component's tests with no fixture repos and no env vars; arbitrary-byte inputs hold documented properties without consulting other components

- [X] T014 [P] [US2] Implement bare-dir/scrubbed-env isolation runner in `crates/xtask/src/isolation.rs` executing each component's test target without repository fixtures or environment dependence (SC-002 gate)
- [X] T015 [P] [US2] Add missing no-panic/round-trip proptest suites for parsers/serializers in `crates/git-hash/`, `crates/git-object/`, and `crates/git-varint/` (one suite per crate, arbitrary-byte inputs)
- [X] T016 [P] [US2] Add missing no-panic/round-trip proptest suites for parsers/serializers in `crates/git-index/`, `crates/git-config/`, and `crates/git-date/` (one suite per crate, arbitrary-byte inputs)
- [X] T017 [US2] Audit component tests under `crates/*/tests/` for shell-outs and cross-component fixtures; rewrite violations to plain-value inputs per `specs/014-rust-architecture/research.md` R-06 (depends on T014)

**Checkpoint**: User Stories 1 AND 2 both work independently — failures are local to one component

---

## Phase 5: User Story 3 - Trace any failure to its owning layer (Priority: P2)

**Goal**: Every fault surfaces the failing layer with exit-code class preserved; context flows as explicit values from the CLI edge

**Independent Test**: Inject one fault per layer and assert the surfaced error names the layer and cause with exit code matching the declared class; repository context reproducible by passing the same values in tests

- [X] T018 [P] [US3] Audit per-component error enums across `crates/*/src/*.rs` against `specs/014-rust-architecture/contracts/error-contract.md` (flat specific variants, no `Other`/`Misc` catch-alls for new errors) and fix violations
- [X] T019 [US3] Execute fault-injection drills via `crates/xtask/src/drills.rs` and record 100% layer-attribution results with exit classes 0/1/129/128+ preserved (depends on T007, T018)
- [X] T020 [US3] Audit library code under `crates/*/src/` for process-global reads (`std::env`, `set_current_dir`, cwd) past the CLI edge; relocate findings to explicitly passed `RepoContext` values originating in `crates/git-cli/` (production paths only; test harnesses keep serializing guards)

**Checkpoint**: User Stories 1–3 work independently — every bug attributes to exactly one layer

---

## Phase 6: User Story 4 - Move large data without copying it (Priority: P2)

**Goal**: Single compression boundary plus streaming bulk paths; oversized fixtures match C git output with same-order-of-magnitude memory

**Independent Test**: Hundred-MB blob hash/store and deep-pack resolution show bounded peak memory with byte-identical outputs to standard Git

- [X] T021 [US4] Scaffold `crates/git-compress/Cargo.toml` + `crates/git-compress/src/lib.rs` as the sole zlib/deflate boundary (streaming encode/decode with size caps over the single vetted `flate2` provider) and register the member in `crates/Cargo.toml`
- [X] T022 [US4] Migrate loose/pack compression call sites in `crates/git-odb/src/` to the `git-compress` facade; verify `flate2` appears in exactly one workspace `Cargo.toml` (`crates/git-compress/Cargo.toml`) (depends on T021)
- [X] T023 [P] [US4] Audit stream-shaped APIs in `crates/git-odb/src/`, `crates/git-hash/src/`, and `crates/git-revision/src/` for hidden whole-input buffering; label buffering APIs as buffering per FR-026
- [ ] T024 [US4] Run oversized-fixture validation via `crates/xtask/src/fixtures.rs` (byte-identical outputs + memory parity vs C git) and record results (depends on T008, T022, T023)

**Checkpoint**: Bulk paths stream structurally — real repositories cannot OOM a copy-per-layer design

---

## Phase 7: User Story 5 - Place the next subsystem without restructuring (Priority: P2)

**Goal**: All 7 missing slots scaffolded as compiling probes with documented owners/deps/boundary data and zero neighbor edits

**Independent Test**: Probe implementation in each documented slot compiles with no edits to neighboring components; all 21 boundaries resolve to exactly one owner

- [ ] T025 [P] [US5] Scaffold `crates/git-pathspec/` (`Cargo.toml` + `src/lib.rs` + probe test) per `specs/014-rust-architecture/contracts/placement.md` FR-011 and register the member in `crates/Cargo.toml`
- [ ] T026 [P] [US5] Scaffold `crates/git-worktree/` (`Cargo.toml` + `src/lib.rs` + probe test) per `specs/014-rust-architecture/contracts/placement.md` FR-017 and register the member in `crates/Cargo.toml`
- [ ] T027 [P] [US5] Scaffold `crates/git-transport/` (`Cargo.toml` + `src/lib.rs` + probe test, C-git retry/timeout/progress parity) per `specs/014-rust-architecture/contracts/placement.md` FR-015 and register the member in `crates/Cargo.toml`
- [ ] T028 [P] [US5] Scaffold `crates/git-protocol/` (`Cargo.toml` + `src/lib.rs` + canned-stream test) per `specs/014-rust-architecture/contracts/placement.md` FR-016 and register the member in `crates/Cargo.toml`
- [ ] T029 [P] [US5] Scaffold `crates/git-hooks/` (`Cargo.toml` + `src/lib.rs` + absent-hooks-identity test) per `specs/014-rust-architecture/contracts/placement.md` FR-018 and register the member in `crates/Cargo.toml`
- [ ] T030 [P] [US5] Scaffold `crates/git-credentials/` (`Cargo.toml` + `src/lib.rs` + canary-secret redaction test, C-git helper scope) per `specs/014-rust-architecture/contracts/placement.md` FR-019 and register the member in `crates/Cargo.toml`
- [ ] T031 [US5] Run the placement drill via `crates/xtask/src/placement.rs` asserting 21/21 boundaries uniquely owned with zero neighbor edits across T025–T030 (depends on T009, T025–T030)

**Checkpoint**: Contributors starting transport/credentials/hooks find a reserved slot — coupling stops growing

---

## Phase 8: User Story 6 - Audit safety and toolchain conformance mechanically (Priority: P3)

**Goal**: Reviewers run checks instead of reading the tree; every violation class fails its gate naming the offender

**Independent Test**: Current tree passes all gates; one introduced violation per gate (unjustified `unsafe`, cycle, MSRV-incompatible construct, incompatible license) fails naming the violation, then reverts

- [ ] T032 [P] [US6] Verify warning-free MSRV 1.74 build and record the new-external-dependency evaluation template (need + GPL-2.0 compatibility + MSRV impact) in `specs/014-rust-architecture/contracts/gates.md` workflow (depends on T003)
- [ ] T033 [US6] Execute one-violation-per-gate negative probes (unjustified `unsafe`, dependency cycle, MSRV-incompatible construct, incompatible license) against `cargo xtask gates`, assert each fails naming the violation, then revert all probes (depends on T010)

**Checkpoint**: All user stories independently functional; guarantees live in gates, not documentation

---

## Phase 9: Polish & Cross-Cutting Concerns

**Purpose**: Final validation and backlog honesty

- [ ] T034 Run `specs/014-rust-architecture/quickstart.md` end-to-end (scenarios 1–6) and fix any validation failures in the owning component (not in the guide)
- [ ] T035 [P] Sync known deviations found during T011–T033 into `docs/plan/FOLLOWUPS.md` as logged backlog entries (reason + affected suites); silently "fixing" C-git behavior is out of scope
- [ ] T036 Verify `cargo test --workspace` green, `cargo xtask differential` byte-identical, `cargo xtask scoreboard` shows no regression vs `crates/scoreboard.json`, and `git status` contains no unintended files (never hand-edit `crates/scoreboard.json`)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — starts immediately
- **Foundational (Phase 2)**: Depends on Setup — BLOCKS all user stories
- **User Stories (Phases 3–8)**: All depend on Foundational completion
  - P1 stories (US1, US2) first, then P2 (US3, US4, US5), then P3 (US6)
  - Parallel team may run stories concurrently once Foundational is green
- **Polish (Phase 9)**: Depends on all desired user stories being complete

### User Story Dependencies

- **US1 (P1)**: After Foundational — no other-story dependencies
- **US2 (P1)**: After Foundational — independent of US1 (shared harness T014 only)
- **US3 (P2)**: After Foundational — uses drill harness T007; independent of US1/US2 outcomes
- **US4 (P2)**: After Foundational — uses fixture harness T008; `git-compress` is new code, zero behavior change to existing paths until T022 migrates callers
- **US5 (P2)**: After Foundational — new crates only; no dependency on US4 (placement harness T009 shared)
- **US6 (P3)**: After Foundational — negative probes assume gates T010 green

### Within Each User Story

- Drill/contract tests first, FAIL before implementation where they assert new behavior
- Scaffolds before migrations (T021 before T022; T025–T030 before T031)
- Story checkpoint validated before next priority begins

### Parallel Opportunities

- Setup: T002, T003 parallel after T001
- Foundational: T007, T008, T009 parallel (separate new files in `crates/xtask/src/`); T005, T006 parallel
- US2: T015, T016 parallel (disjoint crate sets); T014 parallel with both
- US4: T023 parallel with T021/T022 (audit vs new code)
- US5: T025–T030 all parallel (disjoint new crates)
- Polish: T035 parallel with T034

---

## Parallel Example: User Story 5

```bash
# Launch all reserved-slot scaffolds together (disjoint crates, no shared files):
Task: "Scaffold crates/git-pathspec/ (Cargo.toml + src/lib.rs + probe test)"
Task: "Scaffold crates/git-worktree/ (Cargo.toml + src/lib.rs + probe test)"
Task: "Scaffold crates/git-transport/ (Cargo.toml + src/lib.rs + probe test)"
Task: "Scaffold crates/git-protocol/ (Cargo.toml + src/lib.rs + canned-stream test)"
Task: "Scaffold crates/git-hooks/ (Cargo.toml + src/lib.rs + absent-hooks-identity test)"
Task: "Scaffold crates/git-credentials/ (Cargo.toml + src/lib.rs + canary-secret redaction test)"
# Then, after all six land:
Task: "Run placement drill asserting 21/21 uniquely owned (T031)"
```

## Parallel Example: Foundational Gates

```bash
# Launch all gate harnesses together (separate modules, shared contract only):
Task: "Dependency-check gate in crates/xtask/src/depcheck.rs (T005)"
Task: "Safety-gate scan in crates/xtask/src/safety.rs (T006)"
Task: "Fault-injection drills in crates/xtask/src/drills.rs (T007)"
Task: "Oversized fixtures in crates/xtask/src/fixtures.rs (T008)"
Task: "Placement probes in crates/xtask/src/placement.rs (T009)"
# Then wire: single entry in crates/xtask/src/main.rs (T010)
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (T001–T004)
2. Complete Phase 2: Foundational (T005–T010) — CRITICAL, blocks all stories
3. Complete Phase 3: US1 probe-command drill (T011–T013)
4. **STOP and VALIDATE**: probe drill green, `cargo xtask gates` green, no scoreboard diff
5. Demo: adding a command without whole-workspace risk

### Incremental Delivery

1. Setup + Foundational → gates green on untouched tree
2. + US1 → command-addition safety (MVP)
3. + US2 → isolation confidence
4. + US3 → debuggability (fault attribution)
5. + US4 → `git-compress` boundary + streaming proof
6. + US5 → all 21 boundaries placed
7. + US6 → audit-by-gate + Polish → architecture complete

### Parallel Team Strategy

1. Team completes Setup + Foundational together
2. Once Foundational is done: Developer A → US1+US2; Developer B → US3+US4; Developer C → US5; anyone → US6
3. Stories integrate through gates only — no cross-story code dependencies

---

## Notes

- [P] tasks = different files, no dependencies — safe for parallel agents
- [Story] label maps each story-phase task to its user story for traceability
- Constitution compliance per task: no behavior invention (C-git parity), no FFI, honest failure, scoreboard untouched
- Commit after each task or logical group; stop at any checkpoint to validate independently
- Avoid: vague tasks, same-file conflicts in parallel tasks, cross-story dependencies
