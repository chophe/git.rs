# Research: Rust Workspace Architecture

**Feature**: `specs/014-rust-architecture/spec.md` | **Date**: 2026-09-20

All Technical Context items were known or resolved to C-git parity during Clarifications (2026-09-20). Each entry below records the verification performed against the live tree, not an assertion.

## R-01: Dependency structure is acyclic and one-directional (FR-022)

- **Decision**: Keep the existing layering; new crates may depend only downward (surface → access → language → store → mid → foundation). `git-command` stays the sole broad dependent (composition root).
- **Rationale**: Machine-verified on 2026-09-20: 40 internal path-dependency edges across 18 members, topological check reports no cycles. Edge directions match the spec's layer map (e.g. `git-odb` → `git-hash/object/core/commitgraph`; `git-revision` → `object/odb/refs`; nothing points back upward).
- **Alternatives considered**: Re-layering `git-core` (discovery + `Repository`) apart — rejected: spec keeps existing homes; split would churn every consumer for no boundary gain.
- **Verify**: `cargo metadata --format-version 1` edge extraction + cycle check (see `contracts/dependency-rules.md`); re-run in gate.

## R-02: Zero-`unsafe` baseline holds (FR-028)

- **Decision**: Keep the zero-`unsafe` rule; any future `unsafe` requires written justification + safe wrapper + targeted tests + safety-gate pass.
- **Rationale**: Verified 2026-09-20: `grep -rn -w 'unsafe' --include='*.rs' crates/` (excluding `target/`) returns only the words inside userdiff regex literals — zero first-party `unsafe` blocks.
- **Alternatives considered**: Allowing `unsafe` for hot decompression/hash paths — rejected: no evidence of need; `flate2`/SHA-1 backends already encapsulate it outside first-party code.
- **Verify**: safety gate (`contracts/gates.md`).

## R-03: Single compression provider behind a facade (FR-007)

- **Decision**: New `git-compress` crate owns all zlib/deflate use (streaming encode/decode with size caps); `flate2` remains the single provider, becoming an implementation detail of the facade. `git-odb` migrates its direct `flate2` use to the facade.
- **Rationale**: Verified 2026-09-20: exactly one `Cargo.toml` in the workspace depends on `flate2` (`git-odb`). Facade removes the only direct coupling and gives loose/pack paths one documented boundary.
- **Alternatives considered**: (a) Leave `flate2` direct in `git-odb` — rejected: store and loose paths would keep embedding their own compression handling, violating FR-007. (b) Adopt an alternative backend — rejected per FR-030 (near-duplicate providers) with no demonstrated need.
- **Verify**: `grep -rln 'flate2' --include='Cargo.toml' crates/` must list only `git-compress`; streaming proven by oversized-fixture gate.

## R-04: Peak-memory bound = C-git parity (SC-004, FR-026)

- **Decision**: No fixed MB cap in this spec. Bulk paths (hash, compress, pack, delta, walk) stream with bounded memory; acceptance is peak RSS within the same order of magnitude as C git on identical fixtures (hundred-MB blob, deep delta chains, wide trees) plus byte-identical outputs.
- **Rationale**: Clarifications 2026-09-20 decision; matches constitution SHOULD-level performance ("same order of magnitude"). Structural property first, per-milestone numbers later.
- **Alternatives considered**: Flat cap (e.g. 256 MB RSS) — rejected: arbitrary across machines and repo shapes; would encode a number the project cannot defend. Proportional cap (≤2× largest object) — rejected: delta-chain resolution legitimately holds base + result windows; C git is the honest reference.
- **Verify**: oversized-fixture gate (`contracts/gates.md`).

## R-05: Exit-code classes = C-git parity (FR-024, SC-003)

- **Decision**: Per-component error enums map to C-git exit classes — 0 success, 1 generic/unknown-command, 129 usage, 128+ fatal/signal behavior — with layer attribution preserved end to end.
- **Rationale**: Clarifications 2026-09-20; matches porting rules (usage → 129, unknown command → 1) and constitution MUST-level exit-code verification.
- **Alternatives considered**: Minimal trio (0/1/129) — rejected: drops fatal/signal fidelity the `t/` oracle checks. Per-command codes — rejected: breaks uniform fault-injection attribution.
- **Verify**: fault-injection drills, one fault per layer (`contracts/error-contract.md`, `quickstart.md`).

## R-06: Observability = typed errors in libraries, rendering at the edge (FR-024)

- **Decision**: Libraries return typed errors only; all human diagnostics/stderr rendering live at the CLI/surface edge. No metrics/tracing beyond what C git emits.
- **Rationale**: Clarifications 2026-09-20 ("same as original C"); preserves stand-alone testability (no hidden global I/O) and keeps scripting contracts off display text (porcelain/plumbing split).
- **Alternatives considered**: Dedicated observability component with metrics/tracing — rejected: Git lacks it; inventing signals violates Principle V and adds global-state temptation.
- **Verify**: stand-alone component tests in bare dir with scrubbed env (SC-002 gate).

## R-07: Credentials scope = C-git behavior, nothing invented (FR-019)

- **Decision**: `git-credentials` owns lookup, caching, prompting, and redaction mirroring C git's helper protocol and observable behavior. Secrets never flow through config values, logs, or errors beyond what C git exposes.
- **Rationale**: Clarifications 2026-09-20; Behavioral Fidelity forbids inventing a secret-management UX Git lacks.
- **Alternatives considered**: Dedicated redacting secret wrapper type across all boundaries — deferred, not rejected: may land as Rust-native internal if a leak drill demands it, but the observable contract stays C-parity.
- **Verify**: canary-secret fixtures asserting no secret bytes in config output/logs/errors (`quickstart.md`).

## R-08: Transport behavior = C-git parity (FR-015/FR-016)

- **Decision**: `git-transport` (connection/pack-negotiation/progress, depended on only by fetch/push-class commands) matches C git retry/timeout/failure/progress behavior on identical remotes; `git-protocol` (pktline/capabilities/version negotiation) is isolated so both test against canned byte streams.
- **Rationale**: Clarifications 2026-09-20; keeps the dependency arrow one-directional (store never depends on transport — the cyclic-dependency edge case).
- **Alternatives considered**: Explicit retry-with-backoff/resume policy — rejected for this spec: a new network policy is behavior invention; revisit when fetch/push land, behind the same boundary.
- **Verify**: canned-stream protocol tests + crosswise fetch/push suites when those commands land.

## R-09: Missing-slot placements (FR-011, FR-017, FR-018 + SC-005)

- **Decision**: Reserve `git-pathspec` (magic/globs/exclusions/NUL input), `git-worktree` (materialization extracted from checkout/restore/reset/status paths), `git-hooks` (discovery/env/skip-force policy, absent-hooks = identical behavior), each with documented allowed dependencies in `contracts/placement.md`. Probe implementations must compile with zero edits to neighbors.
- **Rationale**: Six-plus-one boundaries have no home today (pathspec ad-hoc per command; worktree logic inside command modules; hooks/transport/protocols/credentials absent; compression direct). Reserved slots stop convenient-but-coupling accretion.
- **Alternatives considered**: Extracting worktree/pathspec logic immediately into full implementations — rejected: behavior moves risk scoreboard regressions; this plan reserves contracts and probes, deferring extraction to subsystem tasks.
- **Verify**: placement-probe drill (`contracts/gates.md`, `quickstart.md`).
