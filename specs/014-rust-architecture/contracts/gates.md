# Contract: Mechanical Gates

**Feature**: `specs/014-rust-architecture/spec.md` (SC-002, SC-003, SC-004, SC-006; FR-028, FR-030) | **Date**: 2026-09-20

Every gate below runs from `crates/` (the real workspace; the root `Cargo.toml` is a stale leftover) and MUST name the offending change on failure. All gates green is the merge precondition alongside the phase differential suites and the committed `scoreboard.json` baseline (regenerated only via `cargo xtask scoreboard` on intentional behavior change).

| Gate | Command / check | Pass criterion |
|---|---|---|
| workspace tests | `cargo test --workspace` | green (unit + doc + property tests) |
| dependency check | internal-edge extraction + cycle/layer check per `contracts/dependency-rules.md` | zero cycles, zero layer violations, zero undeclared use |
| safety gate | first-party `unsafe`-block scan + justification audit | zero unjustified `unsafe` (baseline: zero blocks); each allowed one carries rationale + safe wrapper + invariants + targeted tests |
| MSRV build | warning-free build at rust 1.74 (`rust-version = "1.74"`) | zero warnings; new deps show need + GPL-2.0 compatibility + MSRV impact (FR-030) |
| isolation (SC-002) | each component's tests in a bare dir, scrubbed env | pass identically; inputs as plain values, no shell-outs, no cross-component fixtures |
| fault drills (SC-003) | one injected fault per layer | 100% attribute the failing layer with exit class preserved (`contracts/error-contract.md`) |
| streaming (SC-004) | oversized fixtures (hundred-MB blob, deep delta chains, wide trees) | peak memory within same order of magnitude as C git + byte-identical outputs |
| placement probes (SC-005) | probe impl per missing slot (`git-compress/pathspec/worktree/transport/protocol/hooks/credentials`) | compiles with zero edits to neighbors; 21/21 boundaries uniquely owned |
| differential | `cargo xtask differential` | byte-identical stdout/stderr/exit vs system C git |
| scoreboard | `cargo xtask scoreboard` | no regression vs committed `crates/scoreboard.json` |

Introducing one violation per gate (unjustified `unsafe`, cycle, MSRV-incompatible construct, copyleft-incompatible dep) MUST fail the corresponding gate naming the violation (User Story 6 acceptance).
