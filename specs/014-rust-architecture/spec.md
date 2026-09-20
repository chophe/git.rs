# Feature Specification: Rust Workspace Architecture

**Feature Branch**: `014-rust-architecture`

**Created**: 2026-09-20

**Status**: Draft

**Input**: User description: "Create the Rust architecture specification for git.rs. Design the project as a modular Rust workspace rather than a direct C-to-Rust translation. Identify appropriate boundaries for: CLI, configuration, repository discovery, repository storage, object database, hashing, compression, refs, index, revision parsing, pathspec, attributes, diff, merge, transport, packfiles, protocols, worktree, hooks, credentials, porcelain commands, plumbing commands. Requirements: minimize unnecessary coupling, avoid global mutable state, use explicit error types, use traits only where abstraction provides real value, prefer zero-copy/streaming approaches where appropriate, maintain strong ownership boundaries, preserve safety guarantees, make components independently testable, avoid unsafe code unless explicitly justified and approved, maintain compatibility with the project's existing Rust toolchain and workspace conventions. Inspect the current repository before proposing changes and integrate existing architecture rather than replacing it unnecessarily."

## Current Architecture *(observed, not proposed)*

The workspace (`crates/Cargo.toml`: edition 2021, MSRV 1.74, GPL-2.0-only, resolver v2, path-only internal dependencies) currently contains 17 library/binary crates plus `xtask`, layered as follows:

- Foundation (no internal deps): `git-hash`, `git-varint`, `git-date`.
- Mid: `git-config` (parsing only); `git-object` (model, over `git-hash`); `git-core` (discovery + `Repository`, over `git-hash` + `git-config`); `git-commitgraph`, `git-diff` (over `git-object`), `git-merge` (over `git-object` + `git-diff`), `git-index` (over `git-hash`), `git-attributes` (over `git-core` + `git-config`).
- Store: `git-odb` (loose/pack/idx/midx, over `git-hash` + `git-object` + `git-core` + `git-commitgraph`, compression via external `flate2`); `git-refs` (over `git-hash` + `git-core`); `git-revision` (walking, over object/store/refs); `git-pretty` (formatting, over `git-hash` + `git-date`).
- Surface: `git-command` (one module per command, `Command` trait + `dispatch` + `RepoContext` + `CommandError`, depends on all libraries); `git-cli` (thin binary: global-option parsing, delegates to `dispatch`).
- Automation: `xtask` (differential, fixtures, scoreboard).

Observed health: dependency directions are acyclic and point one way (surface → store → mid → foundation); first-party code contains zero `unsafe` blocks (the only textual matches are the words "unsafe"/"extern" inside userdiff regex literals); process-global mutation (`set_current_dir`) is confined to test harnesses; no global registries; errors are per-crate enums funneled into exit-coded command errors. Missing as components: transport, protocols, credentials, hooks, worktree (logic lives inside command modules), pathspec (ad-hoc per command), compression (external crate used directly, no internal boundary).

This spec keeps that architecture and places the missing pieces; it does not redesign what exists.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Add a command without touching unrelated components (Priority: P1)

A contributor ports a new command by adding one module plus a dispatch entry, touching only the libraries that command genuinely needs, with the build and the rest of the test suite unaffected by the addition.

**Why this priority**: Command throughput is the project's velocity bottleneck. If adding a command requires edits across unrelated components, every port carries regression risk proportional to the whole workspace.

**Independent Test**: Can be fully tested by adding a probe command depending on a single library and asserting the change set touches only the new module, the dispatch table, the shim list, and its suite — plus a dependency check proving no new edges between existing libraries.

**Acceptance Scenarios**:

1. **Given** a new command needing only object reads and ref resolution, **When** it is added, **Then** it compiles against the existing store/ref interfaces with zero changes to those components, and no other command's tests change outcome.
2. **Given** the finished addition, **When** the dependency check runs, **Then** it reports exactly one new edge set (command module → the libraries used) and zero new library-to-library edges.

---

### User Story 2 - Test a component in isolation (Priority: P1)

A contributor tests hashing, config parsing, diff, or merge logic with unit tests inside that component alone — no repository fixture, no process environment, no other component linked — and trusts that passing in isolation predicts passing in composition.

