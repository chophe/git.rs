# Contract: Boundary Placement (21 boundaries → one owner each)

**Feature**: `specs/014-rust-architecture/spec.md` (FR-001–FR-021, SC-005) | **Date**: 2026-09-20

Rule: for every boundary exactly one component answers "I own this". Overlaps and gaps fail the placement drill. `(existing)` = keep current home. `(reserved)` = new scaffold crate; probe must compile with zero edits to neighbors.

| # | Boundary | Owner | May depend on | Boundary data (in → out) |
|---|---|---|---|---|
| FR-001 | CLI | `git-cli` (existing, thin) | `git-command` only | argv/env/cwd (read once) → single dispatch + exit code; no repo/storage/format logic |
| FR-002 | configuration | `git-config` (existing) | nothing (foundation-level) | file paths + bytes → layered merged config, typed reads, origin attribution |
| FR-003 | repository discovery | `git-core` discovery (existing) | `git-config` (paths only), `git-hash` (algorithms) | cwd/env/overrides → explicit `Repository` context; never reads object/ref/index content |
| FR-004 | repository storage | `git-core` `Repository` (existing) | `git-config`, `git-hash` | context → control paths, common-vs-per-worktree routing, scoped store handles; no format logic |
| FR-005 | object database | `git-odb` (existing) | `git-hash`, `git-object`, `git-core`, `git-commitgraph`, `git-compress` (new) | object id → bytes/type (location-hidden: loose/alternates/quarantine/pack/midx) |
| FR-006 | hashing | `git-hash` (existing) | nothing | bytes/stream → digest; id string ⇄ parsed id + validation; sole digest/validation authority |
| FR-007 | compression | `git-compress` (reserved) | `flate2` (sole provider, impl detail) | byte stream ⇄ deflate stream with size caps; only streaming boundary for zlib use |
| FR-008 | refs | `git-refs` (existing) | `git-hash`, `git-core` | refname + op → transaction result; locking/reflog/packed-refs hidden inside |
| FR-009 | index | `git-index` (existing) | `git-hash` | entries/stat facts ⇄ freshness verdicts, atomic read/write; extensions/checksum inside |
| FR-010 | revision parsing | `git-revision` resolver (existing) | `git-object`, `git-odb`, `git-refs`, `git-core` | rev expression + disambiguation rules → resolved ids; no private per-command grammars |
| FR-011 | pathspec | `git-pathspec` (reserved) | `git-core` (config/attributes input as values) | patterns + magic + NUL input → match verdicts per path; sole glob authority |
| FR-012 | attributes | `git-attributes` (existing) | `git-core`, `git-config` | path + stack → ignore/attr/filter verdicts; no per-consumer reimplementation |
| FR-013 | diff | `git-diff` (existing) | `git-hash`, `git-object` | object pairs + options → hunks/rename/binary verdicts; no comparison code in commands |
| FR-014 | merge | `git-merge` (existing) | `git-object`, `git-diff` | tips + options → merge-base, line merges, conflict representation; persistence via index/refs only |
| FR-015 | transport | `git-transport` (reserved) | store/language layers only | endpoint + want/have → pack stream + progress; C-git retry/timeout/progress parity; store never depends on it |
| FR-016 | protocols | `git-protocol` (reserved) | nothing above transport | byte stream ⇄ pktline/capabilities/version-negotiation frames; testable on canned streams |
| FR-017 | worktree | `git-worktree` (reserved) | `git-index`, `git-core` | index entries + intent → created files/modes/symlinks/stat refresh; index bytes meet worktree bytes only here |
| FR-018 | hooks | `git-hooks` (reserved) | `git-core` (paths/config as values) | hook point + env/skip-force policy → run/skip verdict + effects; absent hooks ≡ identical behavior |
| FR-019 | credentials | `git-credentials` (reserved) | `git-core` (config values, never secrets-in-config) | host/context → secret (C-git helper scope); no secret bytes in config/logs/errors beyond C exposure |
| FR-020 | plumbing commands | `git-command` modules (existing) | only libraries each contract needs | args + context + output sink → machine-readable bytes + `CommandError` |
| FR-021 | porcelain commands | `git-command` modules (existing) | same libraries + plumbing contracts (never display text) | args + context + output sink → human formatting isolated from scripted behavior |

Conformance: `cargo xtask` placement drill — for each row, a probe in the owner's slot compiles with zero edits to any other component; "which component owns X?" has exactly one answer.
