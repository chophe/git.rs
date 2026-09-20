# Quickstart: Validating the Workspace Architecture

**Feature**: `specs/014-rust-architecture/spec.md` | **Date**: 2026-09-20

All commands run from `crates/` (the real workspace — never the repo root). Each scenario maps to its contract; details live there, not here.

## Prerequisites

- Rust toolchain covering MSRV 1.74 (workspace pins `rust-version = "1.74"`, edition 2021).
- System C git available (crosswise oracle; `SYSTEM_GIT` override supported).
- Fresh clone state: `crates/scoreboard.json` unmodified (it is the regression baseline).

## 1. Prove a new command touches nothing unrelated (Story 1)

1. Add a probe command module in `git-command/src/` + one `dispatch` entry + shim list entry + one crosswise suite.
2. Run `cargo build --workspace` and `cargo test --workspace` — everything else green, no other suite changes outcome.
3. Run the dependency check — expect exactly one new edge set (probe → libraries used), zero new library-to-library edges.
4. Remove the probe. (See `contracts/dependency-rules.md`, `contracts/gates.md`.)

## 2. Prove components test in isolation (Story 2)

1. For each foundation/mid/store crate, run its tests stand-alone in a bare temp dir with a scrubbed environment.
2. Expect identical passes with no repo fixtures and no env dependence; parsers/serializers additionally hold no-panic + round-trip on arbitrary bytes.
3. Any failure to run bare is a hidden-coupling defect, not a test-environment issue. (See `contracts/gates.md` isolation row.)

## 3. Trace one fault per layer (Story 3)

1. Inject: bad object bytes, missing ref, corrupt index, bad config (one per layer).
2. Expect each surfaced error to name the failing layer and subject with the C-git exit class preserved (0/1/129/128+ per `contracts/error-contract.md`).
3. Inspect `RepoContext` flow: all context arrives as explicit values from the CLI edge — reproducible by passing the same values in tests.

## 4. Stream oversized fixtures (Story 4)

1. Generate fixtures: hundred-MB blob, deep delta chain, wide tree (`cargo xtask gen-fixtures`).
2. Hash/store the blob; resolve the chain; walk the tree — via both the Rust binary (`crates/target/debug/git`) and system C git.
3. Expect byte-identical outputs with Rust peak memory within the same order of magnitude as C git. Fixed MB caps do not apply. (See `research.md` R-04.)

## 5. Place the next subsystem without restructuring (Story 5)

1. Pick a reserved slot (`git-credentials`, `git-transport`, `git-hooks`, `git-worktree`, `git-pathspec`, `git-compress`, `git-protocol`).
2. Scaffold a probe in the documented slot per `contracts/placement.md` (owner, allowed deps, boundary data).
3. Expect compilation with zero edits to neighboring components and exactly one owner per boundary (21/21).

## 6. Audit gates mechanically (Story 6)

1. `cargo test --workspace`, `cargo xtask differential`, `cargo xtask scoreboard` — all green, no baseline edit.
2. Negative probes (do not commit): one unjustified `unsafe`, one dependency cycle, one MSRV-incompatible construct, one incompatible license — each MUST fail its gate naming the violation, then revert.
3. Expected end state: `git status` shows only the intended `specs/014-rust-architecture/` artifacts; no `scoreboard.json` diff, no `target/` artifacts committed.