**Why this priority**: Isolation is what makes failures local. Components that can only be tested through end-to-end runs produce failures attributable to no particular layer.

**Independent Test**: Can be fully tested by running each foundation/mid/store component's test target stand-alone (no fixture repos, no env vars set) and by asserting each component's tests construct inputs as plain values, never by shelling out.

**Acceptance Scenarios**:

1. **Given** any foundation or mid-layer component, **When** its tests run in a bare temporary directory with a scrubbed environment, **Then** they pass identically, proving no hidden dependence on repositories or global state.
2. **Given** a parser/serializer component, **When** arbitrary bytes are supplied, **Then** the documented no-panic and round-trip properties hold without consulting any other component.

---

### User Story 3 - Trace any failure to its owning layer (Priority: P2)

A contributor debugging a wrong output follows the error from the CLI surface down through exactly one path — command module → named libraries → explicit error variants — and finds the single component whose contract was violated, with no "it could be anywhere" global state to audit.

**Why this priority**: Explicit errors and owned context are the debugging story. Global mutable state and untyped errors turn every bug into a whole-workspace investigation.

**Independent Test**: Can be fully tested by injecting one fault per layer (bad object bytes, missing ref, corrupt index, bad config) and asserting the surfaced error names the failing layer and cause, with the exit-code class preserved end to end.

**Acceptance Scenarios**:

1. **Given** a corrupt object in the store, **When** a command reads it, **Then** the reported error identifies the store layer and the object, not a generic failure, and the exit code matches the declared class.
2. **Given** any command invocation, **When** repository context (directories, overrides, config) is inspected, **Then** it arrives as explicitly passed values from the CLI edge — reproducible by passing the same values in tests, with no hidden process-global reads past the edge.

---

### User Story 4 - Move large data without copying it (Priority: P2)

A user packing, hashing, or diffing gigabyte-scale content sees bounded memory use because bulk paths stream bytes (hash while reading, inflate while resolving, walk without materializing whole histories), identically verified by both implementations' outputs.

**Why this priority**: Git repositories routinely exceed memory. Copy-per-layer designs pass small-fixture tests and fail on real repositories; streaming must be a structural property, not a per-command afterthought.

**Independent Test**: Can be fully tested with oversized fixtures (multi-hundred-megabyte blob, deep pack chain, wide tree) asserting bounded peak memory on bulk paths plus byte-identical outputs to standard Git.

**Acceptance Scenarios**:

1. **Given** a large blob, **When** it is hashed and stored, **Then** peak memory stays bounded (streaming hash + streaming compression) and the resulting ID and bytes match standard Git.
2. **Given** a large pack, **When** objects are read through delta chains, **Then** resolution streams base application without materializing the whole pack, and every object matches the opposite implementation's read.

---

### User Story 5 - Place the next subsystem without restructuring (Priority: P2)

A contributor starting transport, credentials, or hooks support finds a reserved, documented slot — which component owns it, what it may depend on, and what crosses its boundary — and implements there without moving existing code.

**Why this priority**: The 21 requested boundaries include six with no home yet (transport, protocols, credentials, hooks, worktree-as-component, pathspec-as-component, plus compression-as-boundary). Unplaced work accretes wherever is convenient, which is how coupling grows.

**Independent Test**: Can be fully tested by writing the placement contract for each missing subsystem (owner, allowed dependencies, boundary data) and asserting a probe implementation in the documented slot compiles with no edits to neighboring components.

**Acceptance Scenarios**:

1. **Given** the placement map, **When** a new subsystem (e.g. credential lookup) is scaffolded in its slot, **Then** it depends only on its documented inputs and no existing component changes.
2. **Given** all 21 boundaries, **When** each is asked "which component owns this", **Then** exactly one answers, with no overlaps and no gaps.

---

### User Story 6 - Audit safety and toolchain conformance mechanically (Priority: P3)

A reviewer verifies safety (`unsafe` justification), dependency hygiene (acyclic, minimal), and toolchain conformance (edition, MSRV, license, dependency policy) by running checks, not by reading the whole tree.

