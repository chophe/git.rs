# Feature Specification: Master Specification

**Feature Branch**: `014-master-specification`

**Created**: 2026-09-20

**Status**: Draft

**Input**: User description: "Synthesize the existing Spec Kit specifications and the current repository into a single coherent master specification for git.rs. Project goal: git.rs is a Rust implementation of Git intended to reproduce Git's observable behavior and remain interoperable with existing Git repositories, clients, and servers. Review all existing specifications, source, workspace, tests, documentation, CI, implementation status, and architectural decisions. Do not duplicate or contradict existing specifications. Resolve conflicts by prioritizing: (1) existing repository constraints and explicit project decisions, (2) Git's documented compatibility requirements, (3) existing tests and observable Git behavior, (4) Rust-native implementation considerations. Produce: system scope, architecture, compatibility contract, command matrix, repository-format matrix, implementation phases, dependency relationships, testing strategy, interoperability strategy, performance requirements, security requirements, error-handling requirements, CLI compatibility requirements, open questions, explicit non-goals. For every major feature assign a status (implemented / partially implemented / specified / planned / intentionally unsupported). Do not claim compatibility merely because a command exists. Compatibility must be demonstrated by tests or explicitly marked as incomplete. Final specification serves as the source of truth for future implementation agents."

## How To Read This Document

This specification synthesizes thirteen prior specifications, the plan documents in `docs/plan/`, and the repository as it exists today. It does not replace the subsystem specifications — it points at them, resolves their conflicts, and assigns a single status to every major feature. Where this document and a subsystem specification disagree, this document wins (conflict resolutions in Conflict Resolutions below). Where this document and the `t/` suite disagree, the `t/` suite wins.

Subsystem specifications: `001-c-to-rust-conversion`, `002-git-rs-product-spec`, `003-repository-object-storage`, `004-git-index-implementation`, `005-porcelain-commands-phases`, `006-plumbing-commands`, `007-diff-merge-infrastructure`, `008-refs-branch-management`, `009-revision-parsing-walking`, `010-network-transport`, `011-config-compatibility`, `012-differential-testing`, `013-path-attributes-ignore` (directory reserved; content missing — see Status Register).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Trust one document to answer "what is done and what is next" (Priority: P1)

A contributor picking up work reads this specification and learns, for any command or format: its status, what compatibility is demonstrated by which test, what is explicitly deferred, and which predecessor document holds the detail — without reconciling contradictory plans themselves.

**Why this priority**: Contradictory scope statements already exist in the repository (see Conflict Resolutions). A single source of truth is the deliverable of this specification.

**Independent Test**: Can be tested by sampling ten features across commands and formats and verifying each has exactly one status, one evidence pointer, and no contradicting claim in any cited document.

**Acceptance Scenarios**:

1. **Given** any major feature named in the Status Register, **When** a reader looks it up, **Then** they find one status value, the test or suite demonstrating it, and a pointer to the detailing specification.
2. **Given** a known inter-document conflict, **When** a reader checks Conflict Resolutions, **Then** they find the conflict named, the resolution, and the priority rule applied.
3. **Given** a command present in the dispatcher, **When** a reader checks the Command Matrix, **Then** they find its completeness explicitly marked rather than implied by its presence.

---

### User Story 2 - Replace Git for local workflows with verified parity (Priority: P1)

A user runs everyday local operations (initialize, stage, commit, inspect, branch, diff, pack, verify) under git.rs and gets results identical to standard Git, with every parity claim backed by a named differential or crosswise suite.

**Why this priority**: Local parity is the project's entry condition and the only compatibility currently demonstrated by tests. Everything networked or exotic builds on it.

**Independent Test**: Can be fully tested by running the registered differential suites and the `t/` scoreboard; every claimed parity maps to a green suite, every gap maps to a deferred item or failing-baseline entry.

**Acceptance Scenarios**:

1. **Given** the implemented local command set on shared fixtures, **When** both binaries run identically, **Then** stdout, stderr, and exit codes match per each command's declared boundary.
2. **Given** artifacts git.rs writes (objects, packs, indexes, refs), **When** standard Git verifies them, **Then** verification passes with no repair step.
3. **Given** repositories standard Git wrote, **When** git.rs reads and mutates them, **Then** all reads agree and standard Git still verifies the result afterward.

---

### User Story 3 - Extend the implementation without breaking compatibility (Priority: P2)

A contributor adds a command or deepens an option surface following this specification's phase gates, and the automated gates alone determine whether compatibility held — including catching a deliberately introduced deviation.

**Why this priority**: The specification must work as operating instructions for future agents, not as a status report. Gates, layering, and boundary rules are what make incremental work safe.

**Independent Test**: Can be tested by introducing a deliberate output divergence in a gated area and confirming the differential suite turns red and the scoreboard check fails.

**Acceptance Scenarios**:

1. **Given** a new command contribution, **When** it lands, **Then** it is registered in the dispatcher, covered by a differential suite, and recorded in the Command Matrix with its boundary.
2. **Given** a behavior change, **When** the scoreboard baseline updates, **Then** the update accompanies the intentional change and never masks a failure.
3. **Given** an uncovered option, **When** a user invokes it, **Then** it is rejected with a usage error rather than silently approximated.

---

### Edge Cases

- A command routed in the dispatcher but incomplete: presence never implies parity; the matrix marks it partial with deferred options listed.
- A scoreboard entry recorded as failing: failing-baseline entries are expected-failure records, not gates passed; promoting one to passing requires the underlying suite to go green, never editing the baseline alone.
- A subsystem specification describing behavior the repository has not built (e.g. transport, full merge): specified status, not implemented; planning reads the spec, status reads the tests.
- The reserved-but-empty `013` directory: treated as planned content with missing specification, not as specified.
- Documentation conflicts (plan overview vs later phase documents): resolved in Conflict Resolutions; the resolution, not the older text, governs.
- Platform differences (case sensitivity, symlinks, permissions): parity claims hold under standard-Git platform degradation rules; cross-platform scenarios belong in the differential suites.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The project MUST deliver a standalone pure-Rust `git` replacement binary with no FFI and no C linkage; the C tree is reference and oracle only (locked decision, reaffirmed from the product specification).
- **FR-002**: Observable behavior MUST be the porting target, never line-by-line translation of C sources; internal architecture stays Rust-native with no observable effect.
- **FR-003**: Machine-readable output and exit codes (including 129 usage, 1 unknown-command) MUST match standard Git for every compatibility-complete surface; human prose SHOULD match with recorded exceptions.
- **FR-004**: On-disk formats MUST be byte-compatible in both directions for every claimed format, verified by standard Git's own integrity commands in one direction and git.rs reads in the other.
- **FR-005**: The `t/` suite MUST be the oracle where C source and suite disagree; the committed scoreboard baseline MUST NOT regress and MUST update only alongside intentional behavior change.
- **FR-006**: Every major feature MUST carry exactly one status from the five-value scale, with compatibility demonstrated by a named test or explicitly marked incomplete (no presence-implies-parity).
- **FR-007**: Uncovered commands and options MUST fail honestly (unknown command exit 1; unknown option exit 129; out-of-scope capability exit 128 with an unsupported diagnostic); silent approximation is a defect.
- **FR-008**: All writes (refs, index, objects, packs, config) MUST be atomic via temp-file plus rename with lock discipline; interrupted operations MUST leave no valid-but-wrong state.
- **FR-009**: Parsers and serializers MUST be total on arbitrary bytes (no panic, hang, or unintended write) within documented resource bounds.
- **FR-010**: Layering MUST hold: storage libraries own formats; the command layer owns CLI behavior; transport moves bytes while protocol negotiates; each layer is testable without the others.
- **FR-011**: Configuration precedence and environment-variable handling MUST match standard Git for covered keys; undeclared keys MUST NOT change behavior.
- **FR-012**: Phase gates MUST require green workspace tests, green phase differential suites, no scoreboard regression, and coverage at the project threshold before the next phase claims completeness.
- **FR-013**: Every intentional deviation MUST be a logged backlog entry naming the reason and affected suites; undiscovered-by-user is the acceptance bar for deviation records.
- **FR-014**: Performance MUST stay within the same order of magnitude as standard Git on comparable operations; deferred internal encodings trading size for simplicity MUST be named backlog items with re-evaluation triggers.
- **FR-015**: This document MUST remain the single authoritative scope statement; subsystem specifications detail behavior within the boundaries set here, and new conflicts are resolved here first.

