# Feature Specification: Diff Merge Infrastructure

**Feature Branch**: `007-diff-merge-infrastructure`

**Created**: 2026-09-20

**Status**: Draft

**Input**: User description: "Create the specification for git.rs diff, merge, and three-way comparison infrastructure. Define: blob comparison, tree comparison, working-tree comparison, index comparison, rename detection, copy detection, similarity calculation, diff algorithms, unified diff output, binary file handling, file mode changes, additions/deletions, conflict representation, three-way merge, merge bases, recursive/ort-style merge behavior where applicable, conflict markers, merge index stages, merge drivers, attributes, custom merge strategies. Separate: core reusable algorithms, Git-compatible command behavior, filesystem/worktree integration. Specify correctness requirements and interoperability tests against standard Git."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Review changes as accurate diffs (Priority: P1)

A user compares blobs, trees, index state, and work-tree files and gets unified diffs, name-status lists, and binary/mode-change reports identical to standard Git, including rename and copy detection.

**Why this priority**: Diff output is the shared language of review, status, log, and patch flows. Wrong hunks, missed renames, or mishandled binary files corrupt every downstream consumer.

**Independent Test**: Can be fully tested by comparing blobs/trees/index/work-tree pairs under both implementations across text, binary, mode-change, add/delete, rename, and copy fixtures, asserting identical patches, summaries, and exit codes.

**Acceptance Scenarios**:

1. **Given** two versions of a text file, **When** the user diffs them, **Then** hunk headers, context lines, and +/- lines are byte-identical to standard Git for the same options.
2. **Given** renamed or copied files above the similarity threshold, **When** the user diffs with rename/copy detection enabled, **Then** both implementations report the same rename/copy pairs with the same similarity scores and headers.
3. **Given** binary files or mode-only changes, **When** the user diffs them, **Then** both implementations emit the same binary notice or mode-change lines instead of text hunks.

---

### User Story 2 - Merge branches with correct conflict handling (Priority: P1)

A user merges two branches sharing a merge base: clean merges produce the correct tree automatically, conflicting merges leave conflict markers in work-tree files plus stage-1/2/3 index entries, and resolution converges to the same tree under either implementation.

**Why this priority**: Three-way merge correctness is a data-integrity boundary. A wrong auto-merge silently loses user content; a wrong conflict representation blocks or misleads resolution.

**Independent Test**: Can be fully tested with two-head merge fixtures (clean, overlapping, criss-cross bases, file/directory conflicts, binary conflicts) comparing merged trees, marker bytes, index stages, and diagnostics.

**Acceptance Scenarios**:

1. **Given** non-overlapping changes on two branches, **When** the user merges, **Then** both implementations produce the same merged tree without conflicts.
2. **Given** overlapping changes to the same file region, **When** the user merges, **Then** both implementations write byte-identical conflict markers, record identical stage-1/2/3 entries, and refuse tree-writing with equivalent diagnostics until resolved.
3. **Given** a resolved conflict staged by the user, **When** the merge concludes, **Then** the resulting tree is identical regardless of which implementation performed the merge.

---

### User Story 3 - Reuse one comparison core everywhere (Priority: P2)

A user running status, diff, log-with-patches, stash, or patch-application sees consistent results because all of them share the same blob/tree comparison, similarity scoring, and diff-rendering core with identical semantics.

**Why this priority**: Duplicated comparison logic drifts. A single core with one similarity model and one hunk renderer guarantees that status renames, diff output, and merge decisions agree with each other and with standard Git.

**Independent Test**: Can be fully tested by cross-checking rename decisions and hunk bytes across commands on the same fixtures: status rename pairs equal diff rename pairs, and merge content decisions match file-level three-way merge of the same inputs.

**Acceptance Scenarios**:

1. **Given** a renamed file, **When** the user checks status and diff, **Then** both report the same old→new pair, and the pair equals the core similarity decision for those blobs.
2. **Given** driver or attribute configuration affecting comparison (text/binary, custom drivers), **When** any command compares content, **Then** all commands honor the same effective driver choice.
3. **Given** a file-level three-way merge, **When** performed standalone or inside a branch merge, **Then** the merged bytes and conflict markers are identical in both contexts.

---

### User Story 4 - Honor attributes and custom merge drivers (Priority: P3)

