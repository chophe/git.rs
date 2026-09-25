# git.rs — Pure-Rust Rewrite of Git

## What This Is

Standalone pure-Rust reimplementation of git (upstream C git `v2.55.0` at repo root as reference/behavior oracle only, no FFI). The active work lives under `crates/` (24-member workspace, binary `git` from `git-cli`); on-disk formats stay byte-compatible so Rust and C git interoperate crosswise. For developers working on the Rust port and its differential/crosswise verification harness.

## Core Value

Byte-identical behavior with C git — same stdout/stderr/exit codes and crosswise-readable on-disk formats — verified by the `t/` oracle suite and crosswise tests.

## Requirements

### Validated

- ✓ Thin CLI entry with C-exact exit codes (129 usage, 1 unknown command, 141 SIGPIPE) — existing (`crates/git-cli/src/lib.rs`)
- ✓ Composition-root dispatch (`git_command::dispatch`, ~45 subcommands, `Command` trait, `RepoContext`) — existing (`crates/git-command/src/lib.rs`)
- ✓ Foundation leaves: SHA-1/SHA-256 hashing, varint, dates, config layering, strbuf-faithful `StringBuf` — existing (`crates/git-hash/`, `git-varint/`, `git-date/`, `git-config/`, `git-core/`)
- ✓ Object store reads: loose objects, pack file/idx/midx, delta resolution — existing (`crates/git-odb/`, `crates/git-object/`)
- ✓ Revision queries: oid/ref/abbrev resolution, `~`/`^` peels, rev-walk — existing (`crates/git-revision/`)
- ✓ Refs (loose + packed-refs) and v2 index read/write — existing (`crates/git-refs/`, `crates/git-index/`)
- ✓ Diff/merge primitives: tree compare, Myers diff, unified render, merge bases — existing (`crates/git-diff/`, `crates/git-merge/`)
- ✓ Differential + crosswise harness (`cargo xtask differential`, `scoreboard`, `scripts/shim-git`) — existing (`crates/xtask/`)

### Active

- [ ] Continue porting remaining builtins to byte-identical parity (per `docs/plan/` phase-a/phase-b task lists)
- [ ] Hold all phase done-gates green: `cargo test --workspace`, proptests, differential byte-identical, crosswise compat, coverage, no scoreboard regression
- [ ] Establish GSD planning scaffold (PROJECT/REQUIREMENTS/ROADMAP/STATE) for this work

### Out of Scope

- Network/transport (fetch/push, Phase 10+) — explicitly out of the core-object-layer scope per `docs/plan/README.md`
- FFI into C git — standalone rewrite by locked decision, never link C
- C-tree changes except as reference/oracle reads — C is the spec, `t/` wins on disagreement

## Context

- Brownfield: full layered workspace already exists (foundation → mid → store → language/access → surface), mapped in `.planning/codebase/` (2026-09-25)
- C git tree at repo root (`builtin/`, `t/`, `Documentation/`) is read-only oracle; system C git at `/usr/bin/git` is the crosswise comparator
- Real workspace is `crates/Cargo.toml`; root `Cargo.toml` (`gitcore` staticlib) is a stale leftover — always run cargo from `crates/`
- `crates/scoreboard.json` is the committed `t/` regression baseline; `cargo xtask scoreboard` regenerates and fails on regression
- `scripts/shim-git` routes ported commands to `crates/target/debug/git`, everything else to system git
- Known intentional deviations logged in `docs/plan/FOLLOWUPS.md` (e.g. SHA-1 not yet collision-detecting in some paths, non-deltified pack writes, UTC-only dates) — do not silently "fix"
- Prior planning material (`docs/plan/`, `specs/`) intentionally not yet ingested (user skipped ingest); treat as source material for requirements/roadmap steps

## Constraints

- [Compatibility]: On-disk formats must round-trip with C git both directions — crosswise contract
- [Compatibility]: Match C exit codes and stderr text exactly (129/128/1/141); `t/` suite is the oracle on disagreement
- [Tech stack]: Rust edition 2021, MSRV 1.74; no async runtime; `unsafe` near-zero (gated by `cargo xtask safety`)
- [Architecture]: Strict downward dependency layering enforced by `cargo xtask depcheck`; `git-command` is the sole composition-root exception
- [Architecture]: No new crypto/CLI/serde/network deps without plan amendment (`flate2` only via `git-compress`, no `clap`/`serde`/`tokio`)
- [Process]: C-side commands run from repo root (`make`, `t/*.sh`); Rust commands run from `crates/`

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Standalone rewrite, no FFI | Upstream-acknowledged strategy; avoids binding maintenance | ✓ Good |
| C git is the spec, `t/` wins ties | Guarantees observable parity over source fidelity | ✓ Good |
| Thin CLI + `Command` trait per builtin | Mirrors C `builtin/` layout, keeps commands unit-testable | ✓ Good |
| GSD scaffold for this repo now | Need phased execution tracking for remaining port work | — Pending |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-09-25 after initialization*
