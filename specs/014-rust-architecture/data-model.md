# Data Model: Rust Workspace Architecture

**Feature**: `specs/014-rust-architecture/spec.md` | **Date**: 2026-09-20

This feature introduces **no new on-disk state** and changes **no formats**. The entities below are architectural (ownership, handles, contracts) — they describe where behavior lives and what crosses boundaries, per `contracts/placement.md`.

## Entity 1: Component

One workspace crate with a single owning responsibility, declared dependencies, an explicit error type, and stand-alone tests (spec Key Entities).

| Field | Type / Rule | Validation |
|---|---|---|
| name | `git-*` crate name, kebab-case | Unique across workspace; one entry in `crates/Cargo.toml` members |
| responsibility | Single boundary from `contracts/placement.md` | Exactly one owner per boundary (SC-005: zero overlaps, zero gaps) |
| dependencies | Declared path deps in crate `Cargo.toml` | Acyclic; point downward through layers only (`contracts/dependency-rules.md`) |
| error type | Per-crate enum (flat, specific variants) | No untyped catch-all for new errors; maps to exit classes (`contracts/error-contract.md`) |
| tests | In-crate unit + `tests/` integration | Pass stand-alone in bare dir, scrubbed env (SC-002); no cross-component fixtures |

**State transitions**: proposed → scaffolded (probe compiles, gates pass) → owned (full behavior + differential parity) → never split without a ratified spec change.

## Entity 2: Layer

Foundation → mid → store → language → access → surface, plus automation (`xtask`) outside the runtime graph.

| Layer | Members (current + reserved) |
|---|---|
| foundation | `git-hash`, `git-varint`, `git-date` (+ config primitives where they live) |
| mid | `git-config`, `git-object`, `git-core` (discovery + `Repository`), `git-commitgraph`, `git-diff`, `git-merge`, `git-index`, `git-attributes`, `git-pretty` |
| store | `git-odb`, `git-refs` (storage formats + atomic operations) |
| language | `git-revision`, diff/merge/attributes/pathspec consumers (`git-pathspec` reserved) |
| access | `git-compress`, `git-worktree`, `git-transport`, `git-protocol`, `git-hooks`, `git-credentials` (all reserved/new) |
| surface | `git-command` modules (plumbing + porcelain), `git-cli` (composition root + thin binary) |

**Rule**: A component may depend only on its own layer or lower layers; `git-command` (composition root) is the sole exception allowed broad dependencies. New edges violating this fail the dependency gate.

## Entity 3: Repository handle (explicit context)

The explicitly passed context every operation receives instead of reading process globals (FR-023/FR-027).

| Field | Owner | Notes |
|---|---|---|
| directories (`git_dir`, `work_tree`, `commondir`) | discovery (`git-core`) | Upward search, `gitdir:` following, bare detection resolved once at CLI edge |
| bare flag, object algorithm | repository storage (`git-core`) | Layout knowledge; hands out scoped store handles |
| merged config + `-c` overrides | config (`git-config`) | Layered merge with origin attribution; file paths supplied by discovery, never found by config |
| worktree identity (reserved) | `git-worktree` slot | Extends the handle; never a process-global lookup |
| transport endpoint / credential helper (reserved) | `git-transport` / `git-credentials` slots | Extend the handle when those subsystems land |

**Validation**: Constructible from plain values in tests; reproducible by passing the same values (no hidden env/cwd reads past the edge). Test-only global mutation stays in harnesses with serializing guards.

## Entity 4: Store handle (scoped accessor)

Scope-borrowed accessor (`object` / `ref` / `index`) performing atomic operations on behalf of callers.

| Field | Rule |
|---|---|
| scope | Borrows the repository handle; never outlives it; owns no paths itself |
| mutation | Only through owning-component atomic ops (ref transactions, index locks, config writes); never shared `&mut` across components |
| visibility | ODB hides physical location (loose/alternates/quarantine/pack/midx/commit-graph) behind "give me object X" |

## Entity 5: Boundary contract

Documented data crossing between two components (values in, verdicts out) that conformance tests pin. The full registry is `contracts/placement.md`; error/diagnostic crossings are `contracts/error-contract.md`.

| Field | Rule |
|---|---|
| inputs | Plain values or borrowed views (`&[u8]`/`&str` where caller retains data — FR-026); bulk inputs stream |
| outputs | Values or per-component error variants; buffering APIs labeled as buffering |
| traits | Only with recorded justification (≥2 real impls or genuine test-double need — FR-025) |

## Entity 6: Safety gate record

| Field | Rule |
|---|---|
| `unsafe` blocks | Zero in first-party code (verified baseline); each future one carries justification + safe wrapper + invariants + targeted tests |
| dependency check | Zero cycles, zero layer violations per change |
| toolchain | Warning-free build at MSRV 1.74; new deps record need + license + MSRV impact |

Relationships: **Layer** contains **Components**; **Component** exposes one **error type** and crossings defined by **Boundary contracts**; operations receive a **Repository handle** and borrow **Store handles**; the **Safety gate record** constrains every change. No entity maps to on-disk bytes — format ownership stays with the existing store components byte-for-byte.