### Key Entities

- **Status Register**: The per-feature table assigning implemented / partially implemented / specified / planned / intentionally unsupported with evidence pointers.
- **Command Matrix**: Per-command record of dispatcher presence, completeness level, deferred surface, and demonstrating suite.
- **Repository-Format Matrix**: Per-format record of read/write support, width/algorithm variants, and crosswise verification.
- **Compatibility Contract**: The MUST/SHOULD/Rust-specific obligation levels governing every parity claim.
- **Compatibility Boundary**: The declared per-command/per-format edge of covered behavior; everything outside fails honestly.
- **Differential Suite**: Fixture-driven stdout/stderr/exit-code comparison against standard Git on identical inputs.
- **Crosswise Check**: Bidirectional artifact exchange proving either implementation's output works in the other.
- **Scoreboard Baseline**: The committed expected-results record; regression signal, never edited to hide failure.
- **Dispatcher Registry**: The single command-routing table plus the development shim that falls through to system Git for unported commands.
- **Phase Gate**: The machine-checkable exit criteria promoting a phase from claimed to complete.

## System Scope

In scope: local repository lifecycle (init, staging, committing, inspection, branching, diffing, packing, verification), history walking and revision resolution, refs and branch management, index and worktree operations, merge foundations (bases, file-level three-way) with full merge engines sequenced later, configuration and attributes/ignore engines, network transport (sequenced after local phases), and the differential/crosswise test infrastructure that proves all of it.

Out of scope: everything under Explicit Non-Goals. Deferred-but-in-scope work is listed with its phase in Implementation Phases, never silently dropped.

## Architecture

- **Binary layer**: a thin CLI entry delegating every real command to the command dispatcher with a caller-supplied output writer (unit-testable without spawning processes); global options (`-C`, `-c`, `--git-dir`, `--work-tree`, `--common-dir`, `--bare`, `--no-pager`, `--literal-pathspecs`) parsed once into a shared repository context honoring CLI-over-environment-over-discovery precedence plus `GIT_CONFIG_COUNT`-family overrides.
- **Command layer**: one module per command behind a common command trait returning typed errors that map to process exit codes (usage 129, fatal 128, general 1, silent diff-style codes); unknown names return no-handler so the caller reports unknown-command exit 1.
- **Storage libraries** (dependency order): hashing and primitives → configuration, dates, pathspec/attributes → object model and object database (loose, packs, indexes, multi-pack-index, commit-graph) → revision resolution and walking → diff → index → refs → merge. Each library owns its on-disk format and exposes no CLI behavior.
- **Automation**: a task runner owning the crosswise suite registry, fixture generation, and scoreboard regeneration with regression failure; a development shim routing ported commands to the Rust binary and everything else to system Git so the full `t/` suite runs while the port is incomplete.
- **Test layout**: unit plus doc plus property tests per crate; integration crosswise suites per subsystem asserting byte-identical stdout/stderr/exit codes and bidirectional artifact exchange; the upstream `t/` suite as oracle through the shim with the committed baseline as regression signal.
- **Continuous integration**: three jobs — workspace tests plus lints, crosswise suites against system Git, scoreboard regression check.

## Compatibility Contract

