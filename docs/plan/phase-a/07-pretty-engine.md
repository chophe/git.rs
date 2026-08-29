# A7 — Pretty-Printing Engine (`pretty.c`)

Parent plan: [00-overview.md](00-overview.md). Backlog ref: FOLLOWUPS.md §D
Phase 4 "Remaining": pretty-printing, `--format`, trailers, mailmap.

## Goal

A `git-pretty` crate implementing C git's commit/object formatting language:
built-in formats (`oneline`, `short`, `medium`, `full`, `fuller`, `raw`,
`reference`, `format:`, `tformat:`), `%(placeholder)` interpolation, date
formatting, trailers, and mailmap. Consumers: `log` (A6), `show` (B9),
`shortlog` (B9), `format-patch` (F).

## Current Rust state (observed)

- `log.rs` hard-codes the `--oneline` rendering.
- No format-language code anywhere in the workspace.

## C reference

- `pretty.c` (the entire engine: `pp_*` functions, placeholder expansion,
  `format_commit_message`), `date.c` (`DATE_FORMATS`, approxidate for
  `--date=`), `trailer.c` (trailer parsing/rewriting), `mailmap.c`.
- Gate scripts: `t/t4205-log-pretty-formats.sh`, `t/t6006-rev-list-format.sh`,
  `t/t7513-interpret-trailers.sh` (shared engine), `t/t4203-mailmap.sh`.

## Deliverables

1. Format specifier parser: `%H %h %T %t %P %p %an %ae %aN %aE %ad %aD
   %ar %at %ai %aI %cn %ce %cN %cE %cd %ci %s %f %b %B %N %e %GG %n %%
   %x## %w(width) %C(...) %m %+-%-space toggles`, and the `<()` / `>()`
   alignment/wrap directives.
2. Date formatting: all `--date=` formats (`relative`, `local`, `iso`,
   `iso-strict`, `rfc`, `short`, `raw`, `human`, `unix`, `format:`,
   `default`) on top of A12's timezone-correct dates.
3. Trailer support: `%(trailers[:options])` with the same option grammar as
   `git interpret-trailers` (`key=`, `only`, `separator=` etc.).
4. Mailmap: `.mailmap` parsing and application to `%aN`/`%aE`/`%cN`/`%cE`.
5. Error contract: unknown format specifiers and malformed `%(...)` produce
   C-identical stderr/exit code.

## Sub-tasks (ordered)

1. Placeholder tokenizer + `%x##`/`%%` byte escapes; proptest: no panics on
   arbitrary format strings.
2. Identity/date fields (needs A12); `--date=` format table.
3. Body/subject handling with the exact folding and `%w()` wrapping
   semantics (C's `strbuf_add_wrapped_text` in `utf8.c`).
4. Color directives (`%C(...)`) honoring `--color` plumbing (store state,
   emit only when enabled — matches C's plumbing default).
5. Trailers (extract the parser from `t/t7513` expectations).
6. Mailmap last.

## Test gates

- `t/t4205`, `t/t6006`, `t/t4203` (mailmap subset), `t/t7513` (trailer
  subset) via the shim.
- Crosswise suite: golden commits rendered through ~50 format strings,
  byte-identical.

## Risks / notes

- `%h` abbreviation length is `core.abbrev`-dependent and must use A4's
  disambiguation logic; `%f` subject-sanitization rules have many edge cases
  (punctuation stripping) — extract them from `pretty.c` `format_subject`.
- This crate is a long-lived dependency of Phases B/F; invest in the
  proptest harness here.
