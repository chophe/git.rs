# A12 — Local-Timezone Dates & Idents

Parent plan: [00-overview.md](00-overview.md). Backlog ref: FOLLOWUPS.md
§A6 (`git-date` UTC-only, month/year relative approximations) and §A7
(`ident.rs` writes `+0000` instead of the local offset).

## Goal

Timezone-correct date handling everywhere dates are produced or parsed:
parsed dates without an explicit offset use the local timezone (with C's
TZ-database semantics), idents record the local offset, and `git-date`
relative math is calendar-aware.

## Current Rust state (observed)

- `crates/git-date`: tz-less inputs are treated as UTC; `month`/`year`
  relative units approximated as 30/365 days.
- `crates/git-command/src/ident.rs`: `now_utc()` writes `+0000`.

## C reference

- `date.c` (the parser: `parse_date`, `approxidate`, the `tm`-based local
  time handling, `DATE_NORMAL` formatting with `+HHMM`), `ident.c` (ident
  string construction with `local_tzoffset`).
- Gates: `t/t0006-date.sh` (approxidate + format parity), `t/t4212-log-date.sh`.

## Deliverables

1. Timezone source: use the OS (macOS/Linux `localtime`/TZ env semantics) to
   produce the offset for a given timestamp — C git calls `localtime_r` per
   conversion, so DST correctness per-date matters (an offset captured
   once at startup is wrong across DST boundaries).
2. `git-date`: parsing tz-less inputs in local time; relative `month`/`year`
   via calendar arithmetic (same-day-of-month clamping like C's `tm`
   normalization); all `--date=` output formats consume this (A7 depends).
3. `ident.rs`: emit `+HHMM` of the local zone at commit time; respect
   `GIT_AUTHOR_DATE`/`GIT_COMMITTER_DATE` with their full accepted formats.
4. Round-trip guarantee: for any timestamp, `format(parse(format(t)))` is
   stable in C git's formats — proptest it.

## Sub-tasks (ordered)

1. Timezone abstraction (per-timestamp local offset + name); decide: pure
   Rust TZ database port vs `chrono`-style dependency — **check what the
   workspace already uses** (`git-date` currently has no deps) and prefer a
   small pure-Rust tzfile (`/usr/share/zoneinfo`) reader over adding a
   heavyweight dependency, matching the no-FFI/pure-Rust house style.
2. Parser fixes (`t/t0006` cases for tz-less parsing).
3. Calendar-aware relative math.
4. Ident offsets + `GIT_*_DATE` env parsing.
5. `--date=` formats (hand off to A7's table once both land).

## Test gates

- `t/t0006-date.sh`, `t/t4212-log-date.sh` via the shim.
- Crosswise: run `git var GIT_AUTHOR_IDENT` / ident-emitting commands under
  `TZ=` variations (`UTC`, `America/New_York` across a DST transition) and
  compare bytes.

## Risks / notes

- TZ database semantics are a rabbit hole; scope strictly to what `date.c`
  does (`localtime_r` + tzname), not full ICU. The tzfile reader only needs
  `TTinfo`/transition lookup, not all of TZif.
- Commit objects written before this fix use `+0000` — they are valid
  objects (C git accepts them); do not rewrite fixtures silently.