**Why this priority**: Guarantees that live only in documentation decay. Mechanical gates keep the architecture properties true as the workspace grows.

**Independent Test**: Can be fully tested by introducing one violation per gate (an unjustified `unsafe`, a dependency cycle, an MSRV-incompatible construct, a copyleft-incompatible dependency) and asserting each gate fails naming the violation.

**Acceptance Scenarios**:

1. **Given** the current tree, **When** all gates run, **Then** they pass: zero unjustified `unsafe`, zero cycles, MSRV build clean, license/dependency policy clean.
2. **Given** a proposed dependency on a new external crate, **When** it is evaluated, **Then** the decision record shows need, license compatibility, and MSRV compatibility before the dependency lands.

---

### Edge Cases

- Cyclic-dependency temptation: revision walking needs store + refs while future fetch needs walking — the placement MUST keep the arrow one-directional (access layers depend on language/store layers, never the reverse).
- `Repository` handle growth: new context (worktree identity, transport endpoint, credential helper) MUST extend the explicitly-passed context, never add process-global lookups.
- Error-variant explosion: per-component enums MUST stay flat and specific; shared "misc/other" variants MUST NOT become catch-alls that erase layer attribution.
- Trait temptation: new abstractions (storage backends, hash providers, transports) MUST be concrete types first; a trait is introduced only with two real implementations or a test-double need, recorded with rationale.
- Streaming boundaries: APIs that look streaming but buffer whole inputs internally MUST be documented as buffering; callers MUST NOT assume bounded memory from a streaming-shaped signature.
- Test-only coupling: test helpers shared across components MUST live in exactly one documented place with no production dependency on them; production code MUST NOT depend on test utilities.
- External dependency risk: each new external crate expands supply-chain and MSRV surface; near-duplicate functionality MUST reuse the vetted crate (e.g. one compression provider, one CLI parser) rather than adding alternatives.
- Binary size and build time: new components MUST NOT regress clean-build time or binary size beyond declared budgets without a recorded decision.
- Porcelain/plumbing split: human-oriented formatting MUST live above the machine-readable core so scripting contracts never depend on display text; a display change MUST NOT alter plumbing output (verified by the differential boundary suites).

## Requirements *(mandatory)*

### Functional Requirements

Component boundaries (all 21 requested areas placed; existing homes kept):

