# Implementation Plan: Rust Workspace Architecture

**Branch**: `014-rust-architecture` | **Date**: 2026-09-20 | **Spec**: `specs/014-rust-architecture/spec.md`

**Input**: Feature specification from `specs/014-rust-architecture/spec.md` (plus Clarifications session 2026-09-20: memory/exit-code/transport/credentials/observability all resolve to C-git parity)

## Summary

Keep the existing 17-crate + `xtask` workspace exactly as observed (verified acyclic, zero `unsafe`, path-only deps, edition 2021 / MSRV 1.74 / GPL-2.0-only) and place the seven missing boundaries as new reserved crates with documented owners, allowed dependencies, and boundary data: `git-compress` (facade over the single vetted `flate2` provider), `git-pathspec`, `git-worktree`, `git-transport`, `git-protocol`, `git-hooks`, `git-credentials`. No existing crate is reorganized; new edges run only toward lower layers; conformance is enforced by mechanical gates (dependency check, safety gate, MSRV build, exit-code/fault drills, oversized-fixture streaming check) defined in `contracts/gates.md`.

## Technical Context

**Language/Version**: Rust, edition 2021, MSRV 1.74 (pinned in `crates/Cargo.toml`; verified toolchain present builds it)

**Primary Dependencies**: `flate2` (sole compression provider, used only by `git-odb` today — becomes an implementation detail of the new `git-compress` facade); no new external crates proposed (FR-030: near-duplicate providers rejected; any future addition records need/license/MSRV first)

**Storage**: N/A (no new storage; on-disk formats stay byte-compatible per constitution — loose/pack/idx/midx/refs/index layouts unchanged, owned by existing components)

**Testing**: `cargo test --workspace` (run from `crates/`), `proptest` for parser/serializer properties, differential/crosswise suites via `cargo xtask differential`, `t/` scoreboard via `cargo xtask scoreboard` (no-regression baseline in `crates/scoreboard.json`)

**Target Platform**: Wherever C git builds and Rust 1.74 builds: Linux/macOS primary, no platform-specific code proposed

**Project Type**: CLI workspace (libraries + thin binary dispatcher + automation xtask)

**Performance Goals**: C-git parity per Clarifications 2026-09-20 — peak memory on bulk paths (hash/compress/pack/delta/walk) within the same order of magnitude as C git on identical fixtures; no fixed MB cap in this spec; per-milestone budgets may tighten at task time

**Constraints**: Acyclic surface → access → language → store → mid → foundation dependencies (violation fails the gate); no process-global reads past the CLI edge; per-component error enums mapped to C-git exit-code classes (0/1/129/128+); zero unjustified `unsafe`; warning-free MSRV build; GPL-2.0-only

**Scale/Scope**: 17 existing crates + `xtask` kept; 7 new reserved crates scaffolded (facade/placement only — full subsystem behavior lands in later features); 40 existing internal edges verified acyclic; all 21 boundaries resolve to exactly one owner (SC-005)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Behavioral Fidelity**: PASS — plan invents no behavior; every boundary contract (exit codes, redaction, retry/progress, memory profile) is defined as C-git parity with the `t/` suite as oracle. No compatibility boundary declared.
- **II. Standalone Implementation**: PASS — no FFI, no linking, no calls into C git; C tree remains reference/oracle only. New crates are pure Rust.
- **III. Evidence Over Assertion**: PASS — every structural claim is pinned to a mechanical gate in `contracts/gates.md` (dependency check, safety gate, fault-injection drills, oversized fixtures, probe-command/placement drills). Presence in dispatch never implies parity.
- **IV. Explicit Boundaries, Honest Failure**: PASS — missing subsystems land as explicitly incomplete reserved slots; unimplemented paths fail honestly (unknown command / usage 129 / unsupported-capability). Known deviations stay logged in `docs/plan/FOLLOWUPS.md`, not silently fixed.
- **V. Rust-Native Internals, Fixed Observables**: PASS — crate splits, handle types, error plumbing are Rust-native and free; output bytes, exit codes, and on-disk formats are fixed and verified by differential/crosswise suites.
- **Compatibility Requirements (MUST)**: atomic writes preserved (ref transactions, index locks, config writes stay with owners); total parsers + pre-acceptance validation preserved (FR-029 property tests per component).
- **Development Workflow**: phase gates in `contracts/gates.md` are machine-checkable; scoreboard baseline untouched by this plan (scaffolding adds no behavior).

Post-Phase-1 re-check: PASS — `research.md` resolved all unknowns as C-git parity with verification commands; `data-model.md` introduces no new on-disk state; `contracts/` pins only ownership/directions/diagnostics already required by the spec; `quickstart.md` validates exclusively through existing gates and suites. No new violations; Complexity Tracking stays empty.

## Project Structure

### Documentation (this feature)

```text
specs/014-rust-architecture/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md        # Phase 1 output (/speckit-plan command)
├── quickstart.md        # Phase 1 output (/speckit-plan command)
├── contracts/           # Phase 1 output (/speckit-plan command)
│   ├── placement.md         # 21 boundaries → exactly one owner each
│   ├── dependency-rules.md  # layer order + violation semantics
│   ├── error-contract.md    # per-component errors → C-git exit classes
│   └── gates.md             # mechanical gates + how each runs
└── tasks.md             # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

### Source Code (repository root)

```text
crates/
├── Cargo.toml            # workspace manifest (resolver v2, inherited edition/MSRV/license)
├── git-hash/ git-varint/ git-date/          # foundation (no internal deps)
├── git-config/ git-object/ git-core/        # mid primitives + discovery/Repository
├── git-commitgraph/ git-diff/ git-merge/
├── git-index/ git-attributes/ git-pretty/
├── git-odb/ git-refs/ git-revision/        # store + walking/formatting
├── git-compress/       # NEW reserved: zlib/deflate facade over flate2
├── git-pathspec/       # NEW reserved: magic/globs/exclusions/NUL input
├── git-worktree/       # NEW reserved: materialization (extract from command modules)
├── git-transport/      # NEW reserved: connection/pack-negotiation/progress
├── git-protocol/       # NEW reserved: pktline/capabilities/version negotiation
├── git-hooks/          # NEW reserved: discovery/env/skip-force policy
├── git-credentials/    # NEW reserved: lookup/cache/prompt (C-git scope)
├── git-command/        # composition root: one module per command + dispatch
├── git-cli/            # thin binary: global options → dispatch → exit code
├── xtask/              # differential / gen-fixtures / scoreboard automation
└── target/             # build dir (not committed)

tests live inside each crate (unit + `tests/` integration);
crosswise suites under crates/*/tests/ registered in xtask::suites()
```

**Structure Decision**: Keep the existing workspace layout untouched; add exactly the seven reserved crates above as thin placement scaffolds (types + boundary signatures + tests pinning allowed dependencies). No existing crate moves, splits, or gains a new library-to-library edge. `git-command` remains the sole composition root allowed broad dependencies.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| — | — | — |

No violations. Seven new crates are reserved placements required by FR-001–FR-021/SC-005 (one owner per boundary), not speculative abstraction: each ships only its boundary contract plus a compiling probe, with full behavior deferred to later subsystem features.