A user configuring text/binary attributes, diff drivers, or custom merge drivers gets the same comparison and merge behavior in both implementations, including graceful handling of missing driver definitions.

**Why this priority**: Attributes change the meaning of content. Ignoring them produces text diffs of binary files or auto-merges that should have conflicted; failing loudly on missing drivers is safer than guessing.

**Independent Test**: Can be fully tested with attribute fixtures (forced text, forced binary, custom driver patterns, merge-driver assignments, missing-driver cases) comparing diff output and merge outcomes.

**Acceptance Scenarios**:

1. **Given** a binary file marked as text (or vice versa), **When** the user diffs or merges it, **Then** both implementations treat it per the effective attribute, not per content sniffing alone.
2. **Given** a path assigned a custom merge driver that succeeds, **When** the user merges conflicting changes, **Then** both implementations use the driver result identically; when the driver is undefined, both fail with an equivalent diagnostic instead of silently auto-merging.
3. **Given** a merge strategy choice (e.g. resolve-style vs ort-style semantics where covered), **When** the user merges, **Then** tree outcomes, conflict presentation, and diagnostics match for the declared strategy.

---

### Edge Cases

- Empty files on one or both sides; files differing only by trailing newline or missing final newline (`\ No newline at end of file` marker placement identical).
- Files with NUL bytes, invalid UTF-8, lone carriage returns, or mixed line endings: binary detection and line splitting agree exactly.
- Large files: comparison completes without excessive memory use and yields identical hunks; binary-vs-text decision thresholds match standard Git.
- Mode-only changes, type changes (file↔symlink↔submodule), and empty-tree vs missing-tree comparisons reported identically.
- Similarity boundaries: exact-threshold scores, empty-file similarity (never renamed), and rename-vs-delete+add ambiguity resolved deterministically like standard Git.
- Multiple candidate rename sources/targets: pairing choices deterministic and identical, not input-order dependent.
- Criss-cross histories with multiple merge bases: base selection and recursive/virtual-base handling match the declared strategy.
- Conflict styles (merge vs diff3 markers), marker size with nested conflicts, and `--ours/--theirs` auto-resolution produce identical bytes.
- Binary conflicts: never auto-merged as text; work-tree file and stage entries left in the standard conflicted state.
- Custom driver crashes, non-zero exits, or missing executables: merge fails safely with driver diagnostics; no partial driver output committed as resolved.
- Pathological inputs (million-line files, deeply nested trees, adversarialSimilarity cases) complete without hangs or panics; limits documented where standard Git imposes them.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST provide a single reusable comparison core (blob diff, tree diff, similarity scoring, hunk rendering, three-way file merge) used by all commands, so status, diff, log, merge, stash, and patch flows cannot disagree on the same inputs.
- **FR-002**: The system MUST compare blobs line-wise with the declared diff algorithm defaults matching standard Git (including minimal-hunk equivalence for covered options) and MUST document which algorithm options are covered vs deferred.
- **FR-003**: The system MUST compare trees as sorted (mode, name, object-ID) entry sets, reporting additions, deletions, modifications, type changes, and mode changes with identical classification and ordering to standard Git.
- **FR-004**: The system MUST compare index state (staged entries incl. intent-to-add and skip-worktree semantics) and work-tree files (with stat-freshness plus content verification per the index specification) yielding identical dirty/clean decisions to standard Git.
- **FR-005**: The system MUST implement rename detection with the same similarity model, thresholds, and limits as standard Git for covered options, producing identical old→new pairs and `R100`-style score headers.
- **FR-006**: The system MUST implement copy detection where declared, with identical source selection and output to standard Git for covered options; uncovered copy options MUST be rejected or documented as disabled-by-default, never half-applied.
- **FR-007**: Similarity scores MUST be computed deterministically with the same formula and rounding as standard Git, so threshold-boundary fixtures pair identically in both implementations.
- **FR-008**: Unified diff output (hunk headers with offsets/counts, context lines, +/- markers, extended headers for renames/modes/binary, `\ No newline` markers) MUST be byte-identical to standard Git for covered options.
- **FR-009**: Binary files MUST be detected with the same rule as standard Git (NUL-byte heuristic plus attribute overrides) and MUST render as `Binary files ... differ` notices (or binary patches where covered), never as text hunks.
- **FR-010**: The system MUST compute merge bases (best common ancestors, multi-base cases) with identical selection to standard Git for the declared strategy, and MUST expose ancestry-test semantics (`--is-ancestor` equivalent) consistently.
- **FR-011**: Three-way file merge MUST combine base/ours/theirs line-wise, auto-merging non-overlapping changes and conflicting otherwise, with merged bytes identical to standard Git for covered inputs.
- **FR-012**: Conflict markers MUST use the standard format (`<<<<<<<`/`=======`/`>>>>>>>` with ours/theirs labels, diff3 base section where the style is selected) with identical placement, labels, and marker sizes, including nested-conflict growth.
- **FR-013**: Merges MUST record conflicts as index stages 1 (base) / 2 (ours) / 3 (theirs) with correct modes and object IDs, MUST leave work-tree files with markers, and MUST block tree-writing until resolution, per the index specification.
- **FR-014**: Recursive/ort-style multi-base behavior MUST match the declared strategy: virtual-base construction (where applicable), tree-merge order, and file-level merge delegation identical for covered cases; uncovered strategies MUST be rejected explicitly.
- **FR-015**: The system MUST honor text/binary/merge attributes and diff/merge driver configuration with the same precedence (config, attributes, defaults) as standard Git for covered keys; undefined drivers MUST fail loudly, never silently auto-merge.
- **FR-016**: Custom merge strategies and drivers MUST run with identical inputs (ancestor/current/other temp files or blobs), identical argument passing, and identical success/failure interpretation; driver output MUST be validated before acceptance.
- **FR-017**: Core algorithms MUST be total on arbitrary bytes (no panics on non-UTF8, NULs, huge inputs) with documented resource bounds; violations are defects, not diagnostics.
- **FR-018**: Layers MUST be separated: the core exposes pure comparison/merge functions (no filesystem, no config); command behavior maps options to core calls with Git-compatible output/diagnostics; filesystem integration handles work-tree reads, stat refresh, and file writes. Cross-layer behavior MUST still be identical end to end.
- **FR-019**: Differential tests MUST compare unified output, name-status, similarity headers, merged bytes, markers, index stages, and exit codes against standard Git on shared fixtures in both directions (artifacts from either implementation consumed by the other).
- **FR-020**: Phase and scope boundaries MUST be explicit: which diff algorithms, rename/copy options, conflict styles, strategies, and driver types are covered at each level; anything outside is rejected or deferred with a clear diagnostic.

