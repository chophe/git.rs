# Feature Specification: Differential Compatibility Testing

**Feature Branch**: `012-differential-testing`

**Created**: 2026-09-20

**Status**: Draft

**Input**: User description: "Create a comprehensive specification for differential compatibility testing between git.rs and standard Git. The central testing principle is: Given the same repository, filesystem state, configuration, environment, and command invocation, git.rs should produce behavior compatible with standard Git for every feature declared compatibility-complete. Define: golden tests, differential tests, repository fixture tests, binary-format tests, CLI output tests, exit-code tests, stdout/stderr tests, filesystem snapshot tests, object database compatibility tests, cross-client interoperability tests, fuzz testing, property-based testing, corruption testing, crash/recovery testing, concurrency testing. Tests should include both directions: (1) repositories created by Git consumed by git.rs, (2) repositories created by git.rs consumed by Git. Where exact output legitimately differs, explicitly define the accepted compatibility boundary instead of silently ignoring differences."

## Central Principle *(normative)*

Given the same repository, filesystem state, configuration, environment, and command invocation, git.rs produces behavior compatible with standard Git for every feature declared compatibility-complete. Every test family below exists to check one facet of that sentence; anything that weakens a comparison (normalization, filtering, ignored fields) MUST be recorded in the compatibility-boundary registry (FR-022), never applied silently.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Prove identical command behavior on identical inputs (Priority: P1)

A contributor runs the differential suite for a compatibility-complete command and gets a pass/fail verdict proving that, for the same repository, files, config, environment, and arguments, git.rs emits the same stdout, same stderr class, and same exit code as standard Git.

**Why this priority**: This is the central principle made executable. Without byte-level CLI comparison, every other test family can pass while users still see different behavior.

**Independent Test**: Can be fully tested by running each differential suite against both binaries on shared fixtures and asserting identical captures; introducing a deliberate output divergence MUST turn the suite red.

**Acceptance Scenarios**:

1. **Given** a compatibility-complete command and a shared fixture, **When** the differential suite invokes both binaries identically, **Then** stdout bytes, stderr diagnostics, and exit codes match per the command's declared boundary, and any mismatch fails the suite with a diff pointing at the divergence.
2. **Given** a command NOT yet declared complete, **When** its differential cases run, **Then** they are marked as expected-divergent with a boundary record — never silently excluded from the run.
3. **Given** environment-sensitive output (timestamps, timezones, paths, ordering), **When** suites run, **Then** hermetic controls (fixed clock, fixed timezone, canonical paths, sorted iteration) hold, so reruns on the same machine produce identical results.

---

### User Story 2 - Trust repositories in both directions (Priority: P1)

A user creates a repository with standard Git and works in it with git.rs, or creates one with git.rs and hands it to standard Git (or alternates between the two command by command), and everything reads, verifies, and continues identically.

**Why this priority**: Bidirectional interop is the project's core promise ("crosswise"). One-directional tests hide writer bugs that only the other implementation trips over.

**Independent Test**: Can be fully tested by generating fixture repositories with each implementation (objects, packs, indexes, refs, reflogs, worktrees, both hash widths) and running the full read/verify/mutate matrix from the opposite side.

**Acceptance Scenarios**:

1. **Given** a repository created by standard Git (loose + packed + packed-refs + index + worktree), **When** git.rs reads, mutates, and verifies it, **Then** every object, ref, and listing agrees and standard Git still verifies the result afterward.
2. **Given** a repository created by git.rs, **When** standard Git reads, mutates, and verifies it, **Then** the same agreement holds in reverse, including `fsck`-family verification passing.
3. **Given** an alternating script (command 1 via Git, command 2 via git.rs, …), **When** it completes a stage→commit→branch→walk flow, **Then** intermediate IDs and the final store are identical to a single-implementation run.

---

### User Story 3 - Lock formats with golden and snapshot tests (Priority: P2)

A contributor changing a serializer or command output runs golden (checked-in expected bytes), binary-format, and filesystem-snapshot suites that fail on any unintended byte change to on-disk formats, CLI output, or work-tree/index/store end states.

**Why this priority**: Formats are the interop contract. Golden files and snapshots turn "byte-compatible" from an aspiration into a checked-in artifact that reviews can inspect.

**Independent Test**: Can be fully tested by mutating one serialization byte or one output line and asserting the corresponding golden/snapshot suite fails naming the artifact; regenerating goldens MUST be an explicit, reviewable command, never automatic.

**Acceptance Scenarios**:

1. **Given** checked-in golden files for object serializations, pack/index samples, and CLI outputs, **When** any suite runs, **Then** current bytes match goldens exactly, and regeneration is only via the explicit fixture command with reviewer-visible diffs.
2. **Given** a command that mutates the store or work tree, **When** its snapshot suite runs, **Then** post-run file trees (paths, modes, bytes) plus control files (index, refs, HEAD, packed-refs) match the recorded snapshot or the opposite implementation's end state.
3. **Given** a binary-format sample (index, pack, pack-index, commit-graph, config), **When** parsed by both implementations, **Then** field-level agreement holds (same entries, offsets, checksums verdicts), not just "parses without error".

---

### User Story 4 - Break it like the real world does (Priority: P2)

A contributor runs corruption, crash/recovery, and concurrency suites that prove git.rs reports damage like standard Git, never shows half-written state after a kill, and never loses a concurrent update silently.

**Why this priority**: Safety properties cannot be verified by happy-path differentials. Corruption handling, atomicity, and race behavior are where data-loss bugs hide, and they need dedicated hostile harnesses.

**Independent Test**: Can be fully tested with a shared damage corpus (bit-flips, truncations, bad headers/checksums at every layer), kill-injection at every write point, and racing writers with expected-old guards — asserting matching verdicts, old-or-new-only reads, and zero silent overwrites.

**Acceptance Scenarios**:

1. **Given** each damage-corpus fixture, **When** both implementations read/verify it, **Then** verdicts (healthy/corrupt/missing/dangling) and exit-code classes agree, with diagnostics naming the same object/ref/file and cause.
2. **Given** kills injected at every atomic-write point, **When** readers subsequently open the store, **Then** they observe only complete old or complete new states, with no lock residue blocking future writers beyond standard stale-lock behavior.
3. **Given** concurrent writers racing the same ref/index/config, **When** the race harness completes, **Then** every race resolves as winner-plus-detected-conflict (never a silent overwrite), identically classified by both implementations.

---

### User Story 5 - Explore inputs no human would write (Priority: P3)

A contributor runs property-based and fuzz suites that feed arbitrary bytes and generated histories into every parser/serializer and prove two invariants: no panics on any input, and round-trip identity (parse→serialize→hash equals the original) for well-formed inputs.

**Why this priority**: Hand-written fixtures cover known cases; parsers meet hostile input in the wild (corrupt clones, fuzzed packs, adversarial configs). Generative testing is the only scalable way to cover the unknown.

**Independent Test**: Can be fully tested by seeding corpora from real fixtures, running fixed-budget generative runs, and asserting zero panics plus round-trip equality; every crash MUST produce a minimized regression fixture checked into the corpus.

**Acceptance Scenarios**:

1. **Given** arbitrary bytes fed to any parser (object headers, packs, indexes, configs, refs, reflogs), **When** the no-panic property runs, **Then** zero inputs cause panics or hangs — every input yields a value or a well-formed error.
2. **Given** generated well-formed artifacts, **When** the round-trip property runs (serialize→parse→serialize, hash stability, incremental-equals-oneshot), **Then** all invariants hold across the run budget.
3. **Given** a fuzzer-found crash, **When** it is minimized and added to the corpus, **Then** the regression suite fails without the fix and passes with it, permanently.

---

### Edge Cases

- Fixture staleness: checked-in goldens generated by an older standard-Git version; regeneration MUST record the generator version and diff goldens for review rather than silently updating.
- Nondeterminism sources: timestamps, timezones, PIDs, temp paths, hash iteration order, filesystem mtime granularity, locale; every suite MUST pin or normalize these through declared controls, each normalization recorded in the boundary registry.
- Platform variance: path separators, executable bits, symlinks, case-insensitive filesystems, nanosecond timestamp support; suites MUST declare which platforms they gate on and skip-with-reason elsewhere, never pass vacuously.
- Scale edges: empty repositories, single-object stores, 100k-ref stores, gigabyte packs, deep histories; performance budgets are advisory (correctness gates first) but MUST be measured and reported.
- Oracle availability: standard Git binary missing or wrong version — suites MUST fail loudly with the version requirement, never skip into a false green.
- Flaky-signal control: suites MUST be rerunnable to identical verdicts (fixed seeds for generative runs, recorded in output); a suite that passes on retry without a code change MUST be quarantined and fixed, not retried into green.
- Boundary drift: any new normalization or ignored-field MUST fail review unless accompanied by a boundary-registry entry with rationale, scope, and expiry review date.

