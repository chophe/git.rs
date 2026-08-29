# A0 — Phase A Overview: Deepen What's Routed

Parent plan: [conversion-plan.md](../conversion-plan.md). Evidence base:
[gap-analysis.md](../gap-analysis.md).

## Purpose

Phase A fixes the depth problem before broadening the port. The gap analysis
showed that every currently routed command is shallow (e.g. `diff` handles 2
options vs ~100 in C, `status` only `--porcelain`). Building Phase B
(`add`, `commit`, `checkout`) on shallow foundations would multiply rework.

## Items and their files

| # | Item | File | Dependencies |
|---|---|---|---|
| A1 | sha1dc collision-detecting SHA-1 | [01-sha1dc.md](01-sha1dc.md) | none |
| A2 | `--git-dir`/`--work-tree` threading | [02-repo-discovery-env.md](02-repo-discovery-env.md) | **first** |
| A3 | `cat-file --batch`/`--batch-check`/`%(format)` | [03-cat-file-batch.md](03-cat-file-batch.md) | none |
| A4 | abbreviation resolution | [04-abbrev-resolution.md](04-abbrev-resolution.md) | needs `git-refs` (done) |
| A5 | `rev-parse` completion | [05-rev-parse-completion.md](05-rev-parse-completion.md) | A4 |
| A6 | `rev-list`/`log` options | [06-rev-list-log-options.md](06-rev-list-log-options.md) | A4, A5, A7 |
| A7 | pretty-printing engine | [07-pretty-engine.md](07-pretty-engine.md) | A12 for date formats |
| A8 | diff engine completion | [08-diff-options.md](08-diff-options.md) | none |
| A9 | userdiff hunk headers | [09-userdiff-hunk-headers.md](09-userdiff-hunk-headers.md) | A8 |
| A10 | `count-objects -v` close-out | [10-count-objects-v.md](10-count-objects-v.md) | none (verify-only) |
| A11 | `.gitignore` + attributes engine | [11-gitignore-attributes.md](11-gitignore-attributes.md) | none |
| A12 | local timezone dates/idents | [12-local-timezone-dates.md](12-local-timezone-dates.md) | none |
| A13 | pack delta compression on write | [13-pack-delta-compression.md](13-pack-delta-compression.md) | none |

## Suggested order

1. **A2 first** — a shared repo-context struct threaded through every command
   means all later commands are born with the right environment handling.
2. Then the independent leaves: A1, A3, A8, A10, A11, A12, A13 (any order;
   A8 is the largest and most valuable).
3. A4 → A5 → A7 → A6 (resolution → rev-parse → pretty → rev-walk options).
4. A9 last (depends on A8).

## Shared gates (apply to every item)

1. `cargo test --workspace` green; new proptests for any parser/serializer.
2. Crosswise suite byte-identical against system C git for every CLI-visible
   change; suite registered in `xtask::suites()`.
3. `cargo xtask scoreboard` — no regression against the committed baseline.
4. The item's named `t/` scripts pass through `scripts/shim-git`.
5. ≥90% line coverage on touched crates (`cargo llvm-cov`).
6. FOLLOWUPS.md updated (mark DONE or add new deviations found).