### Key Entities *(include if feature involves data)*

- **Blob Comparison**: Line-oriented diff of two file contents yielding hunks; inputs may come from objects, index, or work tree.
- **Tree Comparison**: Entry-set diff of two trees yielding per-path change records (add/delete/modify/rename/copy/type/mode change).
- **Working-Tree Comparison**: Freshness-checked file reads (stat + content verification) compared against index or tree state.
- **Index Comparison**: Staged-entry set (with flags/stages) compared against HEAD trees or work-tree state.
- **Similarity Score**: Deterministic content-similarity measure (e.g. 0–100) deciding rename/copy pairing against thresholds.
- **Hunk**: A unified-diff block (offsets, counts, context, +/- lines) rendering one changed region.
- **Binary Notice**: The `Binary files ... differ` stand-in emitted instead of hunks for binary content.
- **Merge Base**: A best common ancestor commit of two heads; the three-way merge's base input (possibly virtual for multi-base strategies).
- **Three-Way Merge**: Base/ours/theirs combination producing auto-merged bytes or conflict markers.
- **Conflict Markers**: In-file `<<<<<<<`/`=======`/`>>>>>>>` (plus base section in diff3 style) delimiting unresolvable regions.
- **Merge Stages**: Index slots 1/2/3 holding base/ours/theirs entries for conflicted paths.
- **Merge Driver**: Configured program or builtin rule resolving conflicting content for matching paths.
- **Attributes**: Per-path settings (text/binary, diff driver, merge driver) from attribute files and configuration.
- **Merge Strategy**: The overall branch-merge algorithm (e.g. resolve-style single-base, ort-style multi-base); selects base handling and tree-merge order.

## Detailed Behavioral Specification

Each item states expected behavior, input/output format, compatibility requirements, error behavior, and required tests.

### 1. Blob comparison and diff algorithms

