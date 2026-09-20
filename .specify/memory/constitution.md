<!--
Sync Impact Report (review scratch — remove before commit):
- Version change: unversioned template → 1.0.0 (initial ratification)
- Modified principles: none (no prior ratified content; template placeholders replaced)
- Added sections: Core Principles I–V; Compatibility Requirements; Development Workflow;
  Governance — all derived from the ratified project principle plus the repository's
  already-locked decisions (product spec 002, master spec 014, docs/plan).
- Removed sections: none (template slots filled, not removed).
- Follow-up TODOs: none. Open item: constitution-template layers untouched per skill scope.
-->

# git.rs Constitution

## Core Principles

### I. Behavioral Fidelity (NON-NEGOTIABLE)

git.rs is NOT a Git-inspired version-control system. It is a Rust reimplementation
of Git.

Whenever there is a choice between inventing a cleaner behavior, designing a more
convenient API, simplifying a Git format, or changing a Git command's semantics,
and preserving documented or observed Git behavior, the implementation MUST
preserve Git behavior unless the project specification explicitly declares a
compatibility boundary.

Git's C implementation MUST NOT be translated mechanically. Git is the behavioral
specification; Rust is the implementation language.

Compatibility is measured against real Git behavior, repository formats, command
semantics, interoperability, and tests — never by similarity of source code.

### II. Standalone Implementation

The Rust binary MUST never call into C Git, and the C Git binary MUST never be
linked into the Rust build. The C tree is reference material and test oracle
only. This rule is not negotiable and is not a temporary state.

### III. Evidence Over Assertion

The `t/` suite is the oracle: where C source and the suite disagree, the suite
wins. No compatibility claim stands without demonstration — parity MUST be shown
by differential suites, crosswise checks, or upstream scripts, or the capability
MUST be explicitly marked incomplete. Presence of a command in the dispatcher
never implies parity.

### IV. Explicit Boundaries, Honest Failure

Every known divergence MUST be a logged backlog entry naming its reason and
affected suites. Uncovered commands and options MUST fail honestly (unknown
command, usage error, unsupported-capability diagnostic) — silent approximation
of unimplemented semantics is a defect, more dangerous than rejection.

### V. Rust-Native Internals, Fixed Observables

Crate boundaries, module layout, memory management, concurrency, and error
plumbing are Rust-native and free to differ from C. They MUST NOT change output
bytes, exit codes, or on-disk formats. No user-facing option, format, or
behavior may be invented that Git lacks.

## Compatibility Requirements

Obligation levels from the product specification apply to all work: MUST
(non-negotiable, machine-verified — exit codes, machine-readable output,
bidirectional on-disk compatibility, no regression against the committed
scoreboard baseline, buildable and testable at every commit); SHOULD (expected
with recorded exceptions — diagnostic wording, human prose, performance within
the same order of magnitude, environment and configuration handling).

All writes MUST be atomic (temp file plus rename under lock discipline).
Parsers MUST be total on arbitrary input. Received and incoming state MUST be
validated before acceptance.

## Development Workflow

Specification before implementation: the master specification is the source of
truth; subsystem specifications detail behavior within its boundaries; conflicts
are resolved there first under the fixed priority (repository constraints and
locked decisions, then Git compatibility, then tests and observed behavior, then
Rust-native considerations).

Phase gates are machine-checkable: workspace tests green, phase differential
suites green, no scoreboard regression, coverage at the project threshold. The
scoreboard baseline updates only alongside intentional behavior change, never to
mask failure.

## Governance

This constitution supersedes all other practices. Amendments require
documentation, a version bump, and a migration note for affected specifications;
governance-principle removals or redefinitions bump MAJOR, new principles or
materially expanded guidance bump MINOR, clarifications bump PATCH. All reviews
MUST verify constitutional compliance. Complexity MUST be justified against
Principle I: any deviation from Git behavior needs an explicit compatibility
boundary, not an engineering preference.

**Version**: 1.0.0 | **Ratified**: 2026-09-20 | **Last Amended**: 2026-09-20