- **MUST (gating, machine-verified)**: exact exit codes; byte-identical machine-readable output; bidirectional on-disk compatibility for claimed formats; no FFI; `t/` suite as oracle; no scoreboard regression; buildable and testable at every commit.
- **SHOULD (expected, exceptions recorded)**: stderr wording with standard prefixes; human-facing prose; logged deviations naming reason and suites; same-order-of-magnitude performance; environment-variable handling; configuration precedence semantics.
- **Rust-specific (no obligation)**: crate boundaries, memory model, concurrency model, `Result`-based error plumbing, pure-Rust dependencies, with zero permitted observable effect.
- **Correctness bar for any capability**: behavioral identity on covered inputs; format identity both directions; corresponding upstream scripts passing through the dispatcher; no regression; robustness on malformed input; atomicity under interruption; determinism across reruns; declared boundary with honest failure outside it; coverage at threshold.

## Command Matrix

Completeness levels: L1 viable subset, L2 byte-identical core, L3 full parity. "Routed" means present in the dispatcher; it never alone implies parity. Evidence abbreviations: crosswise suites (CW), upstream scripts (t/), backlog (F).

**Routed and substantially complete (L2 core, gaps listed)**:
- `init` — implemented (plain, bare, initial branch, separate-git-dir, templates, reinit; gaps: sha256 object layer, reftable backend config-only).
- `add` — implemented (pathspecs, `-A`/`-u`, ignore integration, dry-run/verbose, racy-aware cache-tree invalidation; deferred: `-p`/`-i`, `-N`, `--chmod`, pathspec magic).
- `commit` — implemented (multi-step sequences, nothing-to-commit variants, `-a`/amend/allow-empty, identity; deferred: pathspec commits, `--porcelain`/`--dry-run`, hooks, signing).
- `status` — implemented (long/short/porcelain, `-z`, branch display, ignored/untracked handling, unborn, unmerged codes, exact-rename staging; deferred: worktree similarity renames, upstream ahead/behind, stash summary, pathspec limiting).
- `checkout` / `switch` / `restore` / `reset` — partially implemented (core switch/detach/create, staged/worktree restore, soft/mixed/hard, dirty guards, ORIG_HEAD/reflog updates; deferred: merge-mode checkout, `--merge`/`--keep`, `-p`, `--orphan`, submodules).
- `diff` / `diff-tree` — partially implemented (context counts, stat family, rename detection exact plus line-similarity, filters, binary, cached/index/HEAD sources, `--no-index`, exit-code/quiet; known deviation: rename scoring differs from C; deferred: word-diff, color, patience/histogram, dirstat, whitespace family, pickaxe, relative paths).
- `log` / `rev-list` / `show` — routed, partial (walk plus common options; pretty-printing and full option surface pending per revision spec).
- `cat-file` (incl. batch modes) / `hash-object` / `ls-tree` / `mktree` / `commit-tree` / `write-tree` — implemented core (batch with format specifiers; write-tree full with cache-tree writeback; deferred: `hash-object` outside-repo algorithm edge confirmation).
- `read-tree` — partially implemented (one-way, `--empty`, prefix-limited reads; deferred: two/three-way merge reads, worktree update — needs unpack-trees).
- `update-index` — routed, partial (refresh and flag subset; full option surface pending).
- `ls-files` — routed, partial (cached/others/stage listing; full flag surface pending).
- `rev-parse` — substantially complete (discovery flags, verify, short/abbrev-ref).
- `branch` / `tag` (lightweight create/delete/list) / `show-ref` / `for-each-ref` / `update-ref` / `symbolic-ref` — implemented core (checked-out-branch deletion refusal; deferred: packed-refs write, transactions, `--stdin`, worktree refs, annotated-tag full surface).
- `merge-base` (incl. `--is-ancestor`) / `merge-file` (three-way) — implemented core (extended octopus/independent modes pending).
- `verify-pack` / `index-pack` / `pack-objects` (deltified with window/depth/compression options) / `unpack-objects` / `count-objects -v` / `multi-pack-index` / `commit-graph` (read/verify) — implemented core (commit-graph write, chains, bloom query pending).
- `apply` — routed, partial (check/stat/apply core; deferred: three-way, index application, reject files, full whitespace/binary surface).
- `check-ignore` / `check-attr` — implemented over the ignore/attributes engine.
- `fsck` — routed, partial (reachable scan and classification; deferred: strict/connectivity-only/no-dangling/full/lost-found/message catalog).
- `rm` / `mv` / `clean` — routed; worktree-mutation completeness pending verification (matrix marks partial until suites confirm).