- **Expected**: Normalize-nothing line splitting on LF (lone CRs preserved inside lines); default algorithm yields minimal hunks equivalent to standard Git defaults; covered algorithm options select identical variants.
- **Format**: Internal op sequences feeding hunk renderer; no user-visible format beyond diff output.
- **Compatibility**: Hunk boundaries and minimality MUST match for covered options; uncovered algorithm flags rejected with usage errors.
- **Error**: Unknown algorithm name is a usage error (129 class), not a silent fallback.
- **Tests**: Hunk-equivalence corpus (overlapping edits, repeated lines, newline edge cases) diffed against standard Git output bytes.

### 2. Tree comparison and change classification

- **Expected**: Recursive sorted walk pairing same-name entries; type/mode/ID comparison classifies add/delete/modify/type-change/mode-change; directories recursed, submodules compared as commit IDs.
- **Format**: Internal change records (path, old/new mode, old/new ID, kind) feeding name-status and patch renderers in byte-sorted path order.
- **Compatibility**: Classification, ordering, and empty-tree handling MUST match standard Git exactly.
- **Error**: Corrupt tree objects surface as corruption diagnostics, not empty diffs.
- **Tests**: Tree-pair matrix (all change kinds, nesting, submodules, symlinks) comparing name-status bytes.

### 3. Work-tree and index comparison

- **Expected**: Work-tree files read through stat-freshness (size/timestamps/inode) with content hashing for suspect entries; index entries compared with flag semantics (skip-worktree skipped, intent-to-add as empty).
- **Format**: Same change records as tree comparison, sourced from filesystem + index.
- **Compatibility**: Dirty/clean decisions and refreshed-stat write-back MUST match standard Git observably.
- **Error**: Unreadable files are I/O diagnostics; missing files are deletions, not corruption.
- **Tests**: Racy-clean, permission-denied, and flag-marked fixtures comparing status/diff outputs plus resulting index bytes.

### 4. Rename/copy detection and similarity

- **Expected**: Unpaired deletes and adds scored pairwise; pairs above threshold (default 50% where standard Git defaults apply) emitted as renames/copies with exact scores; pairing deterministic under ties.
- **Format**: `R100`/`C075`-style headers and extended `rename from/to` lines identical to standard Git.
- **Compatibility**: Scores, thresholds, limits (rename limit), and tie-breaking MUST match for covered options.
- **Error**: Over-limit inputs behave per documented policy (same warning + same fallback pairing as standard Git), never silently different pairs.
- **Tests**: Similarity corpus incl. exact-threshold, empty-file, multi-candidate, and limit-exceeded fixtures.

### 5. Unified output, binary, modes, add/delete

- **Expected**: `diff --git` headers, `---`/`+++`, `@@ -a,b +c,d @@`, context lines, binary notices, `old mode`/`new mode`/`new file`/`deleted file` lines, rename headers — all byte-identical.
- **Compatibility**: Default context length, abbreviation of object IDs in `index` lines, and quoting of special paths MUST match.
- **Error**: Malformed option combinations are usage errors; output to broken pipes follows standard conventions.
- **Tests**: Golden-patch fixtures vs standard Git bytes, incl. binary, mode-only, add/delete, rename, no-newline cases.

### 6. Merge bases and strategies

- **Expected**: Best-ancestor computation over commit reachability; single base for linear histories; declared multi-base handling (recursive virtual base or ort equivalent) for criss-cross cases.
- **Compatibility**: Base selection MUST equal standard Git for the declared strategy; ancestry queries agree.
- **Error**: Unrelated histories (no base) reported with the standard no-merge-base diagnostic; strategy/option mismatches are usage errors.
- **Tests**: Base-selection matrix (linear, merged, criss-cross, unrelated, multi-base) comparing chosen bases and outcomes.

### 7. Three-way merge, markers, stages

- **Expected**: Line-level three-way combination; clean regions auto-merged; conflicts emitted with standard markers at correct positions; index stages 1/2/3 + work-tree markers written atomically per the index/locking contracts.
- **Compatibility**: Merged bytes, marker bytes, stage contents, and unmerged-blocked diagnostics MUST match standard Git.
- **Error**: Binary/type conflicts never text-merged; custom-strategy mismatches rejected explicitly.
- **Tests**: File-merge corpus (clean, overlapping, adjacent-line, whitespace, binary, diff3-style) plus branch-merge fixtures comparing trees, markers, stages, exit codes.

