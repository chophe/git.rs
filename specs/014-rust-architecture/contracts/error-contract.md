# Contract: Errors → Diagnostics → Exit Codes

**Feature**: `specs/014-rust-architecture/spec.md` (FR-024, SC-003; Clarifications 2026-09-20) | **Date**: 2026-09-20

## Per-component error enums

- Every component exposes its own error enum with flat, specific variants: I/O, not-found, corrupt, ambiguous, locked, invalid, plus component-specific causes.
- Untyped catch-all variants (`Other`, `Misc`, `Box<dyn Error>` as a new error) MUST NOT be used for new errors — they erase layer attribution.
- Errors carry the failing layer and the subject (e.g. store layer + object id), never a bare string.

## Edge rendering (observability = C-git parity)

- Libraries return typed errors only and perform no logging, no metrics, no tracing.
- All human diagnostics render at the CLI/surface edge (`git-cli` + command modules) onto stderr, matching C git's observable wording class (SHOULD-level prose: same meaning; exact bytes where the differential suites pin them).
- No metrics/tracing signals exist beyond what C git emits. None are added by this feature.

## Exit-code classes (C-git parity)

| Class | Meaning | Examples |
|---|---|---|
| 0 | success | clean command completion |
| 1 | generic error / unknown command | runtime failure, unrecognized subcommand |
| 129 | usage error | bad flags/operands (mechanical, before repository work) |
| 128+ | fatal / signal behavior | as C git emits on identical inputs (oracle decides) |

Fault-injection drills (SC-003): one injected fault per layer (bad object bytes, missing ref, corrupt index, bad config, …) MUST surface an error naming the failing layer while preserving the exit-code class end to end, 100% of drills.