**Specified but not implemented**: `merge` (ort engine), `merge-tree`, sequencer family (`cherry-pick`, `revert`, `rebase`), `stash`, full reflog read/write surfaces, `config` command, transport family (`clone`, `fetch`, `push`, `pull`, `ls-remote`, `remote`, `bundle`, `fast-export`/`fast-import`), patch-mail family (`format-patch`, `am`), search/history tools (`grep`, `blame`, `bisect`, `notes`, `replace`, `worktree`, `range-diff`, `rerere`, `archive`), maintenance (`repack`, `gc`, `prune`, `maintenance`), `git` daemon protocol roles, credential handling.

**Intentionally unsupported**: legacy VCS bridges, GUI and web tooling, mail senders, editor/tool front-ends (see Non-Goals).

## Repository-Format Matrix

- **Loose objects** (header, zlib, fanout layout, both hash widths): implemented, crosswise-verified both directions.
- **Packfiles and v2 pack indexes** (undeltified, offset and reference deltas, 64-bit offsets, checksums): implemented, crosswise-verified both directions; pack writing deltified with standard-compatible options.
- **Multi-pack-index** (read/verify/write core): implemented; optional chunks, incremental chains, preferred-pack: planned.
- **Commit-graph** (read/verify, chunk format, bloom parse): implemented; write, chains, bloom query: planned.
- **Pack bitmaps, cruft packs**: planned (not started).
- **Blob/tree/commit/tag serialization** (both widths, canonical ordering, identity lines): implemented, crosswise-verified.
- **Collision-detecting SHA-1**: implemented and threaded through hashing and loose writes.
- **SHA-256 repositories end to end**: partially implemented (width-aware paths exist; full SHA-256 operation matrix pending).
- **SHA-1↔SHA-256 conversion**: planned.
- **Index v2 with TREE extension**: implemented, crosswise-verified; v3/v4 read, REUC, split/sparse index: specified, not implemented.
- **Refs**: loose refs, packed-refs read, symrefs, HEAD attached/detached/unborn: implemented; packed-refs write, transactions/locking, reflogs read/write, reftable backend: planned.
- **Config files** (sections, values, precedence, includes): engine implemented and consumed; full `config` command surface: planned.
- **Shallow and partial-clone state**: specified (transport spec), not implemented.

## Implementation Phases

Phases order by dependency; each gates on green workspace tests, green phase differential suites, no scoreboard regression, and coverage at threshold.

- **Phase A — Foundation and routed-command deepening**: hashing (done, incl. collision detection), discovery and global-option threading (done), batch object reads (done), abbreviation and revision-operator resolution (done), log/rev-list option surface with pretty-printing (partial), diff engine core (done with listed deviations), ignore/attributes engine (done), timezone-correct dates/idents (done), deltified pack writing (done). Status: substantially complete; remaining: deferred diff options, rename-score convergence.
- **Phase B — Workflow core**: init/add/commit/status/checkout-core (done per matrix); index extensions and v3/v4 (partial: TREE done); write-tree/read-tree (partial); rm/mv/clean/show/describe-family/apply-completion (routed, verification pending). Status: in progress.
- **Phase C — History and merge completeness**: ort-class merge, merge-tree, extended merge-base, sequencer, rebase, reflog, ref transactions and packed-refs write, small independent commands, config command, notes/replace/worktree/bisect/stash/grep/blame/archive, fsck completion. Status: planned (cores specified in subsystems 006/008).
- **Phase D — On-disk format completeness**: bitmaps, cruft, commit-graph write, MIDX optional surface, index-pack completion, repack/gc/prune/maintenance, algorithm conversion. Status: planned.
- **Phase E — Network and transport**: local transport and fetch/push core first, then server endpoints, SSH with version fallback, HTTPS with auth/proxy/TLS boundaries, shallow and partial clone, helpers and hardening last. Status: specified (010), not implemented.
- **Phase F — Stretch**: signature verification, mail-series tooling, remaining ancillary commands, sparse/split index rollout, filters and line-ending conversion. Status: planned-low.