- **FR-001 (CLI)**: The `git-cli`-equivalent surface MUST stay thin: global-option parsing (`-C`, `-c`, `--git-dir/--work-tree/--bare`), dispatch to exactly one command module, exit-code propagation. It MUST contain no repository, storage, or formatting logic.
- **FR-002 (configuration)**: The config component MUST own file parsing, layered merging, typed reads, and origin attribution, usable by every other component with no per-consumer re-parsing; it MUST NOT depend on repository discovery (discovery supplies file paths; config never finds repositories).
- **FR-003 (repository discovery)**: The discovery component MUST own upward search, `gitdir:`/`commondir` following, bare detection, and environment/file override resolution, producing an explicit context value; it MUST NOT read object, ref, or index content.
- **FR-004 (repository storage)**: The `Repository`-owning component MUST own layout knowledge (control paths, common vs per-worktree files, algorithm selection) and hand out scoped store handles; it MUST NOT implement object/ref/index formats itself — it routes to the owning components.
- **FR-005 (object database)**: The ODB component MUST own loose objects, alternates, and quarantine visibility; packfile/index/midx/commit-graph specifics live behind its read interface so callers ask "give me object X" without knowing its physical location.
- **FR-006 (hashing)**: The hash component MUST own algorithm selection, ID parsing/formatting/validation, streaming and one-shot digest computation, and collision-detecting SHA-1 semantics; no other component may implement its own digest or ID validation.
- **FR-007 (compression)**: All zlib/deflate use MUST go through one documented compression boundary (single provider, streaming encode/decode with size caps); store and loose paths MUST NOT each embed their own compression handling.
- **FR-008 (refs)**: The refs component MUST own loose/packed storage, symref following, transactions, locking, reflogs, and packed-refs maintenance; branch/tag commands MUST reach refs only through its operations, never by writing ref files directly.
- **FR-009 (index)**: The index component MUST own entry encoding (versions, extensions, checksum), freshness comparison, and atomic read/write; work-tree scanning supplies stat/content facts, and the index decides freshness — scanning logic MUST NOT duplicate freshness rules.
- **FR-010 (revision parsing)**: One resolver MUST own revision-expression parsing and disambiguation for all commands (IDs, abbreviations, ref expressions, ranges, pathspecs-of-history); command modules MUST NOT implement private revision grammars.
- **FR-011 (pathspec)**: Path filtering MUST live in one component (magic, globs, exclusions, NUL-separated input) consumed by every path-taking command; per-command ad-hoc glob code MUST NOT exist.
- **FR-012 (attributes)**: Ignore/attribute evaluation (ignore stacking, attribute lookup, streaming filter decisions) MUST live in one component consumed by status/add/checkout/diff paths; matching semantics MUST NOT be reimplemented per consumer.
- **FR-013 (diff)**: The diff component MUST own similarity engines, hunk rendering, rename detection, and binary handling; commands select options and print results — they MUST NOT contain comparison algorithms.
- **FR-014 (merge)**: The merge component MUST own merge-base computation, line-level merging, and conflict representation; index/ref updates resulting from merges go through the index/refs components, never via direct file writes from merge code.
- **FR-015 (transport)**: Network and local data-movement (connection handling, pack negotiation, progress) MUST live in a dedicated component that depends on store/language layers and is depended on by fetch/push-class commands only; store components MUST NOT depend on transport.
- **FR-016 (protocols)**: Wire encoding (packet lines, capability advertisement, protocol-version negotiation) MUST be isolated from transport flow control so each can be tested with canned byte streams and neither leaks framing details into command logic.
- **FR-017 (worktree)**: Work-tree materialization (file creation, mode/symlink handling, stat refresh, sparse patterns) MUST live in one component used by checkout/restore/reset/status paths; index entries and work-tree bytes MUST meet only at its interface.
- **FR-018 (hooks)**: Hook discovery, execution environment, and skip/force policy MUST live in one component; command flows call explicit hook points and MUST behave identically (modulo the hook's own effects) when hooks are absent.
- **FR-019 (credentials)**: Secret lookup, caching, and redaction MUST live in one component; secrets MUST NOT flow through config values, logs, or error messages, and no other component may implement its own credential prompting.
- **FR-020 (plumbing commands)**: Plumbing modules MUST compose library components into stable machine-readable behavior, each depending only on the libraries its contract needs; shared parsing/output helpers MUST be explicit modules, not copy-pasted snippets.
- **FR-021 (porcelain commands)**: Porcelain MUST be built on top of the same library components and plumbing contracts (never on display text of other commands); human formatting MUST be isolated so display changes cannot alter scripted behavior.

Structural rules:

- **FR-022 (acyclic minimal coupling)**: The dependency graph MUST be acyclic with arrows pointing surface → access → language → store → mid → foundation (see FR-001–FR-021 placements); a new edge that creates a cycle or a layer violation MUST fail the dependency check. Each component MUST declare its dependencies; undeclared use MUST fail the build's interface check.
- **FR-023 (no global mutable state)**: Process environment and working directory MUST be read once at the CLI edge into explicitly passed context values; library code MUST receive what it needs as arguments. Test-only process-global mutation MUST stay confined to test harnesses with serializing guards, never in production paths.
- **FR-024 (explicit errors)**: Every component MUST expose its own error enum with specific variants (I/O, not-found, corrupt, ambiguous, locked, invalid) convertible to the surfaced exit-code classes without losing layer attribution; untyped catch-all variants MUST NOT be used for new errors.
- **FR-025 (traits with justification)**: New traits MUST be introduced only with a recorded justification — at least two real implementations or a genuine test-double need (the existing per-command dispatch trait is the precedent pattern); single-implementation abstraction speculation MUST be rejected in review.
- **FR-026 (zero-copy/streaming)**: Parsers MUST offer borrowed views over input bytes where the data is retained by the caller; bulk paths (hash, compress, pack, delta, walk) MUST stream with bounded memory; buffering APIs MUST be labeled as buffering so callers cannot mistake them for streaming.
- **FR-027 (ownership)**: Handles MUST own their scope: repository handles own paths/config, store handles borrow the repository scope, command invocations own their output sinks and contexts. Cross-component shared mutation MUST go through the owning component's atomic operations (ref transactions, index locks, config writes), never through shared mutable references.
- **FR-028 (safety)**: First-party code MUST contain zero `unsafe` blocks (current baseline: zero — the only textual matches are regex literals naming other languages' keywords); any future `unsafe` MUST carry a written justification, a safe wrapper with documented invariants, and targeted tests, and MUST pass the safety gate — otherwise the build fails.
- **FR-029 (testability)**: Every component MUST be testable stand-alone: constructible from plain values, runnable with no fixture repository and no environment dependence, with parser/serializer properties (no-panic, round-trip) checked inside the owning component.
- **FR-030 (toolchain and conventions)**: The workspace MUST keep the existing conventions — single workspace manifest, inherited package keys (edition, MSRV 1.74, GPL-2.0-only), path-only internal dependencies, resolver v2 — and every crate MUST build warning-free at MSRV. New external dependencies MUST record need, license compatibility, and MSRV impact before landing; near-duplicate providers of an already-covered capability MUST be rejected.

### Key Entities

- **Component**: One workspace crate with a single owning responsibility, declared dependencies, an explicit error type, and stand-alone tests.
- **Layer**: Foundation (hash/varint/date/config-primitives) → mid (object/config/discovery primitives) → store (object/ref/index storage) → language (revision/diff/merge/attributes/pathspec) → access (transport/protocols/credentials/hooks/worktree) → surface (plumbing, porcelain, CLI).
- **Repository handle**: The explicitly passed context (directories, bare flag, algorithm, merged config, overrides) every operation receives instead of reading process globals.
- **Store handle**: A scope-borrowed accessor (object/ref/index) that performs atomic operations on behalf of callers.
- **Boundary contract**: The documented data crossing between two components (values in, verdicts out) that conformance tests pin.
- **Composition root**: The command-dispatch layer where libraries meet invocations; the only place allowed to depend broadly.
- **Safety gate**: The mechanical check enforcing zero-unjustified-`unsafe`, acyclic dependencies, MSRV build, and dependency policy.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Contributors complete routine command ports touching only the new module, dispatch table, shim list, and suite — 90% of ports require zero edits to existing library components.
- **SC-002**: Every component's tests pass stand-alone in a bare directory with a scrubbed environment; no component's tests require another component's fixtures or a live repository layout.
- **SC-003**: Fault-injection drills (one fault per layer) attribute correctly 100% of the time: the surfaced error names the failing layer and preserves the exit-code class end to end.
- **SC-004**: Oversized fixtures (hundred-megabyte blob, deep delta chains, wide trees) process with bounded peak memory on bulk paths while producing byte-identical outputs to standard Git.
- **SC-005**: All 21 requested boundaries resolve to exactly one owning component with zero overlaps and zero gaps, and probe implementations in new slots compile without touching neighbors.
- **SC-006**: All mechanical gates pass continuously: zero unjustified `unsafe`, zero dependency cycles or layer violations, warning-free MSRV build, clean dependency policy — with every introduced violation caught naming the offending change.
- **SC-007**: New contributors locate the right component for a bug or feature on first attempt in 80% of trials, measured by routing issues/PRs without reassignment.

## Assumptions

- The existing 17-crate layout plus `xtask` is the starting point; this spec places missing pieces and constrains growth rather than reorganizing what exists.
- Standard Git behavior (test suite as oracle on disagreements) defines correctness; architecture defines where correctness is implemented, not what it is.
- MSRV 1.74, edition 2021, GPL-2.0-only licensing, and path-only internal dependencies remain fixed; changing any of them is a separate ratified decision, not a per-feature choice.
- Performance budgets (build time, binary size, peak memory on bulk paths) are declared per milestone at planning time; structural properties (acyclicity, explicit errors, no global state) gate before optimization.
- Network-dependent subsystems follow the same boundary rules when they land; their absence today does not exempt their reserved slots from the dependency directions stated here.