## Requirements *(mandatory)*

### Functional Requirements

Harness and hermeticity:

- **FR-001**: Every differential case MUST fix the five inputs of the central principle (repository, filesystem state, configuration, environment, invocation) identically for both binaries: shared fixture setup, pinned clock/timezone/locale, scrubbed absolute paths, and identical argument vectors; any deviation MUST be a declared control, not an accident.
- **FR-002**: Comparisons MUST capture stdout bytes, stderr bytes, and exit codes separately; verdicts MUST distinguish match / boundary-accepted mismatch (FR-022) / real mismatch, and real mismatches MUST print a minimal diff identifying the divergent stream and cause.
- **FR-003**: Suites MUST be deterministic: reruns on the same machine and standard-Git version produce identical verdicts; generative runs use recorded seeds; ordering-dependent outputs iterate in a canonical order.
- **FR-004**: The oracle requirement MUST be explicit: suites declare the required standard-Git version, verify it at startup, and fail loudly (not skip) when it is missing or wrong.

Test families (each requested kind defined with what it checks and gates):

- **FR-005 (golden tests)**: Checked-in expected bytes for serializations, CLI outputs, and diagnostics; MUST fail on any byte change; regeneration MUST be an explicit reviewer-visible command; goldens MUST record the generating standard-Git version.
- **FR-006 (differential tests)**: Same-invocation both-binaries comparison of stdout/stderr/exit per FR-002; required for every feature declared compatibility-complete; incomplete features MUST carry boundary records, never silent exclusions.
- **FR-007 (repository fixture tests)**: Shared fixture repositories (plain, bare, worktree-linked, packed, shallow/grafted, both hash widths, conflicted, special names/modes) generated by a pinned procedure; MUST be usable identically by every suite.
- **FR-008 (binary-format tests)**: Field-level parse agreement on index, pack, pack-index, commit-graph, config, refs, and reflog samples — same entries/offsets/values/checksum verdicts, not merely "both parse".
- **FR-009 (CLI output tests)**: Byte-identical stdout for machine-readable and human-readable outputs within each command's declared boundary; intentional format deviations MUST be boundary-registered per line-pattern, never whole-output-ignored.
- **FR-010 (exit-code tests)**: Exit-class agreement (`0` success, `1` negative verdict/general error, `128` fatal, `129` usage) on every case, including negative/edge invocations; exit-code-only divergences MUST still fail the suite.
- **FR-011 (stdout/stderr tests)**: Stream-separation discipline — payload on stdout, diagnostics on stderr; diagnostics MUST name the object/ref/file and cause with the same specificity class; stream-swapped output MUST fail even when combined text matches.
- **FR-012 (filesystem snapshot tests)**: Post-command comparison of work-tree bytes/modes, index bytes, ref/HEAD/packed-refs bytes, and lock-residue absence; snapshots MUST cover clean, dirty, and conflicted end states.
- **FR-013 (object database compatibility tests)**: Loose/pack/delta/alternates reads and writes cross-checked per object: IDs equal, bytes equal, verification verdicts equal, in both directions.
- **FR-014 (cross-client interoperability tests)**: Alternating-implementation scripts plus full handover flows (create in A → work in B → verify in A); MUST assert intermediate-ID equality at every handoff, not just final-state equality.
- **FR-015 (fuzz testing)**: Continuous arbitrary-byte feeding of every parser with fixed CI budgets and fixture-seeded corpora; every finding MUST become a minimized checked-in regression case; zero open crashers is a release gate.
- **FR-016 (property-based testing)**: Checked-in properties for no-panic-on-arbitrary-input, round-trip identity, hash stability, and incremental-equals-oneshot on all parsers/serializers; properties run in the standard unit-test command with recorded seeds.
- **FR-017 (corruption testing)**: Shared damage corpus (bit-flip, truncate, header lie, checksum damage, delta-base damage, bad ref/log lines) at every layer; verdict + exit-code + diagnostic agreement required per fixture.
- **FR-018 (crash/recovery testing)**: Kill-injection at every atomic-write point (object, ref, index, config, pack, reflog, batch commit); old-or-new-only reads and lock-hygiene asserted after every kill point.
- **FR-019 (concurrency testing)**: Racing writers on refs, index, and config with expected-value guards; required outcome classes are applied / stale-detected / lock-conflicted — silent overwrite is always a failure; races MUST run enough iterations to observe conflicts, not just once.

Directions and regression gating:

- **FR-020 (direction 1 — Git creates, git.rs consumes)**: Fixtures generated by standard Git MUST be read, walked, verified, and mutated by git.rs with full agreement; this direction MUST include corrupt and edge-case fixtures, not only healthy ones.
- **FR-021 (direction 2 — git.rs creates, Git consumes)**: Artifacts written by git.rs MUST be read, walked, verified, and mutated by standard Git with full agreement, including standard verification tools passing on git.rs output.
- **FR-022 (compatibility-boundary registry)**: Every accepted non-identical behavior MUST be a checked-in record stating: the exact output pattern, why exact match is not required, the weaker predicate accepted instead, the affected commands/fixtures, and a review date; comparisons MUST enforce the predicate mechanically — unregistered differences fail, and registry entries MUST NOT use whole-output ignores where a line-pattern predicate suffices.
- **FR-023 (scoreboard regression gate)**: A committed baseline records per-suite verdicts; the gate command MUST fail on any regression vs baseline, and baseline updates MUST be explicit, reviewer-visible commits allowed only for intentional behavior changes.
- **FR-024 (suite registration rule)**: Every newly ported feature MUST land with its differential suite registered in the single suite index, its fixtures, its boundary records (if any), and its `t/`-suite wiring where applicable — an unregistered feature MUST fail the completeness check.
- **FR-025 (upstream `t/` suite)**: The upstream shell test suite runs through the dispatcher shim (ported commands to git.rs, rest to standard Git); gated scripts per feature MUST pass 100%, and shim-routing changes MUST be covered by a routing test proving each claimed command reaches git.rs.

### Key Entities

- **Differential case**: One fixed (repository, filesystem, config, environment, invocation) pair executed under both binaries with captured stdout/stderr/exit.
- **Fixture**: A shared, reproducibly generated repository/state/config bundle both binaries consume; includes healthy, edge-case, and corrupt variants.
- **Golden file**: Checked-in expected bytes (output, serialization, diagnostic) that current behavior MUST match exactly.
- **Snapshot**: Checked-in (or opposite-implementation-derived) post-command filesystem/store end state used for comparison.
- **Damage corpus**: The shared set of corrupted fixtures, one per layer and failure mode, with agreed verdicts.
- **Boundary record**: A checked-in declaration of an accepted non-identical behavior with its weaker predicate, scope, rationale, and review date.
- **Baseline**: The committed per-suite verdict record the regression gate compares against.
- **Oracle**: The pinned standard-Git binary and version that defines correct behavior (the `t/` suite wins on disagreements).
- **Handoff**: A point in a cross-client flow where one implementation's output becomes the other's input; intermediate IDs MUST match here.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: For every feature declared compatibility-complete, its differential suite passes 100% on shared fixtures — identical stdout/stderr/exit within declared boundaries — in both directions (Git-created and git.rs-created stores).
- **SC-002**: Every accepted output difference is traceable to a boundary-registry record: audits find zero silent normalizations, zero whole-output ignores, and zero expected-divergent cases without a record.
- **SC-003**: The regression gate catches real breakage: 100% of deliberately injected divergences (output byte, exit code, serialization byte, snapshot file) across a 20-injection drill turn the gate red with a diff naming the divergence.
- **SC-004**: Hostile suites run green by construction: zero open fuzzer crashers, zero property violations over the fixed run budget, 100% damage-corpus verdict agreement, zero half-written states observed over the kill matrix, and zero silent overwrites over the race harness.
- **SC-005**: Suites are trustworthy infrastructure: reruns produce identical verdicts (deterministic), missing/wrong-version oracles fail loudly instead of skipping, and contributors resolve a red suite to a named divergence or boundary record in under 30 minutes median.
- **SC-006**: The committed baseline shows no regressions release over release, and every newly completed feature lands with its suite, fixtures, boundary records, and upstream-script wiring registered per the registration rule.

## Assumptions

- Standard Git at the pinned version is the oracle; where its documentation and its test suite disagree, the test suite wins.
- The pinned oracle version is recorded alongside goldens and baselines; oracle upgrades are explicit events that re-verify goldens, boundaries, and baselines rather than silent environment changes.
- Exact-output identity is the default; the boundary registry exists for legitimately divergent outputs (e.g. paths, PIDs, version strings), not as a general tolerance mechanism.
- Performance and scale budgets are advisory and reported, not gating, until a feature declares them; correctness, safety, and determinism gate first.
- Network-dependent and interactive behaviors are out of scope for differential comparison until their features declare compatibility-complete; their harnesses follow the same family definitions when they land.