## Dependency Relationships

Foundation (hashing, configuration, dates, core paths) underpins the object database, which underpins the object model and revision walking; diff builds on tree walking; index and worktree build on the object model; refs stand beside foundation; merge needs object model plus diff plus index plus refs; maintenance needs everything; transport needs the object database plus refs plus maintenance primitives; porcelain phases compose all lower layers. Test infrastructure runs as a parallel track gating every phase. No layer above may bypass the layer below (e.g. commands must not reimplement comparison, hashing, or ref updates inline).

## Testing Strategy

- **Unit, doc, and property tests** per library: parsers never panic, serializers round-trip, incremental hashing equals one-shot hashing.
- **Differential suites**: identical fixtures under both binaries; stdout, stderr, and exit codes compared byte-for-byte within each command's declared boundary; deliberate divergences turn suites red.
- **Crosswise checks**: every artifact git.rs writes verified by standard Git's own integrity commands; every artifact standard Git writes read back; alternating-implementation scripts proving converged end states.
- **Golden and snapshot tests**: byte vectors for headers, entries, extensions, patches, markers, and framing locking formats against silent drift.
- **Corruption, crash, and concurrency tests**: bit-flip/truncation corpora, kill-during-write proving old-or-new completeness, lock-contention suites, stale-lock handling.
- **Fuzzing**: parser targets run to a fixed budget with zero panic/hang/write findings (track operational; harness pending).
- **Upstream oracle**: `t/` scripts through the dispatcher with the committed baseline as regression signal; baseline updates only with intentional behavior change in the same change.
- **Coverage**: project threshold (90% lines) enforced per phase's libraries at phase completion (track operational; gate pending in CI).

## Interoperability Strategy

- **Client direction**: git.rs artifacts and requests accepted by standard Git (servers, verifiers, patch appliers, clients reading served endpoints).
- **Server direction**: standard-Git artifacts and requests accepted by git.rs (repos, packs, indexes, refs, patches, conflicted states, remote dialogues once transport lands).
- **Alternation**: mixed-implementation scripts must converge to single-implementation end states.
- **Boundaries**: environment-sensitive output normalized only under declared hermetic controls; any comparison weakening recorded in a boundary registry, never silent.
- **Transport interop** (once Phase E lands): both client-against-standard-server and standard-client-against-git.rs matrices over local, SSH, and HTTPS fixture servers.

## Performance Requirements

Same order of magnitude as standard Git on comparable operations (user-observable wall clock, technology-agnostic). Deferred internal encodings that trade size or speed for simplicity are permitted only as named backlog items with re-evaluation triggers. No phase claims completion on performance alone; correctness and byte-compatibility gate first. Benchmark repositories and maximum acceptable factors are planning-track work, not assumed here.

## Security Requirements

Collision-attack inputs rejected with collision diagnostics through all hashing and verification paths (implemented). Secrets never cross the credential boundary into logs, diagnostics, or persisted state beyond the helper contract (transport-phase gate). TLS identity verified against system trust with standard failure classes and no overbroad insecure bypass (transport-phase gate). Untrusted incoming objects quarantined until validated and accepted (implemented for local paths; extended to transfers in Phase E). Alternates treated read-only; maintenance never deletes reachable objects; corruption reported, never auto-repaired or silently skipped. Resource bounds enforced on decompression, delta application, and revision walking (no zip-bomb allocation, no unbounded recursion).

## Error-Handling Requirements

