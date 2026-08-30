# Plan: Convert git (C) to Rust — Standalone Core-Object-Layer Rewrite

This directory contains the conversion plan for reimplementing git in Rust as a
**standalone, pure-Rust** project. The C tree in this repository (`git.git` at
v2.55.0-540, including the official initial Rust port under `src/`) is treated
exclusively as:

- **reference implementation** (read the C to understand behavior), and
- **behavior oracle** (run the C binary and the `t/` suite to verify the Rust
  port).

There is **no FFI**: the Rust implementation never calls into C git, and the C
git binary is never linked into the Rust build. On-disk formats must remain
byte-compatible so both implementations can operate on the same repositories
crosswise.

## Strategic decisions (locked)

| Decision | Choice |
|---|---|
| Strategy | Standalone rewrite (gitoxide-style), no FFI |
| Scope | Core object layer first: hash, object store (loose/pack/midx/bitmap), refs, index, revision walking, diff |
| Plan format | Phased roadmap with milestones; detailed per-phase blueprints |
| Deliverable location | `docs/plan/` |

## Phase map

| Phase | Title | Crate(s) | Part |
|---|---|---|---|
| [Phase 0](phase-0-foundation.md) | Foundation | git-hash, git-date, git-config, git-core, git-cli | **Part 1** |
| [Phase 1](phase-1-loose-objects.md) | Loose objects | git-odb, git-object, git-varint | **Part 1** |
| [Phase 2](phase-2-packs-idx.md) | Packs & idx | git-odb (pack/idx/revindex/delta) | |
| [Phase 3](phase-3-midx-bitmaps-commitgraph.md) | MIDX, bitmaps, commit-graph, cruft | git-odb, git-commitgraph | |
| [Phase 4](phase-4-object-model-revision.md) | Object model & revision walking | git-object, git-revision | |
| [Phase 5](phase-5-diff.md) | Diff | git-diff | |
| [Phase 6](phase-6-index-worktree.md) | Index & worktree | git-index, git-core | |
| [Phase 7](phase-7-refs-reftable.md) | Refs & reftable | git-refs | |
| [Phase 8](phase-8-merge-reachability.md) | Merge machinery & reachability | git-merge | |
| [Phase 9](phase-9-object-conversion-fsck.md) | Object conversion & fsck | git-odb, git-fsck | |

Phase 10+ (network/transport, `git apply`/`am`, submodules, blame, stash,
bisect, all remaining builtins) is explicitly **out of the core-object-layer
scope** and is not planned here beyond a stretch note.

## Dependency order

```
Phase 0 (foundation)
   └─▶ Phase 1 (loose objects)          needs: git-hash, git-config, git-core
   └─▶ Phase 2 (packs & idx)            needs: Phase 0
   └─▶ Phase 3 (midx/bitmap/cg)         needs: Phase 2
   └─▶ Phase 4 (object model + revwalk) needs: Phase 1, Phase 3 (commit-graph)
   └─▶ Phase 5 (diff)                   needs: Phase 4 (tree walking)
   └─▶ Phase 6 (index & worktree)       needs: Phase 4
   └─▶ Phase 7 (refs & reftable)        needs: Phase 0
   └─▶ Phase 8 (merge & reachability)   needs: Phase 4, Phase 5, Phase 6, Phase 7
   └─▶ Phase 9 (conversion & fsck)      needs: Phase 1..Phase 8
```

**Part 1 (Phases 0 + 1) is the first deliverable** and is expanded in the most
detail. The automated-test infrastructure ([test-infrastructure.md](test-infrastructure.md))
is bootstrapped during Phase 0 so every subsequent phase lands with its gates
already wired.

## Shared acceptance criteria (every phase)

A phase is **done** when all of the following are machine-checkable and green:

1. `cargo test --workspace` — unit + doc tests pass.
2. Proptest suites pass (parsers/serializers never panic, round-trip invariants hold).
3. Differential suite passes — Rust CLI output byte-identical to C git on the
   same inputs.
4. Crosswise on-disk compatibility passes — C↔Rust read/write interop, verified
   with C `git fsck`/`verify`/`cat-file` where applicable.
5. Coverage gate ≥ 90% line coverage on the phase's crates (`cargo llvm-cov`).
6. No regression on the committed `t/` scoreboard baseline
   (`cargo xtask scoreboard`).
7. The phase's listed `t/` scripts pass 100% through the shim dispatcher.
8. Fuzz targets for the phase's parsers run clean for a fixed CI budget.

See [test-infrastructure.md](test-infrastructure.md) for the tooling that makes
criteria 1–8 one-command, CI-enforced, and regression-tracked.

## Reading order

1. [test-infrastructure.md](test-infrastructure.md) — the test/oracle machinery.
2. [FOLLOWUPS.md](FOLLOWUPS.md) — the actionable backlog (deferred code, infra, deviations).
   3. [gap-analysis.md](gap-analysis.md) — systematic C-vs-Rust gap comparison.
   4. [conversion-plan.md](conversion-plan.md) — the ordered plan to convert everything remaining.
   5. [test-completion-plan.md](test-completion-plan.md) — roadmap from current test state to full acceptance criteria.
3. [phase-0-foundation.md](phase-0-foundation.md), [phase-1-loose-objects.md](phase-1-loose-objects.md) — Part 1.
4. Phases 2–9 in dependency order.