### 8. Drivers, attributes, custom strategies

- **Expected**: Attribute lookup precedence (command line, info/attributes, work-tree attributes, config) selects text/binary/diff/merge handling; custom drivers invoked with standard temp-file/argument conventions; results validated.
- **Compatibility**: Effective-driver choice, driver I/O conventions, and missing-driver failures MUST match standard Git for covered driver types.
- **Error**: Undefined driver, driver crash, or invalid output fails the merge path with driver diagnostics; partial output never committed as resolved.
- **Tests**: Attribute/driver matrix in both directions (fixtures authored by either implementation), incl. missing-driver and failing-driver negatives.

### 9. Layering: core vs commands vs filesystem

- **Expected**: Core functions pure (bytes in, records/hunks/merged bytes out); command layer maps flags to core calls and renders Git-compatible output/diagnostics/exit codes; filesystem layer owns work-tree I/O, stat refresh, atomic writes, and lock discipline.
- **Compatibility**: Layer boundaries MUST NOT change observable behavior; any command using the core agrees with any other on the same inputs.
- **Error**: I/O and lock failures surface at the filesystem layer with standard diagnostics; algorithmic failures (unsupported option) surface as usage errors.
- **Tests**: Core unit/property tests (no-panic on arbitrary bytes, round-trip invariants) plus end-to-end differential suites proving layered composition matches standard Git.

### 10. Interoperability tests

- **Direction A (artifacts here, verified by standard Git)**: Patches, merged trees, marker files, and stage states produced here MUST apply/verify/read identically under standard Git (patch application, tree comparison, conflict listing).
- **Direction B (artifacts from standard Git, consumed here)**: Patches, markers, and conflicted states from standard Git MUST parse and behave identically here (same hunks accepted, same resolutions converge).
- **Harness**: Shared fixture repositories plus byte comparison of diff output, merged files, and index dumps; stdout/stderr/exit-code comparison for command equivalents; corruption/lock negatives in both directions.
- **Gates**: No family passes unless both directions agree; any silent rename-pair divergence, marker-byte difference, or stage mismatch is a failure.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Diff outputs (unified patches, name-status, binary/mode/rename headers) are byte-identical to standard Git across a corpus of at least 100 blob/tree/index/work-tree fixtures.
- **SC-002**: Rename and copy pairing agrees with standard Git on every fixture including exact-threshold, empty-file, multi-candidate, and limit cases (zero unexplained pair differences).
- **SC-003**: Merges converge: clean merges yield identical trees and conflicting merges yield byte-identical markers plus identical stage-1/2/3 entries across the merge matrix, with equivalent block-until-resolved diagnostics.
- **SC-004**: Merge-base selection matches standard Git (including multi-base histories) for the declared strategy, and ancestry queries agree on all fixtures.
- **SC-005**: Attribute/driver fixtures behave identically in both directions with zero silent auto-merges and equivalent missing-driver failures.
- **SC-006**: Core algorithms never panic on arbitrary inputs (fuzz property holds) and layered commands agree with each other on shared inputs (status renames equal diff renames; embedded file merges equal standalone merges).
- **SC-007**: A user reviewing, merging, and resolving conflicts observes no behavioral difference between implementations at any step (task-completion parity verified by the differential suites).

## Assumptions

- Standard Git behavior (algorithms plus the `t/` suite) is the oracle; where documentation and the suite disagree, the suite wins.
- Default diff algorithm and default rename-detection thresholds follow the pinned standard Git version; option coverage beyond defaults is declared per command record (see the porcelain-phases spec) rather than assumed here.
- The ort merge strategy is the forward target where multi-base histories apply; single-base resolve-style semantics MUST still match for histories with one base, and any strategy limitation is documented, not silently approximated.
- Custom driver support covers configuration-based external drivers and builtin text/binary/union-class semantics declared in scope; exotic builtins beyond the declared set are deferred with explicit diagnostics.
- The index, object-store, and porcelain-phases specifications are companion contracts; this spec defers byte layouts, locking, and command-gating to them and defines only comparison/merge-observable behavior here.
- Performance targets (large-file diff time, rename-detection limits on huge trees) are out of scope for this spec; correctness and byte-compatibility gate first.