Exit-code classes: 0 success; 1 general errors and diff-found semantics; 2 documented diff-error classes where the reference uses them; 128 fatal (corruption, lock failure, unmerged-blocked operations, transport rejection, unsupported capability); 129 usage (bad options, bad revision syntax class, unknown option). Diagnostics carry standard prefixes (`fatal:`, `error:`, `warning:`, `usage:`) and name the offending object, path, ref, or file. Missing versus ambiguous versus corrupt are distinct verdicts. Failed operations leave prior state intact with no lock residue. Unknown versions, capabilities, extensions, or strategies are explicit errors or explicitly documented skips — never misparsed guesses.

## CLI Compatibility Requirements

Global context honored uniformly: `-C`, `-c`, `--git-dir`, `--work-tree`, `--common-dir`, `--bare`, `--no-pager`, `--literal-pathspecs`, plus repository discovery honoring `GIT_DIR`, `GIT_WORK_TREE`, `GIT_COMMON_DIR`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES`, `GIT_CONFIG_COUNT`-family, `GIT_CEILING_DIRECTORIES`, identity variables, pager/editor selection, and trace families for covered surfaces. Paging and color follow non-terminal suppression and explicit `--progress`/`--quiet`/`--color` rules. Pathspec semantics including `--` separation, magic signatures where covered, and did-not-match diagnostics match the reference. Machine-readable modes (`--porcelain`, `--short`, `-z`, `--format`) are byte-identical where covered.

## Open Questions

- **Q1 — Spec 001 standing**: superseded as an execution plan; retained as historical context only. The behavioral framing governs. (Resolved by this document.)
- **Q2 — Product boundary**: the product is full Git including network, sequenced local-first with transport in Phase E; the plan overview's narrower "core-object-layer" wording describes the first deliverable, not the product. (Resolved by this document; the overview document should gain a pointer here.)
- **Q3 — Performance bar**: same order of magnitude stands; named benchmark repositories and factor limits are planning-track work. (Partially open; owned by planning.)
- **Q4 — Oracle strength**: the long-term target is the full upstream suite passing with no standard-Git process involved for in-scope commands; shim fallback is a development convenience, and "baseline 100%" means in-scope scripts only. (Resolved as target; infrastructure track owns the schedule.)
- **Q5 — Unported-command failure mode**: the shipped binary fails honestly; delegation to system Git lives only in the development/test shim, never as shipped behavior. (Resolved by this document.)
- **Q6 — Constitution**: governance rules (gates, deviation policy, no-FFI) should be ratified into the constitution file, which is currently an unfilled template. (Open; owned by project governance.)
- **Q7 — Empty `013` directory**: the path/attributes/ignore specification was reserved but never written. Either write it from the existing engine plus the differential-testing contract, or remove the reservation. (Open; owned by the next specification pass.)
- **Q8 — Scoreboard semantics**: failing-baseline entries need a machine-readable distinction between expected-failure records and passing gates so future agents cannot misread red as green. (Open; owned by the test-infrastructure track.)

## Explicit Non-Goals

No FFI or C linkage, ever. No line-by-line translation as methodology. No new user-facing options, formats, or behaviors invented beyond Git. No silent deviation — every divergence is logged. No legacy VCS bridges (`cvs*`, `svn`, `p4`, `quiltimport`, `archimport`). No GUI or web tooling (`git-gui`, `gitweb`, `instaweb`, `citool`). No mail senders (`send-email`, `imap-send`). No editor/tool front-ends (`mergetool`/`difftool` drivers as user tools; merge *drivers* as configured programs remain in scope per the diff/merge spec). No library-stability promise — crates are internal structure; the binary is the deliverable. No bit-identical human prose as a hard gate beyond the MUST/SHOULD split.

## Conflict Resolutions

- **001 vs locked strategy**: 001's file-by-file framing is superseded; the behavioral standalone-rewrite strategy governs (priority rule 1: explicit locked decision).
- **Network scope (overview "out of scope" vs Phase E + spec 010)**: network is in product scope sequenced last; "out of scope" is reinterpreted as out of the core-layer deliverable (rules 1 and 2).
- **Delegation vs honest failure (002 Q5)**: binary fails honestly; shim delegates in development only (rule 1: dispatcher design is an explicit repository decision).
- **Rename scoring (diff spec ideal vs implemented engine)**: implemented scoring stands as a logged known deviation with convergence owned by Phase A closure (rule 3: observable behavior plus test evidence over spec aspiration).
- **Non-deltified historical note vs deltified implementation**: implementation (deltified packs, crosswise-verified) supersedes older deferred-encoding notes (rule 3).
- **UTC-only historical note**: superseded by timezone-correct dates/idents now implemented (rule 3).
- **Collision detection (backlog vs implemented sha1dc)**: implemented and threaded through hashing and loose writes; backlog entries describing its absence are historical (rule 3).
- **Scoreboard red vs "done" claims**: test evidence wins over status prose; where suites are failing-baseline, features stay partial regardless of dispatcher presence (rule 3, and the core rule of this document).

## Status Register (Normative Summary)

| Area | Status | Evidence / Owner |
|---|---|---|
| Hashing incl. collision detection, dates, config engine, discovery + global context | Implemented | CW suites, `t/t0013`, `t/t0006`, `t/t1300`-family |
| Loose/pack/index-of-pack/MIDX-core/commit-graph-read object store | Implemented | pack + graph-midx CW, verify commands |
| Blob/tree/commit/tag model, revision operators, range walking | Implemented core | phase4 CW, `t/t1400`-family; reflog/time selectors partial |
| Diff core, status core, checkout/reset core, add/commit/init | Implemented core | phase CW suites, named `t/` scripts; deferred options listed in matrix |
| Index v2 + cached tree; refs loose/packed-read/symref/HEAD | Implemented core | phase6/7 CW; v3/v4, REUC, transactions pending |
| Merge-ort, sequencer, rebase, reflog write, packed-refs write, reftable | Planned | Specified in 006/008; gates in Phase C |
| Bitmaps, cruft, commit-graph write, MIDX full, gc/repack, algorithm conversion | Planned | Phase D |
| Transport (local→SSH→HTTPS, shallow, partial) | Specified | 010; Phase E, not implemented |
| Config/attributes/ignore engines | Implemented core | engine CW; `config` command + 013 content pending |
| Differential/crosswise infrastructure, shim, scoreboard, CI | Partially implemented | 012 specifies the target; fuzz/coverage/test-tool tracks pending |
| Legacy bridges, GUI/web, mail senders, tool front-ends | Intentionally unsupported | Non-Goals |

## Assumptions

- Evidence cited reflects the repository at specification time; statuses decay unless re-verified — any status challenged by newer test evidence loses to the evidence.
- The pinned reference version moves only as a deliberate, separately tested change; version-string sync is part of any such move.
- Pinned-version parity: defaults match the pinned standard Git; version-dependent formatting differences are recorded per surface.
- Platform defaults follow standard Git, including degradation where platforms lack symlinks, executables, or nanosecond timestamps.
- Dependencies stay pure-Rust and license-compatible with zero observable effect.
- The C tree's committed build artifacts are not evidence about the Rust implementation.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Any reader sampling ten features finds exactly one status, one evidence pointer, and zero contradicting claims per feature.
- **SC-002**: Every parity claim in the matrices resolves to a named green suite; every gap resolves to a deferred item, failing-baseline entry, or planned phase — zero presence-implied claims.
- **SC-003**: A new contributor completes a scoped task using this document plus the cited subsystem specification alone, with gates catching an injected deviation.
- **SC-004**: Conflicting scope statements across repository documents drop to zero for questions this document resolves (measured by Q1/Q2/Q4/Q5 having no competing authoritative answer).
- **SC-005**: Status-register accuracy holds across consecutive verification runs: no feature regresses status without a recorded cause, and promotions cite the suite that turned green.
- **SC-006**: End-to-end local workflows (create, stage, commit, inspect, branch, merge-foundations, pack, verify) run identically under either implementation on shared fixtures, then continue under the other with no repair step.
- **SC-007**: Open questions Q3 and Q6–Q8 each gain an owner and a resolution record within the planning track rather than lingering as prose.
